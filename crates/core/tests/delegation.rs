// fac-669: delegated and self-sustained cards auto-give seat and non-visual plan asks to the project's orchestrator
// instead of deciding or self-approving alone.

mod common;

use common::{Sim, reply};
use mork_core::decide::{Decision, Event, Intent, JobEnd};
use mork_core::messages::wakes;
use mork_core::state::Change;
use mork_types::ask::{Ask, AskKind};
use mork_types::card::Stage;
use mork_types::orchestrator::{OrchestratorEvent, OrchestratorWhat};
use mork_types::protocol::ErrorCode;
use mork_types::state::{MessageKind, Sender};
use mork_types::work::Purpose;
use serde_json::json;

const CARD: &str = "web-1";

// why: test helper: a refused start is a test failure.
#[allow(clippy::unwrap_used)]
fn start_brief(s: &mut Sim, card: &str, extra: serde_json::Value) {
    s.config.mork.notifications = true;
    s.board(card, "web");
    let mut start = json!({"card": card});
    if let (Some(dst), Some(src)) = (start.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    s.operator(json!({"start": start})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == card))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(card), Stage::Brief);
}

fn offer_plan(s: &mut Sim, card: &str) -> Decision {
    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("- where: src\n---\nThe plan.");
    s.turn(&format!("{card}/planner"), recap);
    s.turn(&format!("{card}/foreman"), reply("working", "No objection"))
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

// why: test helper: a missing expected ask is a test failure.
#[allow(clippy::unwrap_used)]
fn open_ask(s: &Sim, kind: AskKind) -> &Ask {
    s.state.asks.values().find(|a| a.open && a.kind == kind).unwrap()
}

#[test]
fn answer_asks_are_auto_given_without_notice_when_delegated_and_notified_when_off() {
    for (extra, expected_level) in [
        (json!({}), None),
        (json!({"delegated": true}), Some("delegated")),
        (json!({"self-sustained": true}), Some("self-sustained")),
    ] {
        let mut s = Sim::new();
        start_brief(&mut s, CARD, extra);
        let mut r = reply("working", "Question");
        r["hand"]["ask"] = json!("Should we keep the existing route?");
        let d = s.turn(&format!("{CARD}/planner"), r);
        let ask = open_ask(&s, AskKind::Answer);
        let givens = given_events(&d);
        if let Some(level) = expected_level {
            assert!(ask.given, "{level}: answer ask must be auto-given");
            assert!(!ask.notice, "{level}: auto-given ask must not claim a desktop notice");
            assert!(!notifies(&d, ask.id.as_str()), "{level}: auto-given ask must drop Notify intent");
            assert_eq!(givens.len(), 1);
            assert_eq!(
                wakes::line(&s.state, &givens[0].what).as_deref(),
                Some(format!("ask given to you ({level}): {}", ask.id).as_str())
            );
        } else {
            assert!(!ask.given, "off: answer ask must not be given");
            assert!(ask.notice, "off: answer ask must keep desktop notice");
            assert!(notifies(&d, ask.id.as_str()), "off: answer ask must emit Notify intent");
            assert!(givens.is_empty());
        }
    }
}

#[test]
fn delegated_non_visual_plan_waits_for_approve_and_visual_plan_stays_operators() {
    for flag in ["delegated", "self-sustained"] {
        let mut s = Sim::new();
        start_brief(&mut s, CARD, json!({flag: true}));
        let d = offer_plan(&mut s, CARD);
        assert_eq!(s.stage(CARD), Stage::Brief, "{flag}: delegated plan must not self-approve");
        let ask = open_ask(&s, AskKind::ApproveBrief);
        assert!(ask.given);
        assert!(!ask.notice);
        assert!(!notifies(&d, ask.id.as_str()));
        assert_eq!(given_events(&d).len(), 1);
        s.cmd(Sender::Orchestrator("web".into()), json!({"approve": {"card": CARD}})).unwrap();
        assert_eq!(s.stage(CARD), Stage::Build);
        let approved = s
            .messages(CARD)
            .into_iter()
            .any(|m| m.text.contains("orchestrator:web, on the operator's delegation, approved"));
        assert!(approved, "{flag}: approval must be announced as orchestrator's on the operator's delegation");
    }

    let mut s = Sim::new();
    s.config.mork.vite = Some("vite".into());
    start_brief(&mut s, CARD, json!({"delegated": true, "visual": true}));
    s.turn(&format!("{CARD}/foreman"), reply("built", "Baseline done"));
    let d = offer_plan(&mut s, CARD);
    assert_eq!(s.stage(CARD), Stage::Brief);
    let ask = open_ask(&s, AskKind::ApproveBrief);
    assert!(!ask.given, "visual card's plan ask must never be given");
    assert!(notifies(&d, ask.id.as_str()), "visual card's plan ask keeps its desktop notice");
    let err = s.cmd(Sender::Orchestrator("web".into()), json!({"approve": {"card": CARD}})).unwrap_err();
    assert_eq!(err.code, ErrorCode::Buzz);
}

#[test]
fn own_project_orchestrator_words_withdraw_a_given_plan_and_leave_ungiven_waiting() {
    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({"delegated": true}));
    offer_plan(&mut s, CARD);
    let ask_id = open_ask(&s, AskKind::ApproveBrief).id.clone();

    let other = Sender::Orchestrator("ex".into());
    s.cmd(other, json!({"say": {"place": format!("{CARD}/spec"), "text": "Other project note"}})).unwrap();
    assert!(s.card(CARD).recap, "another project's orchestrator leaves a waiting plan waiting");
    assert!(s.state.asks[&ask_id].open);

    let own = Sender::Orchestrator("web".into());
    s.cmd(own, json!({"say": {"place": format!("{CARD}/spec"), "text": "Split step 2 before approving"}})).unwrap();
    assert!(!s.card(CARD).recap, "own-project orchestrator holding the given plan ask withdraws it");
    assert!(s.card(CARD).draft.is_none());
    assert!(!s.state.asks[&ask_id].open);
    let last = s.messages(&format!("{CARD}/spec")).pop().unwrap();
    assert_eq!(last.delegated.as_ref(), Some(&ask_id));

    let mut off = Sim::new();
    start_brief(&mut off, CARD, json!({}));
    offer_plan(&mut off, CARD);
    let ungiven_id = open_ask(&off, AskKind::ApproveBrief).id.clone();
    off.cmd(
        Sender::Orchestrator("web".into()),
        json!({"say": {"place": format!("{CARD}/spec"), "text": "Note while ungiven"}}),
    )
    .unwrap();
    assert!(off.card(CARD).recap, "ungiven plan ask stays waiting through orchestrator words");
    assert!(off.state.asks[&ungiven_id].open);
    let msg = off.messages(&format!("{CARD}/spec")).pop().unwrap();
    assert!(msg.delegated.is_none());
}

