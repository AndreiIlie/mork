// fac-651: returning reviewers are told their own earlier verdicts and findings, and the review
// foreman's later-round request lists kept reviewers with their topics and last reviewed rounds.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply, sha};
use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_types::card::Mode;
use mork_types::review::Verdict;
use mork_types::state::{Retiring, Sender};
use mork_types::{ActionId, AgentKey, CardId};
use serde_json::{Value, json};

fn finding(file: &str, line: i64, summary: &str) -> Value {
    json!({"file": file, "line": line, "summary": summary, "proof": "proof"})
}

fn verdict_reply(outcome: &str, headline: &str, findings: Vec<Value>) -> Value {
    let mut r = reply(outcome, headline);
    r["findings"] = json!(findings);
    r
}

fn start_round_one(sim: &mut Sim, card: &str, head: &str) -> String {
    sim.through_brief(card, "web");
    sim.built(card, head, "base1");
    sim.checks(card, 0, &[]);
    sim.candidate(card);
    sim.round_asked(card);
    sim.review_card(card)
}

fn spawn_cmd(rc: &str, topic: &str, task: &str) -> Value {
    json!({"spawn": {"card": rc, "role": "reviewer", "topic": topic, "task": task}})
}

fn say_cmd(place: &str, to: &str, text: &str) -> Value {
    json!({"say": {"place": place, "to": to, "text": text, "kind": "request"}})
}

fn run_turn(sim: &mut Sim, key: &str, commands: &[Value], reply: Value) -> String {
    let k = AgentKey::new(key);
    let (id, prompt) = sim.turn_started_with_argv(key, &["x"]);
    for c in commands {
        let answer = sim.cmd(Sender::Agent(k.clone()), c.clone());
        assert!(answer.is_ok(), "{key}'s {c} was refused: {answer:?}");
    }
    let session = SessionSeen { started: true, ..SessionSeen::default() };
    sim.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Ok(reply), session } });
    prompt
}

fn resubmit_after_triage(sim: &mut Sim, card: &str, head: &str, fix: &[&str], decline: &[&str]) {
    sim.proofs_copied();
    sim.suite(card, 0);
    sim.operator(json!({"triage": {"card": card, "fix": fix, "decline": decline, "foreman": []}})).unwrap();
    sim.settle_actions();
    sim.imported(card);
    sim.built(card, head, "base1");
    sim.checks(card, 0, &[]);
    sim.candidate(card);
    sim.round_asked(card);
}

fn run_round_one_with_three_reviewers(sim: &mut Sim, card: &str) -> String {
    let rc = start_round_one(sim, card, "h1");
    let foreman = format!("{rc}/foreman");
    sim.turn_sending(
        &foreman,
        &[
            spawn_cmd(&rc, "correctness and tests", "Check correctness"),
            spawn_cmd(&rc, "tests", "Check tests"),
            spawn_cmd(&rc, "design and simplicity", "Check design"),
        ],
        reply("working", "Spawned reviewers"),
    );
    let p1 = run_turn(
        sim,
        &format!("{rc}/reviewer-1"),
        &[],
        verdict_reply("review-fail", "Shared bug", vec![finding("src/a.rs", 10, "shared bug in a")]),
    );
    assert!(!p1.contains("## Your earlier rounds"), "first-pass reviewer saw earlier rounds: {p1}");

    // reviewer-2 replies twice in round 1: its latest verdict in that round must win.
    let r2 = format!("{rc}/reviewer-2");
    run_turn(sim, &r2, &[], verdict_reply("review-fail", "First", vec![finding("src/b.rs", 20, "stale line")]));
    run_turn(
        sim,
        &format!("{rc}/reviewer-3"),
        &[],
        verdict_reply(
            "review-fail",
            "Two findings",
            vec![
                finding("src/a.rs", 10, "duplicate of shared bug"),
                finding("src/c.rs", 5, "style nit dropped by foreman"),
            ],
        ),
    );
    sim.turn_sending(&foreman, &[say_cmd(&rc, "reviewer-2", "Recheck")], reply("working", "Asked again"));
    run_turn(
        sim,
        &r2,
        &[],
        verdict_reply(
            "review-fail",
            "Updated lines",
            vec![finding("src/b.rs", 22, "missing null check"), finding("src/b.rs", 44, "unused helper")],
        ),
    );
    sim.round_ends(
        card,
        &[],
        &[
            finding("src/a.rs", 10, "[reviewer-1] Shared bug in a"),
            finding("src/b.rs", 22, "[reviewer-2] Reworded null check issue"),
        ],
    );
    resubmit_after_triage(sim, card, "h2", &["2"], &["1"]);
    rc
}

