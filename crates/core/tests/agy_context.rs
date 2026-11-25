// fac-681, fac-629: agy standing-section holding across resumes, compaction/cut clearing, and effective context_cap.

// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply};
use factory_core::agents::turns;
use factory_core::decide::{Event, JobEnd, SessionSeen};
use factory_types::config::{Cli, Effort, SeatOverride};
use factory_types::state::{AgentStatus, Sender};
use factory_types::{AgentKey, SessionId};
use serde_json::{Value, json};

const CARD: &str = "web-1";
const FOREMAN: &str = "web-1/foreman";
const PLANNER: &str = "web-1/planner";
const AGY_ARGV: &[&str] = &["agy", "--model", "gemini-4-neon-high"];
const PLAN_TEXT: &str = "- where: src\n---\nThe full brief plan text.";

fn agy_seat() -> SeatOverride {
    SeatOverride {
        cli: Some(Cli::Agy),
        model: Some("gemini-4-neon".into()),
        effort: Some(Effort::High),
        ..SeatOverride::default()
    }
}

fn start_agy_card(no_planner: bool) -> Sim {
    let mut s = Sim::new();
    s.config.seats.insert("foreman".into(), agy_seat());
    s.config.seats.insert("planner".into(), agy_seat());
    s.board(CARD, "web");
    let start = if no_planner {
        json!({"start": {"card": CARD, "no-planner": true}})
    } else {
        json!({"start": {"card": CARD}})
    };
    s.andrei(start).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, factory_types::work::Purpose::Clone { .. }))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    s
}

fn assert_has_all(prompt: &str, needles: &[&str]) {
    for needle in needles {
        assert!(prompt.contains(needle), "missing `{needle}` in:\n{prompt}");
    }
}

fn agy_session(thread: &str) -> SessionSeen {
    SessionSeen { started: true, thread: Some(thread.into()), answered: true, ..SessionSeen::default() }
}

fn run_agy_turn(s: &mut Sim, agent: &str, r: Value, session: SessionSeen) -> String {
    let (id, prompt) = s.turn_started_with_argv(agent, AGY_ARGV);
    s.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Ok(r), session } });
    prompt
}

fn recap_reply() -> Value {
    let mut r = reply("brief-ready", "The plan");
    r["hand"]["message"] = json!(PLAN_TEXT);
    r
}

fn wake_planner_from_foreman(s: &mut Sim, text: &str) {
    s.cmd(
        Sender::Agent(AgentKey::new(FOREMAN)),
        json!({"say": {"place": format!("{CARD}/challenge"), "to": "planner", "kind": "request", "text": text}}),
    )
    .unwrap();
}

#[test]
fn agy_holds_standing_sections_across_resumes_and_resends_after_compaction() {
    let mut s = start_agy_card(true);

    let first = run_agy_turn(
        &mut s,
        FOREMAN,
        reply("working", "Ok"),
        SessionSeen { context: Some(50_000), ..agy_session("conv-1") },
    );
    assert!(first.contains("## Delegation"), "{first}");
    assert!(first.contains("## Allowed models"), "{first}");
    assert!(first.contains("Not reported yet in this session."), "{first}");
    assert!(!s.state.agents[FOREMAN].held.is_empty());

    // Resumed agy turn omits unchanged standing sections and reports context against the cap.
    s.andrei(json!({"say": {"place": CARD, "text": "Continue."}})).unwrap();
    let second = run_agy_turn(
        &mut s,
        FOREMAN,
        reply("working", "Ok"),
        SessionSeen { context: Some(15_000), compacted: true, ..agy_session("conv-1") },
    );
    assert!(!second.contains("## Delegation"), "held section repeated:\n{second}");
    assert!(!second.contains("## Allowed models"), "held section repeated:\n{second}");
    assert!(second.contains("50000 of 200000 tokens (25%), at your last turn's end."), "{second}");
    assert!(s.state.agents[FOREMAN].held.is_empty(), "compaction must clear held sections");

    // After compaction, standing sections are sent again and context reflects the post-compaction size.
    s.andrei(json!({"say": {"place": CARD, "text": "Next."}})).unwrap();
    let third = turns::plan(&s.state, &AgentKey::new(FOREMAN), s.ctx(), false).unwrap().prompt;
    assert!(third.contains("## Delegation"), "standing section missing after compaction:\n{third}");
    assert!(third.contains("## Allowed models"), "standing section missing after compaction:\n{third}");
    assert!(third.contains("15000 of 200000 tokens (7%), at your last turn's end."), "{third}");
}

