// fac-670: closing turns keep the session's effort, skip cold seats, run for warm planners at brief approval, and
// expose cache expiry to views.

// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply};
use mork_core::agents::{cache, closing, models, turns};
use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_core::views::{outside::Outside, place::place, screen::screen};
use mork_types::card::Stage;
use mork_types::config::{Cli, Effort};
use mork_types::state::{Agent, Owed};
use mork_types::view::CacheState;
use mork_types::work::Purpose;
use mork_types::{AgentKey, JobId};
use serde_json::{Value, json};

const CARD: &str = "web-1";
const PLANNER: &str = "web-1/planner";
const FOREMAN: &str = "web-1/foreman";

fn agent(s: &Sim, key: &str) -> Agent {
    s.state.agents.get(&AgentKey::new(key)).cloned().unwrap_or_else(|| panic!("no agent {key}"))
}

fn finish_with_tools(s: &mut Sim, id: &JobId, reply: Value) {
    let session = SessionSeen { started: true, tools: true, answered: true, ..SessionSeen::default() };
    s.apply(Event::JobEnded { id: id.clone(), end: JobEnd::Finished { reply: Ok(reply), session } });
}

fn run_with_tools(s: &mut Sim, key: &str, reply: Value) {
    let id = s.turn_started(key);
    finish_with_tools(s, &id, reply);
}

fn recap_reply() -> Value {
    let mut r = reply("brief-ready", "The plan");
    r["hand"]["message"] = json!("- where: src\n---\nThe plan.");
    r
}

fn closing_reply() -> Value {
    json!({"feedback": "nothing", "mork_feedback": "nothing"})
}

// Starts a full card, clones it, and runs three planner turns with tools ending in a challenged recap ready to approve.
fn ready_to_approve() -> Sim {
    let mut s = Sim::new();
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let id = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&id, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Brief);

    run_with_tools(&mut s, PLANNER, reply("working", "Scout 1"));
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Continue."}})).unwrap();
    run_with_tools(&mut s, PLANNER, reply("working", "Scout 2"));
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Recap now."}})).unwrap();
    run_with_tools(&mut s, PLANNER, recap_reply());
    s.turn(FOREMAN, reply("working", "No objection"));
    s
}

#[test]
fn closing_turn_keeps_session_effort_and_ignores_pending_pick() {
    let mut s = ready_to_approve();
    s.operator(json!({"pick": {"agent": PLANNER, "effort": "low"}})).unwrap();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    let planner = agent(&s, PLANNER);
    assert!(closing::ending_turn(&planner));
    let seat = models::seat(&s.config, &planner).unwrap();
    assert_eq!(seat.cli, Cli::Claude);
    assert_eq!(seat.effort, Effort::Xhigh, "closing turn keeps the session's launched effort");
}

#[test]
fn warm_planner_closes_at_approval_and_keeps_held_request() {
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    assert_eq!(s.stage(CARD), Stage::Build);
    assert!(s.owed(PLANNER), "warm planner is due for its closing turn while the card is at build");
    assert!(closing::ending_turn(&agent(&s, PLANNER)));

    let id = s.turn_started(PLANNER);
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Spec question."}})).unwrap();
    finish_with_tools(&mut s, &id, closing_reply());

    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none(), "planner stays idle after its closing turn settles");
    assert_eq!(planner.ran.closed, 3);
    assert_eq!(planner.owed, Some(Owed::Wake), "held request wakes planner after closing turn");
}

#[test]
fn stopped_running_brief_turn_at_approval_is_stamped_and_still_closes() {
    let mut s = ready_to_approve();
    s.now += cache::CLAUDE_MS + 5 * 60_000;
    // Start a running turn on the planner while the recap is still offered, then approve.
    let key = AgentKey::new(PLANNER);
    let mut a = agent(&s, PLANNER);
    a.owed = Some(Owed::Wake);
    s.state.agents.insert(key, a);
    let detached = s.turn_started(PLANNER);
    // Keep the recap offered so approval stops this running turn.
    s.operator(json!({"approve": {"card": CARD}})).unwrap();

    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_some(), "running brief turn counts as warm at approval");
    assert!(planner.last_turn_ms.is_some_and(|t| t >= s.now - 1000), "detached stopped turn stamped last_turn_ms");
    assert!(!s.owed(PLANNER), "closing turn waits until detached job exits");

    s.apply(Event::JobEnded { id: detached, end: JobEnd::Stopped });
    assert!(s.owed(PLANNER), "planner stays warm and due once the detached job settles");
}

