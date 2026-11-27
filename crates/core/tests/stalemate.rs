// Pure state-machine tests for delegated review-round stalemate detection and escalation (fac-694).

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply};
use mork_core::decide::{Decision, Intent};
use mork_core::state::Change;
use mork_core::views::outside::Outside;
use mork_core::views::place::place;
use mork_core::views::screen::screen;
use mork_types::ask::{Ask, AskKind, Color};
use mork_types::card::Stage;
use mork_types::orchestrator::{OrchestratorEvent, OrchestratorWhat};
use mork_types::state::{MessageKind, Sender};
use mork_types::work::Purpose;
use serde_json::{Value, json};

const CARD: &str = "web-1";

fn f(file: &str, line: u32, summary: &str, proof: &str) -> Value {
    json!({"file": file, "line": line, "summary": summary, "proof": proof})
}

fn start_delegated_build(s: &mut Sim, extra: Value) {
    s.config.mork.notifications = true;
    s.board(CARD, "web");
    let mut start = json!({"card": CARD});
    if let (Some(dst), Some(src)) = (start.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    s.operator(json!({"start": start})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("- where: src\n---\nThe plan.");
    s.turn(&format!("{CARD}/planner"), recap);
    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    s.cmd(Sender::Orchestrator("web".into()), json!({"approve": {"card": CARD}})).unwrap();
    s.settle_actions();
}

fn run_failed_round(s: &mut Sim, head: &str, findings: &[Value], suite_exit: i32) -> Decision {
    s.built(CARD, head, "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    s.round_ends(CARD, &[], findings);
    s.proofs_copied();
    s.suite(CARD, suite_exit)
}

fn given_events(d: &Decision) -> Vec<&OrchestratorEvent> {
    d.changes
        .iter()
        .filter_map(|c| match c {
            Change::AddOrchestratorEvent(e) if matches!(e.what, OrchestratorWhat::Given(_)) => Some(e),
            _ => None,
        })
        .collect()
}

fn notifies(d: &Decision, ask: &str) -> bool {
    d.intents.iter().any(|i| matches!(i, Intent::Notify { ask: id } if id.as_str() == ask))
}

fn open_triage(s: &Sim) -> &Ask {
    s.state.asks.values().find(|a| a.open && a.kind == AskKind::Triage).unwrap()
}

fn assert_stalemate_views(s: &Sim, ask: &Ask) {
    let sc = screen(&s.state, &Outside::default(), s.ctx());
    let hand = sc.hands.iter().find(|h| h.ask == ask.id).unwrap();
    assert_eq!(hand.color, Color::Red);
    let pv = place(&s.state, CARD, None, &Outside::default(), s.ctx());
    assert!(pv.triage.as_ref().unwrap().stalemate);
    assert_eq!(pv.card.as_ref().unwrap().pipeline[2].sub.as_deref(), Some("stalemate · 1 finding waits for you"));
    let ov = pv.overview.as_ref().unwrap();
    assert_eq!(ov.stages.last().unwrap().summary, "stalemate · 1 to triage");
    assert!(ov.live.iter().any(|r| r.name == "review foreman" && r.words == "stalemate · 1 to triage"));
}

fn assert_stalemate_journal(s: &Sim) {
    let row = |kind: &str| &s.journal.iter().rev().find(|(_, k, _, _)| k == kind).unwrap().3;
    assert_eq!(row("review.stalemate")["findings"], json!([1]));
    assert_eq!(row("review.verdict")["named"], json!([{"n":1,"round":1,"by":"foreman"}]));
    assert_eq!(row("review.asked")["submitted"], row("build.submitted")["head"]);
    assert_eq!(row("build.submitted")["head"], json!(common::sha("h2")));
    assert!(s.journal.iter().any(|(_, k, _, v)| k == "cost" && v["job"] == row("build.requested")["job"]));
}

#[test]
fn delegated_and_self_sustained_escalate_on_repeated_unresolved_findings() {
    let bug = f("src/a.rs", 3, "Missing null guard", "proof 1");
    for extra in [json!({"delegated": true}), json!({"self-sustained": true})] {
        let mut s = Sim::new();
        start_delegated_build(&mut s, extra.clone());

        run_failed_round(&mut s, "h1", std::slice::from_ref(&bug), 0);
        assert!(s.card(CARD).triage.is_none(), "{extra}: round 1 must dispatch directly");
        s.imported(CARD);

        let reqs_before = s.engine_to(CARD, "foreman").len();
        let d = run_failed_round(&mut s, "h2", &[f("src/a.rs", 5, "#1 Missing null guard", "proof 1b")], 0);
        let pending = s.card(CARD).triage.expect("round 2 repeat must open stalemate triage");
        assert!(pending.stalemate && pending.round == 2);
        assert_stalemate_journal(&s);
        let ask = open_triage(&s).clone();
        assert_eq!(ask.color, Color::Red);
        assert!(!ask.given && ask.notice && notifies(&d, ask.id.as_str()));
        assert!(given_events(&d).is_empty());
        assert!(ask.text.contains("stalled") && ask.text.contains("round 1"), "{}", ask.text);
        assert!(!s.owed(&format!("{CARD}/foreman")));
        assert_eq!(s.engine_to(CARD, "foreman").len(), reqs_before);

        assert_stalemate_views(&s, &ask);

        // Re-running delegate / switching to self-sustained / off->on must keep the stalemate open on the operator.
        s.operator(json!({"delegate": {"card": CARD, "self-sustained": true}})).unwrap();
        assert!(s.state.asks[&ask.id].open && s.card(CARD).triage.is_some());
        s.operator(json!({"delegate": {"card": CARD, "off": true}})).unwrap();
        s.operator(json!({"delegate": {"card": CARD}})).unwrap();
        assert!(s.state.asks[&ask.id].open && s.card(CARD).triage.is_some());

        // The operator declines the finding -> verification restarts without waking foreman.
        s.operator(json!({"triage": {"card": CARD, "decline": ["1"]}})).unwrap();
        assert!(s.card(CARD).triage.is_none());
        assert_eq!(s.engine_to(CARD, "foreman").len(), reqs_before);
        assert_eq!(s.stage(CARD), Stage::Merging);
    }
}

#[test]
fn zero_progress_with_new_finding_stalls_while_partial_progress_continues() {
    // Zero progress + new finding (#1 -> #1, #2) still fires stalemate on round 2.
    let mut s = Sim::new();
    start_delegated_build(&mut s, json!({"delegated": true}));
    run_failed_round(&mut s, "h1", &[f("src/a.rs", 3, "Bug 1", "p1")], 0);
    s.imported(CARD);
    run_failed_round(&mut s, "h2", &[f("src/a.rs", 3, "#1 Bug 1", "p1"), f("src/b.rs", 7, "Bug 2", "p2")], 0);
    let pending = s.card(CARD).triage.expect("zero progress with additional finding must stall");
    assert!(pending.stalemate);
    assert_eq!(pending.items.len(), 2);
    let ask_text = &open_triage(&s).text;
    assert!(
        ask_text.contains("1 unresolved finding repeated from round 1 (2 for triage)."),
        "repeat count must reflect prior round's findings only: {ask_text}"
    );
    let notice = s.messages(CARD).into_iter().rfind(|m| m.headline == "review stalemate waits for triage").unwrap();
    assert!(
        notice.text.contains("1 unresolved finding repeated from round 1 (2 for triage)."),
        "room notice repeat count must reflect prior round's findings only: {}",
        notice.text
    );

    // Partial progress (#1, #2 -> #2 after #1 resolved) continues on round 2, then stalls on round 3 if #2 repeats.
    let mut p = Sim::new();
    start_delegated_build(&mut p, json!({"delegated": true}));
    run_failed_round(&mut p, "h1", &[f("src/a.rs", 3, "Bug 1", "p1"), f("src/b.rs", 9, "Bug 2", "p2")], 0);
    p.imported(CARD);
    let rc = p.review_card(CARD);
    p.cmd(Sender::Agent(format!("{CARD}/foreman").into()), json!({"resolve": {"card": rc, "n": 1, "declined": true}}))
        .unwrap();
    run_failed_round(&mut p, "h2", &[f("src/b.rs", 9, "#2 Bug 2", "p2")], 0);
    assert!(p.card(CARD).triage.is_none(), "resolving #1 is progress and must not stall on round 2");
    p.imported(CARD);
    run_failed_round(&mut p, "h3", &[f("src/b.rs", 9, "#2 Bug 2", "p2")], 0);
    let pending3 = p.card(CARD).triage.expect("repeating #2 across rounds 2 and 3 must stall");
    assert!(pending3.stalemate);
    assert_eq!(pending3.round, 3);
    assert!(open_triage(&p).text.contains("round 2"));
}

#[test]
fn superseded_rounds_keep_stalemate_and_brief_resets_it() {
    let mut s = Sim::new();
    start_delegated_build(&mut s, json!({"delegated": true}));
    run_failed_round(&mut s, "h1", &[f("src/a.rs", 3, "Bug 1", "p1")], 0);
    s.imported(CARD);

    // Start round 2, let its suite finish so the slot is free while R2 is still open, then supersede it with h3.
    s.built(CARD, "h2", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    s.suite(CARD, 0);
    s.operator(json!({"say": {"place": CARD, "text": "push newer commit"}})).unwrap();
    run_failed_round(&mut s, "h3", &[f("src/a.rs", 3, "#1 Bug 1", "p1")], 0);
    let pending = s.card(CARD).triage.expect("superseded round 2 must not hide stalemate between rounds 1 and 3");
    assert!(pending.stalemate);
    assert!(open_triage(&s).text.contains("round 1"));

    // Going back to brief sets afresh and resets stalemate comparison until two new rounds stall.
    s.operator(json!({"brief": {"card": CARD}})).unwrap();
    assert!(s.card(CARD).triage.is_none());
    let mut recap = reply("brief-ready", "New plan");
    recap["hand"]["message"] = json!("- where: src\n---\nUpdated plan.");
    s.turn(&format!("{CARD}/planner"), recap);
    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    s.cmd(Sender::Orchestrator("web".into()), json!({"approve": {"card": CARD}})).unwrap();
    s.settle_actions();

    run_failed_round(&mut s, "h4", &[f("src/a.rs", 3, "#1 Bug 1", "p1")], 0);
    assert!(s.card(CARD).triage.is_none(), "first round after brief must not stall against pre-brief rounds");
    s.imported(CARD);

    run_failed_round(&mut s, "h5", &[f("src/a.rs", 3, "#1 Bug 1", "p1")], 0);
    assert!(s.card(CARD).triage.is_some_and(|t| t.stalemate), "second round after brief repeating #1 must stall");
}

#[test]
fn stalemate_carries_suite_failure_re_escalates_and_covers_direct_path() {
    let mut s = Sim::new();
    start_delegated_build(&mut s, json!({"delegated": true}));
    run_failed_round(&mut s, "h1", &[f("src/a.rs", 3, "Bug 1", "p1")], 0);
    s.imported(CARD);

    // Round 2 repeats #1 and suite fails: stalemate triage holds suite_failure and combines both on triage.
    run_failed_round(&mut s, "h2", &[f("src/a.rs", 3, "#1 Bug 1", "p1")], 1);
    let pending = s.card(CARD).triage.expect("stalemate with suite failure must open triage");
    assert!(pending.stalemate && pending.suite_failure.is_some());
    assert!(!s.owed(&format!("{CARD}/foreman")));

    s.operator(json!({"triage": {"card": CARD, "fix": ["1"]}})).unwrap();
    s.imported(CARD);
    let req = s.engine_to(CARD, "foreman").into_iter().rfind(|m| m.kind == MessageKind::Request).unwrap();
    assert!(req.text.contains("[fix]") && req.text.contains("The suite failed (exit 1)"), "{}", req.text);

    // If round 3 still repeats #1 without progress, stalemate escalates again against round 2.
    run_failed_round(&mut s, "h3", &[f("src/a.rs", 3, "#1 Bug 1", "p1")], 0);
    assert!(s.card(CARD).triage.is_some_and(|t| t.stalemate && t.round == 3));
    assert!(open_triage(&s).text.contains("round 2"));

    // Non-verification direct path in from_verdict also escalates on stalemate.
    let mut d = Sim::new();
    start_delegated_build(&mut d, json!({"delegated": true}));
    run_failed_round(&mut d, "h1", &[f("src/a.rs", 3, "Bug 1", "p1")], 0);
    d.imported(CARD);
    d.built(CARD, "h2", "base1");
    d.checks(CARD, 0, &[]);
    d.candidate(CARD);
    d.round_asked(CARD);
    let c = d.state.cards.get_mut(&mork_types::CardId::new(CARD)).unwrap();
    c.stage = Stage::Build;
    c.landing = None;
    d.round_ends(CARD, &[], &[f("src/a.rs", 3, "#1 Bug 1", "p1")]);
    d.proofs_copied();
    assert!(d.card(CARD).triage.is_some_and(|t| t.stalemate));
    assert_eq!(open_triage(&d).color, Color::Red);
}