#[test]
fn delegate_toggle_gives_waiting_asks_and_rewakes_on_self_sustained() {
    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({}));
    offer_plan(&mut s, CARD);
    let plan_ask = open_ask(&s, AskKind::ApproveBrief).id.clone();
    assert!(!s.state.asks[&plan_ask].given);

    let on = s.apply(Event::Command {
        from: Sender::Operator,
        cmd: serde_json::from_value(json!({"delegate": {"card": CARD}})).unwrap(),
    });
    assert_eq!(s.stage(CARD), Stage::Brief, "delegate on must not approve a waiting plan");
    assert!(s.state.asks[&plan_ask].given);
    let givens = given_events(&on);
    assert_eq!(givens.len(), 1);
    assert_eq!(
        wakes::line(&s.state, &givens[0].what).as_deref(),
        Some(format!("ask given to you (delegated): {plan_ask}").as_str())
    );

    let ss = s.apply(Event::Command {
        from: Sender::Operator,
        cmd: serde_json::from_value(json!({"delegate": {"card": CARD, "self-sustained": true}})).unwrap(),
    });
    let re = given_events(&ss);
    assert_eq!(re.len(), 1, "delegated -> self-sustained re-emits Given for open given asks");
    assert_eq!(
        wakes::line(&s.state, &re[0].what).as_deref(),
        Some(format!("ask given to you (self-sustained): {plan_ask}").as_str())
    );

    s.operator(json!({"delegate": {"card": CARD, "off": true}})).unwrap();
    assert!(s.state.asks[&plan_ask].given, "delegate off leaves already-given asks with the orchestrator");
    assert_eq!(wakes::line(&s.state, &re[0].what).as_deref(), Some(format!("ask given to you: {plan_ask}").as_str()));
}

