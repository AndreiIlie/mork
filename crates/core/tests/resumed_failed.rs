// fac-509: resuming a failed turn runs a fresh turn on today's news, each item read once.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::{Sim, reply};
use factory_core::decide::{Event, JobEnd};
use factory_types::CardId;
use factory_types::card::Stage;
use factory_types::state::Owed;
use factory_types::work::Purpose;
use serde_json::json;

const CARD: &str = "web-1";
const PLANNER: &str = "web-1/planner";

#[test]
fn resumed_failed_turn_reads_later_news_and_the_next_turn_none_again() {
    let mut s = Sim::new();
    s.board(CARD, "web");
    s.andrei(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let id = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&id, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Brief);
    s.turn(PLANNER, reply("working", "Reading"));

    s.andrei(json!({"say": {"place": format!("{CARD}/spec"), "text": "Join request."}})).unwrap();
    let (id, failed) = s.turn_started_with_argv(PLANNER, &["x"]);
    assert!(failed.contains("Join request."));
    s.apply(Event::JobEnded { id, end: JobEnd::Failed { words: "bwrap: no clone".into() } });

    // Announced after the failure, waking nobody: the old prompt lacks it.
    let mut edited = s.state.board[&CardId::new(CARD)].clone();
    edited.title = "Later announcement.".into();
    s.apply(Event::BoardsRead { cards: vec![edited], prefixes: BTreeMap::new() });
    assert!(!s.owed(PLANNER), "the announcement must not wake the errored seat");
    s.andrei(json!({"resume": {"agent": PLANNER}})).unwrap();
    assert!(matches!(s.state.agents[PLANNER].owed, Some(Owed::Wake)));
    let (id, resumed) = s.turn_started_with_argv(PLANNER, &["x"]);
    assert!(resumed.contains("Join request."), "the failed turn's news is given back");
    assert!(resumed.contains("Later announcement."), "the resumed turn reads news posted since");
    let session = factory_core::decide::SessionSeen { started: true, ..Default::default() };
    s.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Ok(reply("working", "Joined")), session } });

    s.andrei(json!({"say": {"place": format!("{CARD}/spec"), "text": "Next words."}})).unwrap();
    let (_, next) = s.turn_started_with_argv(PLANNER, &["x"]);
    assert!(next.contains("Next words."));
    assert!(!next.contains("Join request."), "news already read is not given again");
}