#[test]
fn cold_seat_at_card_end_skips_closing_turn() {
    let mut s = ready_to_approve();
    s.now += cache::CLAUDE_MS + 1000;
    s.operator(json!({"close": {"card": CARD, "why": "Done."}})).unwrap();
    assert!(!s.state.agents.contains_key(&AgentKey::new(PLANNER)), "cold removed seat goes immediately");

    // Report card ended (`done`, remove = false): cold seat stays without retiring.
    let mut s = Sim::new();
    s.board("web-2", "web");
    s.operator(json!({"start": {"card": "web-2", "mode": "report"}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == "web-2"))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    for _ in 0..3 {
        run_with_tools(&mut s, "web-2/foreman", reply("working", "Step"));
        s.operator(json!({"say": {"place": "web-2", "text": "More."}})).unwrap();
    }
    s.now += cache::CODEX_MS + 1000;
    s.operator(json!({"done": {"card": "web-2"}})).unwrap();
    let f = agent(&s, "web-2/foreman");
    assert!(f.retiring.is_none(), "cold kept seat skips closing turn");
}

#[test]
fn owed_closing_turn_and_correction_expire_when_cold() {
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Hold this."}})).unwrap();
    assert!(s.owed(PLANNER));

    s.now += cache::CLAUDE_MS + 1000;
    assert!(!s.owed(PLANNER), "cold closing turn is never returned as due");
    s.apply(Event::Tick);
    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none(), "tick settles cold retiring seat");
    assert_eq!(planner.owed, Some(Owed::Wake), "held request becomes owed once cold closing turn expires");

    // Queued correction of a refused closing reply (`asked == true`) also expires on tick when cold.
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    let id = s.turn_started(PLANNER);
    finish_with_tools(&mut s, &id, json!({"feedback": ""}));
    assert!(matches!(agent(&s, PLANNER).owed, Some(Owed::Correction { .. })));
    assert!(agent(&s, PLANNER).retiring.as_ref().is_some_and(|r| r.asked));
    assert!(s.owed(PLANNER));

    s.now += cache::CLAUDE_MS + 1000;
    assert!(!turns::due(&s.state, &s.config, s.now).iter().any(|k| k.as_str() == PLANNER));
    s.apply(Event::Tick);
    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none(), "cold closing correction is expired by tick");
    assert!(planner.owed.is_none());
}

#[test]
fn planner_asked_again_at_card_end_only_if_it_ran_since_approval() {
    // No turns since approval closing turn: skipped at close even while warm.
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    run_with_tools(&mut s, PLANNER, closing_reply());
    assert_eq!(agent(&s, PLANNER).ran.closed, 3);
    s.operator(json!({"close": {"card": CARD, "why": "Stop."}})).unwrap();
    assert!(!s.state.agents.contains_key(&AgentKey::new(PLANNER)), "no second closing turn without a turn since");

    // One turn since approval closing turn while warm: asked again at close.
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    run_with_tools(&mut s, PLANNER, closing_reply());
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Clarify."}})).unwrap();
    s.turn(PLANNER, reply("working", "Clarified"));
    assert_eq!(agent(&s, PLANNER).ran.turns, 4);
    s.operator(json!({"close": {"card": CARD, "why": "Stop."}})).unwrap();
    assert!(agent(&s, PLANNER).retiring.is_some(), "warm planner with a turn since gets second closing turn");
}