fn assert_contains_all(haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(haystack.contains(needle), "missing `{needle}` in:\n{haystack}");
    }
}

fn assert_round_two_prompts(sim: &mut Sim, rc: &str) {
    let p1 = run_turn(sim, &format!("{rc}/reviewer-1"), &[], verdict_reply("review-pass", "Clean", vec![]));
    assert_contains_all(
        &p1,
        &[
            "## Your earlier rounds",
            "Round 1 (commit `",
            "failed, 1 finding(s) reported",
            "1. src/a.rs:10: shared bug in a",
            "- #1 (declined) src/a.rs:10: Shared bug in a",
        ],
    );

    let r2 = format!("{rc}/reviewer-2");
    let p2 = run_turn(
        sim,
        &r2,
        &[],
        verdict_reply("review-fail", "Still broken", vec![finding("src/b.rs", 22, "#2 still broken")]),
    );
    assert_contains_all(
        &p2,
        &[
            "failed, 2 finding(s) reported",
            "1. src/b.rs:22: missing null check",
            "2. src/b.rs:44: unused helper",
            "- #2 (open) src/b.rs:22: Reworded null check issue",
        ],
    );
    assert!(!p2.contains("stale line") && !p2.contains("#1"), "unexpected text in r2 prompt: {p2}");

    let p3 = run_turn(sim, &format!("{rc}/reviewer-3"), &[], verdict_reply("review-pass", "Clean", vec![]));
    assert_contains_all(
        &p3,
        &[
            "failed, 2 finding(s) reported",
            "1. src/a.rs:10: duplicate of shared bug",
            "2. src/c.rs:5: style nit dropped by foreman",
            "Kept under your name in round 1's verdict: none.",
        ],
    );
    assert!(!p3.contains("reviewer-1") && !p3.contains("#1"), "blindness violated: {p3}");
}

#[test]
fn returning_reviewer_sees_verdict_reported_and_kept_findings_and_foreman_sees_kept_reviewers() {
    let mut sim = Sim::new();
    let rc = run_round_one_with_three_reviewers(&mut sim, "web-1");
    let foreman = format!("{rc}/foreman");
    let (h1, h2, b1) = (sha("h1"), sha("h2"), sha("base1"));
    let round2_req = sim.engine_to(&rc, "foreman").pop().unwrap().text;
    let r1_line = format!(
        "reviewer-1 (correctness and tests): last reviewed round 1 (commit `{}`, base `{}`): failed, 1 finding(s) reported",
        h1.short(),
        b1.short()
    );
    let r2_line = format!(
        "reviewer-2 (tests): last reviewed round 1 (commit `{}`, base `{}`): failed, 2 finding(s) reported",
        h1.short(),
        b1.short()
    );
    let r3_line = format!(
        "reviewer-3 (design and simplicity): last reviewed round 1 (commit `{}`, base `{}`): failed, 2 finding(s) reported",
        h1.short(),
        b1.short()
    );
    assert_contains_all(&round2_req, &["Kept reviewers from earlier rounds", &r1_line, &r2_line, &r3_line]);

    sim.turn_sending(
        &foreman,
        &[
            say_cmd(&rc, "reviewer-1", "Check round 2"),
            say_cmd(&rc, "reviewer-2", "Verify #2 and check round 2"),
            say_cmd(&rc, "reviewer-3", "Check round 2"),
        ],
        reply("working", "Tasked returning reviewers"),
    );
    assert_round_two_prompts(&mut sim, &rc);

    // Re-raising #2 in round 2 shows passed verdicts in round 3's foreman request and survives `fresh`.
    let r2 = format!("{rc}/reviewer-2");
    sim.round_ends("web-1", &[], &[finding("src/b.rs", 22, "[reviewer-2] #2 Still broken in round 2")]);
    resubmit_after_triage(&mut sim, "web-1", "h3", &["2"], &[]);
    let round3_req = sim.engine_to(&rc, "foreman").pop().unwrap().text;
    let r1_passed = format!(
        "reviewer-1 (correctness and tests): last reviewed round 2 (commit `{}`, base `{}`): passed",
        h2.short(),
        b1.short()
    );
    assert!(round3_req.contains(&r1_passed), "missing passed verdict line `{r1_passed}` in:\n{round3_req}");

    sim.operator(json!({"fresh": {"agent": r2}})).unwrap();
    sim.turn_sending(&foreman, &[say_cmd(&rc, "reviewer-2", "Verify #2 again")], reply("working", "Tasked"));
    let p2_r3 = run_turn(
        &mut sim,
        &r2,
        &[json!({"resolve": {"card": rc, "n": 2}})],
        verdict_reply("review-pass", "Fixed", vec![]),
    );
    assert!(p2_r3.contains("- #2 (from round 1, open) src/b.rs:22: Still broken in round 2"), "{p2_r3}");
}