#[test]
fn context_line_uses_effective_cap_for_agy_and_claude_and_keeps_codex_unknown() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    let key = AgentKey::new(FOREMAN);

    // Claude with 1M reported window uses the 200k cap, and falls back to 1M when cap is 0.
    s.andrei(json!({"pick": {"agent": FOREMAN, "cli": "claude", "model": "claude-opus-5-5", "effort": "high"}}))
        .unwrap();
    let a = s.state.agents.get_mut(FOREMAN).unwrap();
    a.choice = Some(factory_types::state::Choice {
        cli: Cli::Claude,
        model: "claude-opus-5-5".into(),
        effort: Effort::High,
        by: "andrei".into(),
        phase: Some(factory_types::config::SeatPhase::Work),
    });
    a.pending = None;
    a.context = Some(150_000);
    a.window = Some(("claude-opus-5-5".into(), 1_000_000));
    a.owed = Some(factory_types::state::Owed::Wake);

    let claude_capped = turns::plan(&s.state, &key, s.ctx(), false).unwrap().prompt;
    assert!(claude_capped.contains("150000 of 200000 tokens (75%), at your last turn's end."), "{claude_capped}");

    s.config.factory.context_cap = 0;
    let claude_uncapped = turns::plan(&s.state, &key, s.ctx(), false).unwrap().prompt;
    assert!(claude_uncapped.contains("150000 of 1000000 tokens (15%), at your last turn's end."), "{claude_uncapped}");

    // agy ignores any stale Claude window: without cap prints tokens alone; with cap prints used of cap.
    let a = s.state.agents.get_mut(FOREMAN).unwrap();
    a.choice.as_mut().unwrap().cli = Cli::Agy;
    a.choice.as_mut().unwrap().model = "gemini-4-neon".into();
    let agy_uncapped = turns::plan(&s.state, &key, s.ctx(), false).unwrap().prompt;
    assert!(agy_uncapped.contains("150000 tokens, at your last turn's end."), "{agy_uncapped}");

    s.config.factory.context_cap = 300_000;
    let agy_capped = turns::plan(&s.state, &key, s.ctx(), false).unwrap().prompt;
    assert!(agy_capped.contains("150000 of 300000 tokens (50%), at your last turn's end."), "{agy_capped}");

    // Codex stays unknown.
    let a = s.state.agents.get_mut(FOREMAN).unwrap();
    a.choice.as_mut().unwrap().cli = Cli::Codex;
    a.choice.as_mut().unwrap().model = "gpt-6.2-sol".into();
    let codex = turns::plan(&s.state, &key, s.ctx(), false).unwrap().prompt;
    assert!(codex.contains("Unknown: Codex reports no context size."), "{codex}");
}

#[test]
fn foreman_cut_resends_challenge_plan_and_sections_through_retry_and_second_cut() {
    let mut s = start_agy_card(false);
    run_agy_turn(&mut s, PLANNER, recap_reply(), agy_session("conv-p1"));

    // Foreman reads the waiting challenge and raises an ask, keeping the challenge open and the plan held.
    let mut ask_reply = reply("working", "Question");
    ask_reply["hand"]["ask"] = json!("Clarify one detail?");
    let f1 = run_agy_turn(&mut s, FOREMAN, ask_reply, agy_session("conv-f1"));
    assert!(f1.contains(PLAN_TEXT), "initial challenge missing plan:\n{f1}");
    assert_eq!(s.state.agents[FOREMAN].held.get("plan").map(String::as_str), Some(PLAN_TEXT));

    // Resumed foreman turn omits held standing sections and plan; cutting it clears held while keeping session facts.
    s.andrei(json!({"say": {"place": CARD, "text": "Proceed with the challenge."}})).unwrap();
    let (f2_id, f2_prompt) = s.turn_started_with_argv(FOREMAN, AGY_ARGV);
    assert!(!f2_prompt.contains("## Allowed models") && !f2_prompt.contains(PLAN_TEXT), "{f2_prompt}");

    s.apply(Event::JobEnded {
        id: f2_id,
        end: JobEnd::Cut { session: SessionSeen { tools: true, ..agy_session("conv-f2") } },
    });
    let f = &s.state.agents[FOREMAN];
    assert!(f.held.is_empty() && f.session_started && f.ran.tools);
    assert_eq!(f.session, Some(SessionId::new("conv-f2")));

    let expected = [
        "On a full card the planner settles specification questions.",
        "## Allowed models",
        "## The challenge",
        "## The reply",
        PLAN_TEXT,
    ];
    let (retry_id, retry_prompt) = s.turn_started_with_argv(FOREMAN, AGY_ARGV);
    assert_has_all(&retry_prompt, &["# Your standing sections, again"]);
    assert_has_all(&retry_prompt, &expected);

    // A second cut in a row leaves the seat Cut with held cleared; resuming it resends sections and full plan again.
    s.apply(Event::JobEnded { id: retry_id, end: JobEnd::Cut { session: SessionSeen::default() } });
    assert!(s.state.agents[FOREMAN].status == AgentStatus::Cut && s.state.agents[FOREMAN].held.is_empty());
    s.andrei(json!({"resume": {"agent": FOREMAN}})).unwrap();
    let after_second_cut = turns::plan(&s.state, &AgentKey::new(FOREMAN), s.ctx(), false).unwrap();
    assert!(after_second_cut.resume);
    assert_has_all(&after_second_cut.prompt, &expected);
}

