// fac-644: brief-step helpers mutate the caller's card in place instead of reloading through `Poster::card`.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply};
use mork_core::agents::required;
use mork_core::decide::Decision;
use mork_core::lifecycle::brief;
use mork_core::messages::places::Who;
use mork_core::post::Poster;
use mork_types::ask::AskKind;
use mork_types::card::{MockStep, Stage};
use mork_types::reply::{Reply, Role};
use mork_types::work::Purpose;
use serde_json::{Value, json};

const CARD: &str = "web-1";

fn start_brief(s: &mut Sim, start: Value) {
    s.board(CARD, "web");
    let mut cmd = json!({"card": CARD});
    for (k, v) in start.as_object().unwrap() {
        cmd[k] = v.clone();
    }
    s.operator(json!({"start": cmd})).unwrap();
    s.settle_actions();
    let id = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&id, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Brief);
}

fn recap_reply() -> Value {
    let mut r = reply("brief-ready", "Plan ready");
    r["hand"]["message"] = json!("- module: brief\n---\nThe plan.");
    r
}

fn open_approve_asks(s: &Sim) -> Vec<mork_types::ask::Ask> {
    s.state
        .asks
        .values()
        .filter(|a| a.open && a.kind == AskKind::ApproveBrief && a.card.as_ref().is_some_and(|c| c.as_str() == CARD))
        .cloned()
        .collect()
}

#[test]
fn brief_redraft_preserves_unstaged_caller_changes() {
    let mut s = Sim::new();
    start_brief(&mut s, json!({}));
    s.turn(&format!("{CARD}/planner"), recap_reply());
    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    assert!(s.card(CARD).recap);
    assert_eq!(open_approve_asks(&s).len(), 1);

    let mut c = s.card(CARD);
    c.visual = true;
    let mut p = Poster::for_ctx(&s.state, s.ctx());
    assert!(brief::redraft(&mut p, &mut c));
    assert!(c.visual, "redraft must keep unstaged caller changes on the card");
    assert!(c.draft.is_none() && !c.recap && c.challenge.is_none());
    assert!(!p.card(&c.id).unwrap().visual, "redraft must not stage a reloaded stale card");
    p.put_card(c);

    let mut d = Decision::default();
    p.finish(&mut d);
    s.state.commit(&d.changes);
    assert!(s.card(CARD).visual);
    assert!(s.card(CARD).draft.is_none());
    assert!(open_approve_asks(&s).is_empty());
}

#[test]
fn required_challenge_and_replied_preserve_unstaged_caller_changes() {
    let mut s = Sim::new();
    start_brief(&mut s, json!({}));

    let mut c = s.card(CARD);
    c.visual = true;
    c.draft = Some("The plan.".into());
    let mut p = Poster::for_ctx(&s.state, s.ctx());
    assert!(required::challenge(&mut p, &mut c, &s.config));
    assert!(c.visual && c.challenge.is_some());
    assert!(!p.card(&c.id).unwrap().visual, "challenge must not stage a reloaded stale card");
    p.put_card(c);
    let mut d = Decision::default();
    p.finish(&mut d);
    s.state.commit(&d.changes);

    let foreman = format!("{CARD}/foreman");
    let job_id = s.turn_started(&foreman);
    let job = s.state.jobs[&job_id].clone();
    let who = Who { key: foreman.into(), seat: "foreman".into(), role: Role::Foreman, place: CARD.into() };
    let r: Reply = serde_json::from_value(reply("working", "No objection")).unwrap();

    let mut c = s.card(CARD);
    c.visual = false;
    let mut p = Poster::for_ctx(&s.state, s.ctx());
    assert!(required::replied(&mut p, &mut c, &who, &r, &job, s.ctx()));
    assert!(!c.visual, "replied and brief::offer must keep unstaged caller changes");
    assert!(c.challenge.is_none() && c.recap);
    assert!(p.card(&c.id).unwrap().visual, "replied and offer must not stage a reloaded stale card");
    p.put_card(c);
    let mut d = Decision::default();
    p.finish(&mut d);
    s.state.commit(&d.changes);

    assert!(!s.card(CARD).visual);
    assert!(s.card(CARD).recap);
    assert_eq!(open_approve_asks(&s).len(), 1);
}

#[test]
fn visual_toggle_at_brief_keeps_switch_on_waiting_recap_and_challenge() {
    let mut s = Sim::new();
    start_brief(&mut s, json!({}));
    s.turn(&format!("{CARD}/planner"), recap_reply());
    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    assert!(s.card(CARD).recap);

    s.operator(json!({"visual": {"card": CARD}})).unwrap();
    let c = s.card(CARD);
    assert!(c.visual, "turning visual on at brief must keep the switch");
    assert!(c.draft.is_none() && !c.recap && c.challenge.is_none());
    assert_eq!(c.mock.unwrap().step, MockStep::Baseline);
    assert!(open_approve_asks(&s).is_empty());

    s.operator(json!({"visual": {"card": CARD, "off": true}})).unwrap();
    s.turn(&format!("{CARD}/planner"), recap_reply());
    assert!(s.card(CARD).challenge.is_some());

    s.operator(json!({"visual": {"card": CARD}})).unwrap();
    let c = s.card(CARD);
    assert!(c.visual);
    assert!(c.draft.is_none() && !c.recap && c.challenge.is_none());
    assert_eq!(c.mock.unwrap().step, MockStep::Baseline);
}

#[test]
fn visual_toggle_at_brief_reoffers_given_plan_and_keeps_switch() {
    let mut s = Sim::new();
    start_brief(&mut s, json!({"visual": true, "delegated": true}));
    s.turn(&format!("{CARD}/foreman"), reply("built", "Baseline ready"));
    s.turn(&format!("{CARD}/planner"), recap_reply());
    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Approved);

    // Send back to brief with visual off so a delegated recap is auto-given while mock stays Approved.
    s.operator(json!({"visual": {"card": CARD, "off": true}})).unwrap();
    s.operator(json!({"brief": {"card": CARD, "words": "Revisit the plan"}})).unwrap();
    s.turn(&format!("{CARD}/planner"), recap_reply());
    s.turn(&format!("{CARD}/foreman"), reply("working", "No objection"));
    let given = open_approve_asks(&s);
    assert_eq!(given.len(), 1);
    assert!(given[0].given);

    s.operator(json!({"visual": {"card": CARD}})).unwrap();
    let c = s.card(CARD);
    assert!(c.visual);
    assert!(c.recap);
    let asks = open_approve_asks(&s);
    assert_eq!(asks.len(), 1);
    assert!(!asks[0].given, "visual plan ask must replace the given ask with an ungiven one");
}
