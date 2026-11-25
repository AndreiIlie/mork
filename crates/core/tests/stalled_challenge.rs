// fac-666: unanswered challenge-place requests between planner and foreman count as owed work for the stall check.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply};
use factory_core::decide::Event;
use factory_types::card::Stage;
use factory_types::state::{MessageKind, Sender};
use factory_types::work::Purpose;
use serde_json::{Value, json};

const CARD: &str = "web-1";

fn start_brief(s: &mut Sim) {
    s.board(CARD, "web");
    s.andrei(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let id = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&id, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Brief);
}

fn reply_with(outcome: &str, headline: &str, to: &str, kind: &str, text: &str) -> Value {
    let mut r = reply(outcome, headline);
    r["messages"] = json!([{"to": to, "kind": kind, "text": text, "images": []}]);
    r
}

fn stalled_nudges(s: &Sim, place: &str, to: &str) -> usize {
    s.engine_to(place, to)
        .into_iter()
        .filter(|m| m.kind == MessageKind::Request && m.headline == "Stalled card")
        .count()
}

#[test]
fn brief_stall_check_waits_for_foreman_reply_in_challenge_place() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let foreman = format!("{CARD}/foreman");
    let spec = format!("{CARD}/spec");

    // Planner sends a request to foreman in the challenge place and ends `working`.
    s.turn(&planner, reply_with("working", "Asking foreman", "foreman", "request", "Check this edge case?"));
    assert!(s.owed(&foreman));

    // Before foreman starts its turn, Tick must not nudge planner.
    s.apply(Event::Tick);
    assert_eq!(stalled_nudges(&s, &spec, "planner"), 0, "planner must not be nudged while foreman owes a reply");
    assert!(s.card(CARD).stalled.is_none());

    // Once foreman replies in the challenge place and planner later stalls with nothing owed, planner is nudged.
    s.turn(&foreman, reply_with("working", "Answered", "planner", "response", "Looks good."));
    s.turn(&planner, reply("working", "Still thinking"));
    s.apply(Event::Tick);
    assert_eq!(stalled_nudges(&s, &spec, "planner"), 1, "planner should be nudged once its request was answered");
    assert!(s.card(CARD).stalled.is_some());
}

#[test]
fn brief_stall_check_nudges_foreman_if_it_drops_challenge_request() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let foreman = format!("{CARD}/foreman");
    let spec = format!("{CARD}/spec");

    s.turn(&planner, reply_with("working", "Asking foreman", "foreman", "request", "Check this?"));
    // Foreman runs and ends `working` without replying to planner.
    s.turn(&foreman, reply("working", "Read it"));

    s.apply(Event::Tick);
    assert_eq!(stalled_nudges(&s, &spec, "planner"), 0, "planner must not be nudged when foreman owes the reply");
    assert_eq!(stalled_nudges(&s, CARD, "foreman"), 1, "foreman should be nudged for its unanswered request");
}

#[test]
fn build_stall_check_ignores_brief_objection_and_tracks_build_request() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let foreman = format!("{CARD}/foreman");
    let spec = format!("{CARD}/spec");

    // Recap is challenged with no objection, then foreman posts an unanswered challenge-place request before approval.
    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("- where: src\n---\nThe plan.");
    s.turn(&planner, recap);
    s.turn(&foreman, reply("working", "No objection"));
    s.cmd(
        Sender::Agent(foreman.clone().into()),
        json!({"say": {"place": CARD, "to": "planner", "text": "Late brief question", "kind": "request"}}),
    )
    .unwrap();
    s.andrei(json!({"approve": {"card": CARD}})).unwrap();
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Build);

    // At build, if foreman ends `working` with nothing owed, the brief-time request must not suppress nudging foreman.
    let clean = json!({"branch": CARD, "head": null, "clean": true, "dirty": [], "bundle": false});
    s.turn(&foreman, reply("working", "Paused"));
    for id in s.open_jobs(|p| matches!(p, Purpose::Bundle { card: c, .. } if c.as_str() == CARD)) {
        s.end_job(&id, clean.clone());
    }
    s.apply(Event::Tick);
    assert_eq!(stalled_nudges(&s, CARD, "foreman"), 1, "brief request must not suppress build stall check");
    assert_eq!(stalled_nudges(&s, &spec, "planner"), 0, "brief request must not nudge planner at build");

    // Now foreman asks planner a specification question in the challenge place: the card moves, clearing the prior
    // stall, and when planner ends `working` without answering, the stall check must nudge planner (not foreman).
    s.turn(&foreman, reply_with("working", "Spec question", "planner", "request", "Should X also apply?"));
    for id in s.open_jobs(|p| matches!(p, Purpose::Bundle { card: c, .. } if c.as_str() == CARD)) {
        s.end_job(&id, clean.clone());
    }
    s.apply(Event::Tick);
    assert!(s.card(CARD).stalled.is_none(), "moving to planner should clear the prior stall");
    s.turn(&planner, reply("working", "Read it without answering"));
    s.apply(Event::Tick);
    assert_eq!(stalled_nudges(&s, CARD, "foreman"), 1, "foreman must not be nudged while planner owes a reply");
    assert_eq!(stalled_nudges(&s, &spec, "planner"), 1, "planner should be nudged for its unanswered build request");
}

#[test]
fn brief_say_challenge_request_prevents_ineffective_outcome_refusal() {
    let mut s = Sim::new();
    start_brief(&mut s);
    let planner = format!("{CARD}/planner");
    let foreman = format!("{CARD}/foreman");
    let spec = format!("{CARD}/spec");

    // Ending with an ineffective outcome (empty `brief-ready`) is not refused while foreman owes a challenge reply.
    let say = json!({"say": {"place": spec, "to": "foreman", "text": "Can you verify this?", "kind": "request"}});
    let mut empty_recap = reply("brief-ready", "Waiting");
    empty_recap["hand"]["message"] = json!("   ");
    s.turn_sending(&planner, &[say], empty_recap);
    assert!(s.owed(&foreman));
    assert!(s.state.agents[planner.as_str()].owed.is_none(), "planner reply must not trigger a correction");
}