#[test]
fn last_turn_ms_stamps_only_model_turns_clears_on_fresh_and_round_trips() {
    let mut s = Sim::new();
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();

    // First turn fails before starting a session: last_turn_ms remains None.
    let id = s.turn_started(PLANNER);
    s.apply(Event::JobEnded { id, end: JobEnd::Failed { words: "boom".into() } });
    assert_eq!(agent(&s, PLANNER).last_turn_ms, None);

    // Turn reaches the model: last_turn_ms is stamped and survives serde round trip.
    s.operator(json!({"resume": {"agent": PLANNER}})).unwrap();
    run_with_tools(&mut s, PLANNER, reply("working", "Ran"));
    let stamped = agent(&s, PLANNER).last_turn_ms.expect("stamped on completed turn");
    let raw = serde_json::to_string(&agent(&s, PLANNER)).unwrap();
    let back: Agent = serde_json::from_str(&raw).unwrap();
    assert_eq!(back.last_turn_ms, Some(stamped));

    // Cold seat fails immediately without answering or tools: last_turn_ms does not advance.
    s.now += cache::CLAUDE_MS + 1000;
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Wake."}})).unwrap();
    let id = s.turn_started(PLANNER);
    let failed = JobEnd::Finished { reply: Err("limit".into()), session: SessionSeen::default() };
    s.apply(Event::JobEnded { id, end: failed });
    assert_eq!(agent(&s, PLANNER).last_turn_ms, Some(stamped), "immediate failure does not warm a cold seat");

    // `fresh` clears last_turn_ms.
    s.operator(json!({"fresh": {"agent": PLANNER}})).unwrap();
    assert_eq!(agent(&s, PLANNER).last_turn_ms, None);
}

#[test]
fn views_report_cache_state_and_waiting_hand_cold_at() {
    let mut s = Sim::new();
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();

    let outside = Outside::default();
    let pv = place(&s.state, &format!("{CARD}/spec"), None, &outside, s.ctx());
    let row = pv.sessions.iter().find(|r| r.agent.as_str() == PLANNER).unwrap();
    assert_eq!(row.cache, None, "no cache status before a first turn");

    // Running turn shows CacheState::Running.
    let id = s.turn_started(PLANNER);
    let pv = place(&s.state, &format!("{CARD}/spec"), None, &outside, s.ctx());
    let row = pv.sessions.iter().find(|r| r.agent.as_str() == PLANNER).unwrap();
    assert_eq!(row.cache, Some(CacheState::Running));

    // Idle with an open ask shows Idle { cold_at_ms } in Context and cold_at_ms on HandView.
    let mut ask = reply("working", "Question");
    ask["hand"]["ask"] = json!("Proceed?");
    finish_with_tools(&mut s, &id, ask);
    let expected = agent(&s, PLANNER).last_turn_ms.unwrap() + cache::CLAUDE_MS;

    let pv = place(&s.state, &format!("{CARD}/spec"), None, &outside, s.ctx());
    let row = pv.sessions.iter().find(|r| r.agent.as_str() == PLANNER).unwrap();
    assert_eq!(row.cache, Some(CacheState::Idle { cold_at_ms: expected }));

    let sv = screen(&s.state, &outside, s.ctx());
    let hand = sv.hands.iter().find(|h| h.place.as_str() == format!("{CARD}/spec")).unwrap();
    assert_eq!(hand.cold_at_ms, Some(expected), "waiting seat's hand carries cold_at_ms");
}

#[test]
fn pending_fresh_applies_after_approval_closing_turn_and_detached_brief_turn() {
    // 1. `fresh` sent while the approval closing turn runs takes effect on settle, before the held wake.
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    let id = s.turn_started(PLANNER);
    s.operator(json!({"fresh": {"agent": PLANNER}})).unwrap();
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Follow-up."}})).unwrap();
    finish_with_tools(&mut s, &id, closing_reply());

    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none());
    assert!(planner.session.is_none() && !planner.session_started, "pending fresh replaced the session at settle");
    assert_eq!(planner.last_turn_ms, None);
    assert_eq!(planner.ran.closed, 0);
    assert_eq!(planner.owed, Some(Owed::Wake));

    // 2. `fresh` pending over a brief turn detached by approval (with no closing turn) applies immediately.
    let mut s = Sim::new();
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    run_with_tools(&mut s, PLANNER, recap_reply());
    s.turn(FOREMAN, reply("working", "No objection"));
    let mut a = agent(&s, PLANNER);
    a.owed = Some(Owed::Wake);
    s.state.agents.insert(AgentKey::new(PLANNER), a);
    let _detached = s.turn_started(PLANNER);
    s.operator(json!({"fresh": {"agent": PLANNER}})).unwrap();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();

    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none());
    assert!(planner.session.is_none() && !planner.session_started, "pending fresh over detached brief turn applied");
    assert!(planner.fresh.as_ref().is_some_and(|f| !f.pending));
}

