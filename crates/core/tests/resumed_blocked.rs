// fac-590: resuming a card wakes each live seat whose open `blocked` ask `resume <card>` closes.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply};
use mork_types::ask::AskKind;
use mork_types::card::Stage;
use mork_types::state::{AgentStatus, MessageKind, Owed};
use mork_types::work::Purpose;
use serde_json::json;

const CARD: &str = "web-1";

fn start_brief(s: &mut Sim) {
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let id = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&id, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Brief);
}

fn open_blocked(s: &Sim, card: &str) -> usize {
    s.state.asks.values().filter(|a| a.open && a.kind == AskKind::Blocked && a.card.as_deref() == Some(card)).count()
}

fn resumed_requests(s: &Sim, place: &str, to: &str) -> usize {
    s.engine_to(place, to).into_iter().filter(|m| m.kind == MessageKind::Request && m.headline == "Resumed").count()
}

#[test]
fn resume_wakes_foreman_blocked_during_plan_challenge() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let foreman = format!("{CARD}/foreman");

    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("- module: src\n---\nThe plan.");
    s.turn(&planner, recap);
    assert!(s.owed(&foreman), "foreman should be asked to challenge the plan");

    let mut blk = reply("blocked", "Need access");
    blk["hand"]["ask"] = json!("Need credentials");
    s.turn(&foreman, blk);
    assert_eq!(open_blocked(&s, CARD), 1);
    assert!(s.card(CARD).challenge.is_some());
    assert!(!s.owed(&foreman));

    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    assert_eq!(open_blocked(&s, CARD), 0);
    assert!(s.owed(&foreman), "resumed blocked foreman must be owed a turn");
    assert_eq!(resumed_requests(&s, CARD, "foreman"), 1);

    s.turn(&foreman, reply("working", "No objection"));
    assert!(s.card(CARD).challenge.is_none());
    assert!(
        s.state.asks.values().any(|a| a.open && a.kind == AskKind::ApproveBrief && a.card.as_deref() == Some(CARD)),
        "plan should be offered once resumed foreman finishes the challenge"
    );
}

#[test]
fn resume_wakes_planner_at_brief_and_foreman_at_build() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let foreman = format!("{CARD}/foreman");
    let spec = format!("{CARD}/spec");

    let mut blk = reply("blocked", "Need spec");
    blk["hand"]["ask"] = json!("Clarify requirement");
    s.turn(&planner, blk);
    assert_eq!(open_blocked(&s, CARD), 1);
    assert!(!s.owed(&planner));

    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    assert_eq!(open_blocked(&s, CARD), 0);
    assert!(s.owed(&planner), "resumed blocked planner must be owed a turn");
    assert_eq!(resumed_requests(&s, &spec, "planner"), 1);

    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("- module: src\n---\nThe plan.");
    s.turn(&planner, recap);
    s.turn(&foreman, reply("working", "No objection"));
    s.operator(json!({"approve": {"card": CARD}})).unwrap();
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Build);

    let mut build_blk = reply("blocked", "Missing tool");
    build_blk["hand"]["ask"] = json!("Install tool");
    s.turn(&foreman, build_blk);
    assert_eq!(open_blocked(&s, CARD), 1);
    assert!(!s.owed(&foreman));

    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    assert_eq!(open_blocked(&s, CARD), 0);
    assert!(s.owed(&foreman), "resumed blocked foreman at build must be owed a turn");
    assert_eq!(resumed_requests(&s, CARD, "foreman"), 1);
}

#[test]
fn resume_does_not_double_wake_blocked_seat_already_owed_a_turn() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let spec = format!("{CARD}/spec");

    let mut blk = reply("blocked", "Blocked");
    blk["hand"]["ask"] = json!("Need help");
    s.turn(&planner, blk);
    assert_eq!(open_blocked(&s, CARD), 1);

    // An errored seat with a saved prompt is owed a fresh turn by `seats::resumed` (fac-509).
    let a = s.state.agents.get_mut(planner.as_str()).unwrap();
    a.status = AgentStatus::Error { words: "boom".into() };
    a.last_prompt = Some("previous prompt".into());

    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    assert_eq!(open_blocked(&s, CARD), 0);
    let a = &s.state.agents[planner.as_str()];
    assert!(matches!(a.owed, Some(Owed::Wake)), "error seat should get a fresh turn");
    assert!(!a.wake_after, "already-owed seat must not get an extra wake_after");
    assert_eq!(resumed_requests(&s, &spec, "planner"), 0, "no extra Resumed message when retry is already owed");
}

#[test]
fn resume_wakes_blocked_seat_with_stale_wake_after() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let spec = format!("{CARD}/spec");

    let mut blk = reply("blocked", "Blocked");
    blk["hand"]["ask"] = json!("Need help");
    s.turn(&planner, blk);
    assert_eq!(open_blocked(&s, CARD), 1);

    // `stop --agent` clears `owed` without clearing `wake_after`, which cannot launch a turn on its own.
    let a = s.state.agents.get_mut(planner.as_str()).unwrap();
    a.wake_after = true;

    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    assert_eq!(open_blocked(&s, CARD), 0);
    assert!(s.owed(&planner), "stale wake_after must not suppress waking a resumed blocked seat");
    assert!(!s.state.agents[planner.as_str()].wake_after, "stale wake_after must be cleared when waking");
    assert_eq!(resumed_requests(&s, &spec, "planner"), 1);

    s.turn(&planner, reply("working", "Continuing"));
    assert!(!s.owed(&planner), "settling the resumed turn must not queue a second turn");
}