fn setup_withdrawn_round(sim: &mut Sim, card: &str) -> (String, mork_types::state::Agent) {
    let rc = start_round_one(sim, card, "h1");
    let foreman = format!("{rc}/foreman");
    let r1 = format!("{rc}/reviewer-1");
    sim.turn_sending(&foreman, &[spawn_cmd(&rc, "correctness and tests", "Check")], reply("working", "Spawned"));
    run_turn(
        sim,
        &r1,
        &[],
        verdict_reply(
            "review-fail",
            "Four issues",
            vec![
                finding("src/a.rs", 1, "first"),
                finding("src/a.rs", 2, "second"),
                finding("src/a.rs", 3, "third"),
                finding("src/a.rs", 4, "fourth"),
            ],
        ),
    );
    sim.round_ends(
        card,
        &[],
        &[
            finding("src/a.rs", 1, "[reviewer-1] First"),
            finding("src/a.rs", 2, "[reviewer-1] Second"),
            finding("src/a.rs", 3, "[reviewer-1] Third"),
            finding("src/a.rs", 4, "[reviewer-1] Fourth"),
        ],
    );
    let saved_r1 = sim.state.agents.get(&AgentKey::new(&r1)).unwrap().clone();
    sim.suite(card, 0);
    sim.operator(json!({"brief": {"card": card, "words": "Replan"}})).unwrap();
    sim.settle_actions();
    (rc, saved_r1)
}

fn seed_all_statuses_and_superseded(sim: &mut Sim, rc: &str) {
    let mut card = sim.card(rc);
    assert_eq!(card.mode, Mode::Review);
    let rev = card.review.as_deref_mut().unwrap();
    rev.rounds[0].head = Some(sha("pr-head-1"));
    rev.raised[0].withdrawn = false;
    rev.raised[1].withdrawn = false;
    rev.raised[1].dropped = Some(ActionId::new("act-drop"));
    rev.raised[2].withdrawn = false;
    rev.raised[2].resolved = false;
    rev.raised[2].answer = Some(ActionId::new("act-ans"));
    let mut r2 = rev.rounds[0].clone();
    r2.n = 2;
    r2.commit = sha("h-sup");
    r2.head = Some(sha("pr-head-2"));
    r2.verdict = Some(Verdict::Superseded);
    r2.named.clear();
    rev.rounds.push(r2);
    sim.state.cards.insert(CardId::new(rc), card);
}

fn replan_and_ask_next_round(sim: &mut Sim, card: &str, head: &str) {
    let mut recap = reply("brief-ready", "New plan");
    recap["hand"]["message"] = json!("- where: src\n---\nNew plan.");
    sim.turn(&format!("{card}/planner"), recap);
    sim.turn(&format!("{card}/foreman"), reply("working", "No objection"));
    sim.operator(json!({"approve": {"card": card}})).unwrap();
    sim.settle_actions();
    sim.built(card, head, "base1");
    sim.checks(card, 0, &[]);
    sim.candidate(card);
    sim.round_asked(card);
}

