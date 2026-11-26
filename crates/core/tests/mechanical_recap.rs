// fac-649: an orchestrator's answer to a specification holder's own mechanical request keeps its standing recap.

mod common;

use common::{Sim, reply};
use mork_types::ask::{Ask, AskKind};
use mork_types::card::Stage;
use mork_types::state::Sender;
use mork_types::work::Purpose;
use serde_json::json;

const CARD: &str = "web-1";

// why: test helper: a refused start is a test failure.
#[allow(clippy::unwrap_used)]
fn start_brief(s: &mut Sim, card: &str, extra: serde_json::Value) {
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

fn recap_reply() -> serde_json::Value {
    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("- where: src\n---\nThe plan.");
    recap
}

// why: test helper: a missing expected ask is a test failure.
#[allow(clippy::unwrap_used)]
fn open_ask(s: &Sim, kind: AskKind) -> &Ask {
    s.state.asks.values().find(|a| a.open && a.kind == kind).unwrap()
}

// why: test helper: setup commands must succeed.
#[allow(clippy::unwrap_used)]
fn ask_orchestrator(s: &mut Sim, card: &str) {
    let mut req = reply("working", "Mechanical check");
    req["messages"] = json!([{
        "to": "orchestrator",
        "kind": "request",
        "text": "Check the external version outside the wall.",
        "images": [],
    }]);
    s.turn(&format!("{card}/planner"), req);
}

#[test]
fn orchestrator_answer_to_mechanical_request_keeps_challenged_recap_and_avoids_stall() {
    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({}));
    ask_orchestrator(&mut s, CARD);

    s.cmd(
        Sender::Agent(format!("{CARD}/foreman").into()),
        json!({"say": {"place": format!("{CARD}/spec"), "kind": "request", "text": "Ready for recap"}}),
    )
    .unwrap();
    s.turn(&format!("{CARD}/planner"), recap_reply());
    assert!(s.card(CARD).draft.is_some());
    assert!(s.card(CARD).challenge.is_some());
    assert!(!s.card(CARD).recap);

    s.cmd(
        Sender::Orchestrator("web".into()),
        json!({"say": {"place": format!("{CARD}/spec"), "text": "Checked: version 2.1."}}),
    )
    .unwrap();
    assert!(s.card(CARD).draft.is_some(), "mechanical answer must keep challenged draft");
    assert!(s.card(CARD).challenge.is_some(), "mechanical answer must keep waiting challenge");

    s.turn(&format!("{CARD}/planner"), reply("working", "Plan ready"));
    assert!(s.card(CARD).draft.is_some());
    assert!(s.card(CARD).challenge.is_some());
    assert!(s.card(CARD).stalled.is_none(), "planner replying working after mechanical answer must not stall");

    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    assert!(s.card(CARD).recap);
    assert!(s.card(CARD).stalled.is_none());
    assert!(open_ask(&s, AskKind::ApproveBrief).open);
}

#[test]
fn orchestrator_answer_to_mechanical_request_keeps_delegated_waiting_recap_and_plan_ask() {
    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({"delegated": true}));
    ask_orchestrator(&mut s, CARD);

    s.cmd(
        Sender::Agent(format!("{CARD}/foreman").into()),
        json!({"say": {"place": format!("{CARD}/spec"), "kind": "request", "text": "Ready for recap"}}),
    )
    .unwrap();
    s.turn(&format!("{CARD}/planner"), recap_reply());
    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    let plan_id = open_ask(&s, AskKind::ApproveBrief).id.clone();
    assert!(s.state.asks[&plan_id].given);
    assert!(s.card(CARD).recap);

    s.cmd(
        Sender::Orchestrator("web".into()),
        json!({"say": {"place": format!("{CARD}/spec"), "text": "Checked: version 2.1."}}),
    )
    .unwrap();
    assert!(s.card(CARD).recap, "mechanical answer must keep delegated waiting recap");
    assert!(s.card(CARD).draft.is_some());
    assert!(s.state.asks[&plan_id].open, "mechanical answer must keep given approve-brief ask open");
    let answer = s.messages(&format!("{CARD}/spec")).pop().unwrap();
    assert!(answer.delegated.is_none(), "mechanical answer must not be marked as answering approve-brief");

    s.turn(&format!("{CARD}/planner"), reply("working", "Plan ready"));
    assert!(s.card(CARD).recap);
    assert!(s.state.asks[&plan_id].open);
    assert!(s.card(CARD).stalled.is_none());

    s.cmd(
        Sender::Orchestrator("web".into()),
        json!({"say": {"place": format!("{CARD}/spec"), "text": "Split step 2 before approving"}}),
    )
    .unwrap();
    assert!(!s.card(CARD).recap, "follow-up orchestrator message with no pending request withdraws waiting recap");
    assert!(s.card(CARD).draft.is_none());
    assert!(!s.state.asks[&plan_id].open);
    let critique = s.messages(&format!("{CARD}/spec")).pop().unwrap();
    assert_eq!(critique.delegated.as_ref(), Some(&plan_id));
}

#[test]
fn unread_mechanical_answer_does_not_reject_recap_while_delegated_answer_does() {
    let mut s = Sim::new();
    start_brief(&mut s, CARD, json!({"delegated": true}));
    ask_orchestrator(&mut s, CARD);
    s.cmd(
        Sender::Agent(format!("{CARD}/foreman").into()),
        json!({"say": {"place": format!("{CARD}/spec"), "kind": "request", "text": "Proceed"}}),
    )
    .unwrap();
    let job = s.turn_started(&format!("{CARD}/planner"));
    s.cmd(
        Sender::Orchestrator("web".into()),
        json!({"say": {"place": format!("{CARD}/spec"), "text": "Checked: version 2.1."}}),
    )
    .unwrap();
    s.end_job(&job, recap_reply());
    assert!(s.card(CARD).draft.is_some(), "unread mechanical answer must not reject a recap");
    assert!(s.card(CARD).challenge.is_some());

    let mut delegated = Sim::new();
    start_brief(&mut delegated, CARD, json!({"delegated": true}));
    let mut q = reply("working", "Question");
    q["hand"]["ask"] = json!("Keep the existing route?");
    delegated.turn(&format!("{CARD}/planner"), q);
    delegated
        .cmd(
            Sender::Agent(format!("{CARD}/foreman").into()),
            json!({"say": {"place": format!("{CARD}/spec"), "kind": "request", "text": "Proceed"}}),
        )
        .unwrap();
    let job = delegated.turn_started(&format!("{CARD}/planner"));
    delegated
        .cmd(
            Sender::Orchestrator("web".into()),
            json!({"say": {"place": format!("{CARD}/spec"), "text": "Yes, keep it."}}),
        )
        .unwrap();
    delegated.end_job(&job, recap_reply());
    assert!(delegated.card(CARD).draft.is_none(), "unread delegated answer must reject a recap");
}