#[test]
fn planner_cut_resends_brief_draft_and_sections_on_retry() {
    let mut s = start_agy_card(false);
    run_agy_turn(&mut s, PLANNER, recap_reply(), agy_session("conv-p1"));
    assert_eq!(s.state.agents[PLANNER].held.get("plan").map(String::as_str), Some(PLAN_TEXT));

    wake_planner_from_foreman(&mut s, "Check this assumption.");
    let (p2_id, p2_prompt) = s.turn_started_with_argv(PLANNER, AGY_ARGV);
    assert!(!p2_prompt.contains("## Delegation") && !p2_prompt.contains(PLAN_TEXT), "{p2_prompt}");

    s.apply(Event::JobEnded { id: p2_id, end: JobEnd::Cut { session: SessionSeen::default() } });
    assert!(s.state.agents[PLANNER].held.is_empty());
    let p_retry = turns::plan(&s.state, &AgentKey::new(PLANNER), s.ctx(), false).unwrap();
    assert_has_all(
        &p_retry.prompt,
        &["# Your standing sections, again", "## Delegation", "## Allowed models", "## The reply", PLAN_TEXT],
    );
}

#[test]
fn session_seen_clears_held_only_when_compacted() {
    let mut s = start_agy_card(true);
    run_agy_turn(&mut s, FOREMAN, reply("working", "Ok"), agy_session("conv-1"));
    assert!(!s.state.agents[FOREMAN].held.is_empty());

    s.andrei(json!({"say": {"place": CARD, "text": "Continue."}})).unwrap();
    let (id, _) = s.turn_started_with_argv(FOREMAN, AGY_ARGV);

    // Non-compacted SessionSeen (emitted before a normal JobEnded) keeps held sections.
    s.apply(Event::SessionSeen {
        id: id.clone(),
        session: SessionSeen { started: true, thread: Some("conv-1".into()), ..SessionSeen::default() },
    });
    assert!(!s.state.agents[FOREMAN].held.is_empty(), "non-compacted SessionSeen must keep held sections");

    // Compacted SessionSeen (even without started/thread, before Stopped) clears held sections.
    s.apply(Event::SessionSeen { id: id.clone(), session: SessionSeen { compacted: true, ..SessionSeen::default() } });
    s.apply(Event::JobEnded { id, end: JobEnd::Stopped });
    assert!(s.state.agents[FOREMAN].held.is_empty(), "compacted SessionSeen must clear held sections");

    s.andrei(json!({"say": {"place": CARD, "text": "Again."}})).unwrap();
    let next = turns::plan(&s.state, &AgentKey::new(FOREMAN), s.ctx(), false).unwrap();
    assert!(next.prompt.contains("## Delegation"), "{}", next.prompt);
    assert!(next.prompt.contains("## Allowed models"), "{}", next.prompt);
    assert!(next.prompt.contains("## The reply"), "{}", next.prompt);
}

fn detached_planner_turn() -> (Sim, factory_types::JobId) {
    let mut s = start_agy_card(false);
    run_agy_turn(&mut s, PLANNER, recap_reply(), agy_session("conv-p1"));
    run_agy_turn(&mut s, FOREMAN, reply("working", "No objection"), agy_session("conv-f1"));
    wake_planner_from_foreman(&mut s, "One quick note while awaiting approval.");
    let (detached, _) = s.turn_started_with_argv(PLANNER, AGY_ARGV);
    s.andrei(json!({"approve": {"card": CARD}})).unwrap();
    assert!(s.state.jobs[&detached].detached);
    assert!(!s.state.agents[PLANNER].held.is_empty());
    (s, detached)
}

#[test]
fn detached_planner_brief_turn_clears_held_on_compaction_or_cut() {
    // 1. Detached brief turn settling with compacted SessionSeen + Stopped clears the live planner's held sections.
    let (mut s, detached) = detached_planner_turn();
    s.apply(Event::SessionSeen {
        id: detached.clone(),
        session: SessionSeen { compacted: true, ..SessionSeen::default() },
    });
    s.apply(Event::JobEnded { id: detached, end: JobEnd::Stopped });
    assert!(s.state.agents[PLANNER].held.is_empty(), "compacted detached turn must clear held sections");

    s.andrei(json!({"say": {"place": format!("{CARD}/spec"), "text": "Spec question."}})).unwrap();
    let next = turns::plan(&s.state, &AgentKey::new(PLANNER), s.ctx(), false).unwrap();
    assert_has_all(&next.prompt, &["## Delegation", "## Allowed models", "## The reply"]);

    // 2. Detached brief turn settling as Cut clears the live planner's held sections.
    let (mut s, detached) = detached_planner_turn();
    s.apply(Event::JobEnded { id: detached, end: JobEnd::Cut { session: SessionSeen::default() } });
    assert!(s.state.agents[PLANNER].held.is_empty(), "cut detached turn must clear held sections");
}