#[test]
fn visual_off_on_delegated_waiting_plan_gives_plan_without_mock_link() {
    let mut s = Sim::new();
    s.config.mork.vite = Some("vite".into());
    start_brief(&mut s, CARD, json!({"delegated": true, "visual": true}));
    s.turn(&format!("{CARD}/foreman"), reply("built", "Baseline done"));
    offer_plan(&mut s, CARD);
    let old = open_ask(&s, AskKind::ApproveBrief);
    assert!(!old.given);
    assert!(old.text.contains("mock:"));

    let d = s.apply(Event::Command {
        from: Sender::Operator,
        cmd: serde_json::from_value(json!({"visual": {"card": CARD, "off": true}})).unwrap(),
    });
    assert_eq!(s.stage(CARD), Stage::Brief);
    let next = open_ask(&s, AskKind::ApproveBrief);
    assert!(next.given, "visual --off on a delegated waiting plan must auto-give the new plan ask");
    assert!(!next.text.contains("mock:"), "re-offered plan ask must drop the mock link: {}", next.text);
    assert!(!notifies(&d, next.id.as_str()));
    assert_eq!(given_events(&d).len(), 1);
}

#[test]
fn blocked_and_engine_asks_are_not_auto_given() {
    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({"delegated": true}));
    let mut blocked = reply("blocked", "Need credentials");
    blocked["hand"]["ask"] = json!("Please add the staging key");
    let d = s.turn(&format!("{CARD}/planner"), blocked);
    let ask = open_ask(&s, AskKind::Blocked);
    assert!(!ask.given, "blocked asks must never be auto-given");
    assert!(notifies(&d, ask.id.as_str()));
    assert!(given_events(&d).is_empty());

    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({"delegated": true}));
    let id = s.turn_started(&format!("{CARD}/planner"));
    let d = s.apply(Event::JobEnded { id, end: JobEnd::Failed { words: "boom".into() } });
    let err = open_ask(&s, AskKind::Error);
    assert!(!err.given, "engine asks must never be auto-given");
    assert!(notifies(&d, err.id.as_str()));
    assert!(given_events(&d).is_empty());
}