#[test]
fn cold_limit_hit_in_closing_turn_keeps_not_before_for_held_wake() {
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    let id = s.turn_started(PLANNER);
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "After reset."}})).unwrap();
    // Turn ends past cold_at with a usage limit and no model answer/tools.
    s.now += cache::CLAUDE_MS + 1000;
    let reset_sec = (s.now + 30 * 60_000) / 1000;
    let session = SessionSeen { limit_reached: true, limit_reset: Some(reset_sec), ..SessionSeen::default() };
    s.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Err("rate limit".into()), session } });

    let expected_until = reset_sec * 1000;
    assert!(!s.owed(PLANNER), "cold closing turn waiting on limit is not due");
    assert_eq!(agent(&s, PLANNER).not_before, Some(expected_until));

    s.apply(Event::Tick);
    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none(), "tick expires the cold closing turn");
    assert_eq!(planner.owed, Some(Owed::Wake), "held request stays owed");
    assert_eq!(planner.not_before, Some(expected_until), "limit reset is preserved for the held wake");
}

#[test]
fn stopping_and_resuming_approval_closing_turn_does_not_replay_feedback_prompt() {
    // 1. Stop during approval closing turn, let it settle, then resume.
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    let id = s.turn_started(PLANNER);
    s.operator(json!({"stop": {"agent": PLANNER}})).unwrap();
    s.apply(Event::JobEnded { id, end: JobEnd::Stopped });
    assert!(agent(&s, PLANNER).retiring.is_none());
    assert!(agent(&s, PLANNER).paused);
    s.operator(json!({"resume": {"agent": PLANNER}})).unwrap();
    let planner = agent(&s, PLANNER);
    assert!(!planner.paused);
    assert_eq!(planner.owed, None, "resume does not replay the stopped closing turn as an ordinary turn");

    // 2. Stop and immediately resume while the approval closing turn is still stopping.
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    let id = s.turn_started(PLANNER);
    s.operator(json!({"stop": {"agent": PLANNER}})).unwrap();
    s.operator(json!({"resume": {"agent": PLANNER}})).unwrap();
    s.apply(Event::JobEnded { id, end: JobEnd::Stopped });
    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none());
    assert!(!planner.paused);
    assert_eq!(planner.owed, None, "resume while stopping does not queue a closing-prompt retry");

    // 3. Stop during approval closing turn with a held spec request keeps the wake for resume.
    let mut s = ready_to_approve();
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    let id = s.turn_started(PLANNER);
    s.operator(json!({"say": {"place": format!("{CARD}/spec"), "text": "Spec question during closing."}})).unwrap();
    s.operator(json!({"stop": {"agent": PLANNER}})).unwrap();
    s.apply(Event::JobEnded { id, end: JobEnd::Stopped });
    let planner = agent(&s, PLANNER);
    assert!(planner.retiring.is_none());
    assert!(planner.paused, "stopped seat stays paused until resumed");
    assert_eq!(planner.owed, Some(Owed::Wake), "held spec request stays owed after stopping closing turn");
    assert!(!s.owed(PLANNER), "paused seat does not run until resumed");

    s.operator(json!({"resume": {"agent": PLANNER}})).unwrap();
    assert!(!agent(&s, PLANNER).paused);
    assert!(s.owed(PLANNER), "resuming delivers the held spec request");
}