#[test]
fn finding_statuses_withdrawn_dropped_answered_superseded_afresh_and_resumed_closing_reviewer() {
    let mut sim = Sim::new();
    let (rc, saved_r1) = setup_withdrawn_round(&mut sim, "web-2");
    let r1 = format!("{rc}/reviewer-1");

    let card = sim.card(&rc);
    let rev = card.review.as_ref().unwrap();
    assert!(rev.raised.iter().all(|f| f.resolved && f.withdrawn));
    assert_eq!(rev.afresh, Some(2));
    assert!(!sim.state.agents.contains_key(&AgentKey::new(&r1)));

    seed_all_statuses_and_superseded(&mut sim, &rc);
    sim.state.agents.insert(saved_r1.key.clone(), saved_r1);

    let dm = format!("{r1}/dm");
    sim.operator(json!({"say": {"place": dm, "to": "reviewer-1", "text": "Status check", "kind": "request"}})).unwrap();
    let (job_id, prompt) = sim.turn_started_with_argv(&r1, &["x"]);
    assert_contains_all(
        &prompt,
        &[
            "head `",
            "under an earlier plan",
            "- #1 (resolved) src/a.rs:1: First",
            "- #2 (dropped) src/a.rs:2: Second",
            "- #3 (open — answered) src/a.rs:3: Third",
            "- #4 (withdrawn) src/a.rs:4: Fourth",
            "Round 2",
            "superseded — no foreman verdict",
        ],
    );
    assert!(!prompt.contains("Kept under your name in round 2's verdict"), "{prompt}");

    // A reviewer in a resumable closing turn (!remove && !live) stays in kept_reviewers,
    // and its latest reviewed round is round 2 (superseded) with PR head rather than older round 1.
    sim.apply(Event::JobEnded {
        id: job_id,
        end: JobEnd::Finished { reply: Ok(reply("review-pass", "Done")), session: SessionSeen::default() },
    });
    let mut a = sim.state.agents.get(&AgentKey::new(&r1)).unwrap().clone();
    a.retiring = Some(Retiring { remove: false, asked: true, resumed: false, live: false });
    sim.state.agents.insert(a.key.clone(), a);

    replan_and_ask_next_round(&mut sim, "web-2", "h2");
    let next_req = sim.engine_to(&rc, "foreman").pop().unwrap().text;
    let expected = format!(
        "reviewer-1 (correctness and tests): last reviewed round 2 (commit `{}`, base `{}`, head `{}`, superseded): failed, 4 finding(s) reported",
        sha("h-sup").short(),
        sha("base1").short(),
        sha("pr-head-2").short()
    );
    assert!(next_req.contains(&expected), "expected `{expected}` in:\n{next_req}");
}

#[test]
fn released_reviewer_marked_for_removal_is_omitted_from_kept_reviewers() {
    let mut sim = Sim::new();
    let (rc, saved_r1) = setup_withdrawn_round(&mut sim, "web-3");
    let mut removed = saved_r1;
    removed.retiring = Some(Retiring { remove: true, asked: false, resumed: false, live: false });
    sim.state.agents.insert(removed.key.clone(), removed);

    replan_and_ask_next_round(&mut sim, "web-3", "h2");
    let next_req = sim.engine_to(&rc, "foreman").pop().unwrap().text;
    assert!(!next_req.contains("Kept reviewers from earlier rounds"), "{next_req}");
}

#[test]
fn compacted_reviewer_correction_restores_the_subject_and_carried_decisions() {
    let mut sim = Sim::new();
    let card = "web-1";
    let rc = start_round_one(&mut sim, card, "h1");
    let key = format!("{rc}/reviewer-1");
    sim.turn_sending(&format!("{rc}/foreman"), &[spawn_cmd(&rc, "tests", "Review tests")], reply("working", "Review"));
    let (id, _) = sim.turn_started_with_argv(&key, &["claude"]);
    sim.apply(Event::JobEnded {
        id,
        end: JobEnd::Finished {
            reply: Ok(Value::Null),
            session: SessionSeen { started: true, compacted: true, ..SessionSeen::default() },
        },
    });
    sim.state.board.get_mut(card).unwrap().description =
        "operator 01.01: Subject discussion.\noperator 01.01: Decision: Subject rule.\n\nSubject words.".into();
    sim.state.cards.get_mut(card).unwrap().carries.push(mork_types::card::Carried {
        id: "web-2".into(),
        title: "Carried title".into(),
        description: "operator 01.01: Carried discussion.\noperator 01.01: Decision: Carried rule.\n\nCarried words."
            .into(),
        ..Default::default()
    });
    let (job, prompt) = sim.turn_started_with_argv(&key, &["claude"]);
    let recorded = &sim.state.jobs[&job];
    let prefix = &prompt[..recorded.specification_bytes];
    for words in ["## Under review", "Subject words.", "Subject rule.", "Carried words.", "Carried rule."] {
        assert!(prefix.contains(words), "{prefix}");
    }
    assert!(!prefix.contains("discussion."));
    assert!(recorded.request_text().starts_with("Your reply was refused"));
}