// why: test helper: setup commands must succeed.
#[allow(clippy::unwrap_used)]
fn to_round_failed(s: &mut Sim, extra: serde_json::Value, findings: &[serde_json::Value]) {
    start_brief(s, CARD, extra);
    offer_plan(s, CARD);
    if s.card(CARD).delegated {
        s.cmd(Sender::Orchestrator("web".into()), json!({"approve": {"card": CARD}})).unwrap();
    } else {
        s.operator(json!({"approve": {"card": CARD}})).unwrap();
    }
    s.settle_actions();
    s.built(CARD, "h1", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    s.round_ends(CARD, &[], findings);
    s.proofs_copied();
}

#[test]
fn failed_review_round_routes_by_delegation_level() {
    let finding = json!({"file": "src/a.rs", "line": 3, "summary": "src/a.rs is wrong", "proof": "a proof"});
    for extra in [json!({}), json!({"delegated": true}), json!({"self-sustained": true})] {
        let mut s = Sim::new();
        to_round_failed(&mut s, extra.clone(), std::slice::from_ref(&finding));
        let d = s.suite(CARD, 0);
        if s.card(CARD).delegated {
            assert!(s.card(CARD).triage.is_none(), "{extra}: delegated card must not open triage");
            assert!(!s.state.asks.values().any(|a| a.open && a.kind == AskKind::Triage));
            assert!(given_events(&d).is_empty(), "{extra}: delegated failed round must not wake orchestrator");
            s.imported(CARD);
            let req = s.engine_to(CARD, "foreman").into_iter().rfind(|m| m.kind == MessageKind::Request).unwrap();
            assert!(
                req.text.contains("1. (#1) [yours to decide] src/a.rs:3: src/a.rs is wrong"),
                "{extra}: {}",
                req.text
            );
            assert!(!req.text.contains("Triaged by"), "{extra}: {}", req.text);
            let rc = s.review_card(CARD);
            s.cmd(
                Sender::Agent(format!("{CARD}/foreman").into()),
                json!({"resolve": {"card": rc, "n": 1, "declined": true}}),
            )
            .unwrap();
        } else {
            let ask = open_ask(&s, AskKind::Triage);
            assert!(!ask.given, "{extra}: non-delegated triage ask must stay the operator's");
            assert!(
                ask.notice && notifies(&d, ask.id.as_str()),
                "{extra}: non-delegated triage must notify the operator"
            );
            assert!(
                s.cmd(Sender::Orchestrator("web".into()), json!({"triage": {"card": CARD, "fix": ["1"]}})).is_err(),
                "{extra}: ungiven orchestrator must be refused"
            );
            s.operator(json!({"triage": {"card": CARD, "fix": ["1"]}})).unwrap();
            s.imported(CARD);
            let req = s.engine_to(CARD, "foreman").into_iter().rfind(|m| m.kind == MessageKind::Request).unwrap();
            assert!(req.text.contains("[fix]"), "{extra}: {}", req.text);
        }
    }
}

#[test]
fn delegated_failed_round_and_suite_failure_combine_into_one_foreman_request() {
    let finding = json!({"file": "src/a.rs", "line": 3, "summary": "Broken check", "proof": "a proof"});
    let mut s = Sim::new();
    to_round_failed(&mut s, json!({"delegated": true}), std::slice::from_ref(&finding));
    let reqs_before = s.engine_to(CARD, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).count();
    assert!(!s.owed(&format!("{CARD}/foreman")), "failed round must still wait for suite");

    let d = s.suite(CARD, 1);
    assert!(s.card(CARD).triage.is_none());
    assert!(given_events(&d).is_empty());
    s.imported(CARD);
    let reqs: Vec<_> = s.engine_to(CARD, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).collect();
    assert_eq!(reqs.len(), reqs_before + 1);
    let last = reqs.last().unwrap();
    assert!(last.text.contains("1. (#1) [yours to decide] src/a.rs:3: Broken check"), "{}", last.text);
    assert!(last.text.contains("The suite failed (exit 1)"), "{}", last.text);
}

#[test]
fn turning_delegation_on_releases_ungiven_triage_and_keeps_given_triage() {
    let finding = json!({"file": "src/a.rs", "line": 3, "summary": "Needs decision", "proof": "a proof"});
    for cmd in [json!({"delegate": {"card": CARD}}), json!({"delegate": {"card": CARD, "self-sustained": true}})] {
        let mut s = Sim::new();
        to_round_failed(&mut s, json!({}), std::slice::from_ref(&finding));
        s.suite(CARD, 1);
        let ask_id = open_ask(&s, AskKind::Triage).id.clone();

        let d = s.apply(Event::Command { from: Sender::Operator, cmd: serde_json::from_value(cmd.clone()).unwrap() });
        assert!(d.answer.as_ref().is_some_and(Result::is_ok));
        assert!(!s.state.asks[&ask_id].open, "{cmd}: ungiven triage ask must close on delegate on");
        assert!(s.card(CARD).triage.is_none());
        assert!(given_events(&d).is_empty(), "{cmd}: must not give triage ask to orchestrator");
        s.imported(CARD);
        let req = s.engine_to(CARD, "foreman").into_iter().rfind(|m| m.kind == MessageKind::Request).unwrap();
        assert!(req.text.contains("1. (#1) [yours to decide] src/a.rs:3: Needs decision"), "{}", req.text);
        assert!(req.text.contains("The suite failed (exit 1)"), "{}", req.text);
    }

    let mut s = Sim::new();
    to_round_failed(&mut s, json!({}), std::slice::from_ref(&finding));
    s.suite(CARD, 0);
    let ask_id = open_ask(&s, AskKind::Triage).id.clone();
    s.operator(json!({"give": {"ask": ask_id.as_str()}})).unwrap();
    let d = s.apply(Event::Command {
        from: Sender::Operator,
        cmd: serde_json::from_value(json!({"delegate": {"card": CARD}})).unwrap(),
    });
    assert!(s.state.asks[&ask_id].open && s.state.asks[&ask_id].given, "already-given triage stays given");
    assert!(s.card(CARD).triage.is_some());
    assert!(given_events(&d).is_empty());
    s.cmd(Sender::Orchestrator("web".into()), json!({"triage": {"card": CARD, "fix": ["1"]}})).unwrap();
    assert!(s.card(CARD).triage.is_none());
}

#[test]
fn delegation_toggle_while_failed_round_waits_for_suite_uses_current_level() {
    let finding = json!({"file": "src/a.rs", "line": 3, "summary": "Late toggle", "proof": "a proof"});
    let mut s = Sim::new();
    to_round_failed(&mut s, json!({}), std::slice::from_ref(&finding));
    s.operator(json!({"delegate": {"card": CARD}})).unwrap();
    let d = s.suite(CARD, 0);
    assert!(s.card(CARD).triage.is_none());
    assert!(given_events(&d).is_empty());
    s.imported(CARD);
    let req = s.engine_to(CARD, "foreman").into_iter().rfind(|m| m.kind == MessageKind::Request).unwrap();
    assert!(req.text.contains("[yours to decide]"), "{}", req.text);

    let mut off = Sim::new();
    to_round_failed(&mut off, json!({"delegated": true}), std::slice::from_ref(&finding));
    off.operator(json!({"delegate": {"card": CARD, "off": true}})).unwrap();
    let d = off.suite(CARD, 0);
    let ask = open_ask(&off, AskKind::Triage);
    assert!(!ask.given && ask.notice && notifies(&d, ask.id.as_str()));
}

#[test]
fn paused_all_resolved_triage_restarts_gates_on_resume_and_direct_non_verification_dispatches() {
    let finding = json!({"file": "src/a.rs", "line": 3, "summary": "Single nit", "proof": "a proof"});
    let mut s = Sim::new();
    to_round_failed(&mut s, json!({}), std::slice::from_ref(&finding));
    s.suite(CARD, 0);
    let rc = s.review_card(CARD);
    s.operator(json!({"stop": {"card": CARD}})).unwrap();
    s.operator(json!({"resolve": {"card": rc, "n": 1, "declined": true}})).unwrap();
    s.operator(json!({"delegate": {"card": CARD}})).unwrap();
    assert!(s.card(CARD).triage.is_none());
    assert_eq!(s.card(CARD).gates.checks_waiting, s.card(CARD).head);
    assert_eq!(s.stage(CARD), Stage::Build, "paused card must not start checks until resume");
    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    assert_eq!(s.stage(CARD), Stage::Checks, "resume must restart checks after paused all-resolved release");

    let mut d = Sim::new();
    start_brief(&mut d, CARD, json!({"delegated": true}));
    offer_plan(&mut d, CARD);
    d.cmd(Sender::Orchestrator("web".into()), json!({"approve": {"card": CARD}})).unwrap();
    d.settle_actions();
    d.built(CARD, "h1", "base1");
    d.checks(CARD, 0, &[]);
    d.candidate(CARD);
    d.round_asked(CARD);
    let c = d.state.cards.get_mut(&mork_types::CardId::new(CARD)).unwrap();
    c.stage = Stage::Build;
    c.landing = None;
    d.round_ends(CARD, &[], std::slice::from_ref(&finding));
    d.proofs_copied();
    assert!(d.card(CARD).triage.is_none());
    assert!(!d.state.asks.values().any(|a| a.open && a.kind == AskKind::Triage));
    let req = d.engine_to(CARD, "foreman").into_iter().rfind(|m| m.kind == MessageKind::Request).unwrap();
    assert_eq!(req.headline, "Review round 1 failed");
    assert!(req.text.contains("1. (#1) [yours to decide] src/a.rs:3: Single nit"), "{}", req.text);
    assert!(!req.text.contains("Triaged by"), "{}", req.text);
}

#[test]
fn visual_on_over_approved_mock_at_brief_replaces_given_plan_ask_with_operators() {
    let mut s = Sim::new();
    s.config.mork.vite = Some("vite".into());
    start_brief(&mut s, CARD, json!({"delegated": true, "visual": true}));
    s.turn(&format!("{CARD}/foreman"), reply("built", "Baseline done"));
    offer_plan(&mut s, CARD);
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    s.operator(json!({"visual": {"card": CARD, "off": true}})).unwrap();
    s.operator(json!({"brief": {"card": CARD}})).unwrap();
    offer_plan(&mut s, CARD);
    let old = open_ask(&s, AskKind::ApproveBrief);
    assert!(old.given);
    let old_id = old.id.clone();

    let d = s.apply(Event::Command {
        from: Sender::Operator,
        cmd: serde_json::from_value(json!({"visual": {"card": CARD}})).unwrap(),
    });
    assert!(!s.state.asks[&old_id].open, "old given plan ask must close when visual turns on");
    let next = open_ask(&s, AskKind::ApproveBrief);
    assert!(!next.given, "visual card's plan ask must stay the operator's alone");
    assert!(next.notice, "visual card's plan ask must notify the operator");
    assert!(notifies(&d, next.id.as_str()));
    assert!(given_events(&d).is_empty());
}

#[test]
fn orchestrator_plan_correction_marks_approve_brief_over_other_given_asks() {
    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({"delegated": true}));
    let mut question = reply("working", "Question");
    question["hand"]["ask"] = json!("Keep the existing route?");
    s.turn(&format!("{CARD}/planner"), question);
    let answer_id = open_ask(&s, AskKind::Answer).id.clone();
    assert!(s.state.asks[&answer_id].given);

    s.cmd(
        Sender::Agent(format!("{CARD}/foreman").into()),
        json!({"say": {"place": format!("{CARD}/spec"), "kind": "request", "text": "Wake without closing"}}),
    )
    .unwrap();
    offer_plan(&mut s, CARD);
    let plan_id = open_ask(&s, AskKind::ApproveBrief).id.clone();
    assert!(s.state.asks[&answer_id].open);

    s.cmd(
        Sender::Agent(format!("{CARD}/foreman").into()),
        json!({"say": {"place": format!("{CARD}/spec"), "kind": "request", "text": "Wake without withdrawing"}}),
    )
    .unwrap();
    let mut blocked = reply("blocked", "Need access");
    blocked["hand"]["ask"] = json!("Grant access");
    s.turn(&format!("{CARD}/planner"), blocked);
    let blocked_id = open_ask(&s, AskKind::Blocked).id.clone();
    s.operator(json!({"give": {"ask": blocked_id.as_str()}})).unwrap();
    assert!(s.card(CARD).recap);

    s.cmd(
        Sender::Orchestrator("web".into()),
        json!({"say": {"place": format!("{CARD}/spec"), "text": "Adjust step 1"}}),
    )
    .unwrap();
    assert!(!s.card(CARD).recap);
    assert!(!s.state.asks[&plan_id].open);
    assert!(!s.state.asks[&answer_id].open);
    assert!(!s.state.asks[&blocked_id].open);
    let last = s.messages(&format!("{CARD}/spec")).pop().unwrap();
    assert_eq!(last.delegated.as_ref(), Some(&plan_id));
}
