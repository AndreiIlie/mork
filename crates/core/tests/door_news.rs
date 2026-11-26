// fac-632: only approval-gated and held door calls owe news when no caller waits; direct calls never replay.
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::Sim;
use mork_core::decide::{Event, Intent};
use mork_core::door::{owed_news, status_of};
use mork_types::action::{Action, ActionStatus};
use mork_types::door::status;
use mork_types::door_call::{DoorCall, Verdict};
use mork_types::state::Sender;
use mork_types::work::Found;
use mork_types::{ActionId, AgentKey, CardId, JobId, PlaceId};
use serde_json::{Value, json};

fn call(op: &str, params: &str, plan: &str, turn: &str) -> DoorCall {
    DoorCall {
        from: Sender::Agent(AgentKey::new("web-1/foreman")),
        place: PlaceId::new("web-1"),
        card: Some(CardId::new("web-1")),
        job: Some(JobId::new(turn)),
        turn: Some(JobId::new(turn)),
        target: "replica".into(),
        op: op.into(),
        params: params.into(),
        plan: plan.into(),
        nonce: format!("nonce-{op}-{params}"),
    }
}

fn ask_door(sim: &mut Sim, call: DoorCall, verdict: Verdict) -> (ActionId, Value) {
    let d = sim.apply(Event::DoorAsked { call: Box::new(call), validator: verdict, stopped: Vec::new() });
    let answer = d.answer.unwrap().unwrap();
    let id = ActionId::new(answer["door"]["action"].as_str().unwrap());
    (id, answer)
}

fn start_door(sim: &mut Sim, id: &ActionId, plan: &str, verdict: &Verdict) {
    sim.apply(Event::ActionStarted { id: id.clone(), prepared: Some(json!({"plan": plan, "verdict": verdict})) });
}

fn action(sim: &Sim, id: &ActionId) -> Action {
    sim.state.actions.get(id).cloned().unwrap()
}

fn door_news(sim: &Sim, place: &str) -> Vec<String> {
    sim.engine_to(place, "foreman")
        .into_iter()
        .filter(|m| m.headline.starts_with("door: "))
        .map(|m| m.headline)
        .collect()
}

#[test]
fn direct_reads_never_owe_news_and_dedup_on_same_turn_retry() {
    let mut sim = Sim::new();
    sim.through_brief("web-1", "web");
    let verdict = Verdict::new("allowed", "a read");

    let plan1 = r#"{"sql":"SELECT 1"}"#;
    let c1 = call("sql.select", r#"{"query":"SELECT 1"}"#, plan1, "turn-1");
    let (id1, _) = ask_door(&mut sim, c1.clone(), verdict.clone());
    start_door(&mut sim, &id1, plan1, &verdict);
    let rows = json!({"status": status::DONE, "rows": [[1]], "elapsed": 4});
    sim.apply(Event::ActionReturned { id: id1.clone(), result: Ok(rows) });
    assert!(!owed_news(&action(&sim, &id1)));
    let (retry_id, retry_answer) = ask_door(&mut sim, c1, verdict.clone());
    assert_eq!(retry_id, id1);
    assert_eq!(retry_answer["door"]["status"], status::DONE);
    assert_eq!(retry_answer["door"]["rows"], json!([[1]]));

    let plan2 = r#"{"sql":"SELECT 2"}"#;
    let (id2, _) = ask_door(&mut sim, call("sql.select", r#"{"query":"SELECT 2"}"#, plan2, "turn-1"), verdict.clone());
    start_door(&mut sim, &id2, plan2, &verdict);
    sim.apply(Event::ActionFound {
        id: id2.clone(),
        found: Found::Done { result: json!({"status": status::DONE, "rows": [[2]], "elapsed": 5}) },
    });
    assert!(!owed_news(&action(&sim, &id2)));

    let plan3 = r#"{"sql":"SELECT 3"}"#;
    let (id3, _) = ask_door(&mut sim, call("sql.select", r#"{"query":"SELECT 3"}"#, plan3, "turn-1"), verdict.clone());
    start_door(&mut sim, &id3, plan3, &verdict);
    sim.apply(Event::ActionFound { id: id3.clone(), found: Found::Unclear { words: "no receipt".into() } });
    assert!(!owed_news(&action(&sim, &id3)));

    let plan4 = r#"{"sql":"SELECT 4"}"#;
    let (id4, _) = ask_door(&mut sim, call("sql.select", r#"{"query":"SELECT 4"}"#, plan4, "turn-1"), verdict.clone());
    start_door(&mut sim, &id4, plan4, &verdict);
    sim.apply(Event::ActionReturned {
        id: id4.clone(),
        result: Ok(json!({"status": status::FAILED, "text": "stopped: its caller went away", "caller_went": true})),
    });
    assert!(!owed_news(&action(&sim, &id4)));
}

#[test]
fn unstarted_pending_read_dropped_owes_no_news_and_reasks_fresh() {
    let mut sim = Sim::new();
    sim.through_brief("web-1", "web");
    let verdict = Verdict::new("allowed", "a read");
    let plan = r#"{"sql":"SELECT 9"}"#;
    let req = call("sql.select", r#"{"query":"SELECT 9"}"#, plan, "turn-1");

    let (id1, _) = ask_door(&mut sim, req.clone(), verdict.clone());
    sim.apply(Event::DoorUnstarted { id: id1.clone(), busy: None });
    let dropped = action(&sim, &id1);
    assert_eq!(dropped.status, ActionStatus::Dropped);
    assert!(!owed_news(&dropped));

    let (id2, ans2) = ask_door(&mut sim, req, verdict.clone());
    assert_ne!(id2, id1);
    assert_eq!(ans2["door"]["status"], status::UNDER_WAY);

    sim.apply(Event::DoorUnstarted { id: id2.clone(), busy: Some("waiting; nothing ran; ask again".into()) });
    assert_eq!(action(&sim, &id2).status, ActionStatus::Dropped);
    assert!(!owed_news(&action(&sim, &id2)));

    let plan3 = r#"{"sql":"SELECT 10"}"#;
    let (id3, _) = ask_door(&mut sim, call("sql.select", r#"{"query":"SELECT 10"}"#, plan3, "turn-1"), verdict);
    sim.operator(json!({"stop": {"card": "web-1"}})).unwrap();
    assert!(action(&sim, &id3).cancelled);
    sim.apply(Event::DoorUnstarted { id: id3.clone(), busy: None });
    let dropped_cancelled = action(&sim, &id3);
    assert_eq!(dropped_cancelled.status, ActionStatus::Dropped);
    assert!(!dropped_cancelled.cancelled);
    assert!(!owed_news(&dropped_cancelled));
    sim.intents.clear();
    sim.operator(json!({"resume": {"card": "web-1"}})).unwrap();
    assert!(!sim.intents.iter().any(|i| matches!(i, Intent::RunAction { id } if *id == id3)));
}

#[test]
fn direct_sh_run_owes_news_only_when_held_or_settled_by_operator() {
    let mut sim = Sim::new();
    sim.through_brief("web-1", "web");
    let verdict = Verdict::new("allowed", "a named command of the target");

    let plan1 = r#"{"sh":"status"}"#;
    let (id1, _) = ask_door(&mut sim, call("sh.run", r#"{"command":"status"}"#, plan1, "turn-1"), verdict.clone());
    start_door(&mut sim, &id1, plan1, &verdict);
    sim.apply(Event::ActionReturned {
        id: id1.clone(),
        result: Ok(json!({"status": status::DONE, "text": "ok", "elapsed": 2})),
    });
    assert!(!owed_news(&action(&sim, &id1)));

    let plan2 = r#"{"sh":"changed"}"#;
    let (id2, _) = ask_door(&mut sim, call("sh.run", r#"{"command":"changed"}"#, plan2, "turn-1"), verdict.clone());
    start_door(&mut sim, &id2, r#"{"sh":"other"}"#, &verdict);
    assert_eq!(action(&sim, &id2).status, ActionStatus::Dropped);
    assert!(!owed_news(&action(&sim, &id2)));

    let plan3 = r#"{"sh":"deploy"}"#;
    let (id3, _) = ask_door(&mut sim, call("sh.run", r#"{"command":"deploy"}"#, plan3, "turn-1"), verdict.clone());
    start_door(&mut sim, &id3, plan3, &verdict);
    sim.apply(Event::ActionFound { id: id3.clone(), found: Found::Unclear { words: "no receipt".into() } });
    assert_eq!(action(&sim, &id3).status, ActionStatus::Held);
    assert!(owed_news(&action(&sim, &id3)));
    sim.apply(Event::DoorDelivered { id: id3.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id3)));
    assert_eq!(door_news(&sim, "web-1"), vec!["door: sh.run on replica: held"]);

    sim.operator(json!({"action.done": {"action": id3, "note": "checked"}})).unwrap();
    assert!(owed_news(&action(&sim, &id3)));
    sim.apply(Event::DoorDelivered { id: id3.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id3)));
    assert_eq!(door_news(&sim, "web-1").len(), 2);

    let plan4 = r#"{"sh":"restart"}"#;
    let (id4, _) = ask_door(&mut sim, call("sh.run", r#"{"command":"restart"}"#, plan4, "turn-1"), verdict.clone());
    start_door(&mut sim, &id4, plan4, &verdict);
    sim.apply(Event::ActionFound { id: id4.clone(), found: Found::Unclear { words: "no receipt".into() } });
    sim.operator(json!({"action.drop": {"action": id4}})).unwrap();
    assert_eq!(status_of(&action(&sim, &id4)), status::NOT_ALLOWED);
    assert!(owed_news(&action(&sim, &id4)));
    sim.apply(Event::DoorDelivered { id: id4.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id4)));
}

#[test]
fn asked_misc_run_owes_news_for_done_refused_guard_and_held() {
    let mut sim = Sim::new();
    sim.through_brief("web-1", "web");
    let outside = Verdict::new("outside", "the misc door always asks the operator");

    let plan1 = r#"{"misc":["echo","hi"]}"#;
    let (id1, ans1) =
        ask_door(&mut sim, call("misc.run", r#"{"argv":["echo","hi"]}"#, plan1, "turn-1"), outside.clone());
    assert!(!owed_news(&action(&sim, &id1)));
    let ask1 = ans1["door"]["ask"].as_str().unwrap().to_owned();
    sim.operator(json!({"door.once": {"ask": ask1}})).unwrap();
    start_door(&mut sim, &id1, plan1, &outside);
    sim.apply(Event::ActionReturned {
        id: id1.clone(),
        result: Ok(json!({"status": status::DONE, "text": "hi", "elapsed": 3})),
    });
    assert!(owed_news(&action(&sim, &id1)));
    sim.apply(Event::DoorDelivered { id: id1.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id1)));
    assert_eq!(door_news(&sim, "web-1"), vec!["door: misc.run on replica: done"]);

    let plan2 = r#"{"misc":["echo","no"]}"#;
    let (id2, ans2) =
        ask_door(&mut sim, call("misc.run", r#"{"argv":["echo","no"]}"#, plan2, "turn-1"), outside.clone());
    let ask2 = ans2["door"]["ask"].as_str().unwrap().to_owned();
    sim.operator(json!({"door.no": {"ask": ask2}})).unwrap();
    assert!(owed_news(&action(&sim, &id2)));
    sim.apply(Event::DoorDelivered { id: id2.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id2)));

    let plan3 = r#"{"misc":["echo","guard"]}"#;
    let (id3, ans3) =
        ask_door(&mut sim, call("misc.run", r#"{"argv":["echo","guard"]}"#, plan3, "turn-1"), outside.clone());
    let ask3 = ans3["door"]["ask"].as_str().unwrap().to_owned();
    sim.operator(json!({"door.once": {"ask": ask3}})).unwrap();
    start_door(&mut sim, &id3, r#"{"misc":["changed"]}"#, &outside);
    assert_eq!(action(&sim, &id3).status, ActionStatus::Dropped);
    assert!(owed_news(&action(&sim, &id3)));
    sim.apply(Event::DoorDelivered { id: id3.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id3)));

    let plan4 = r#"{"misc":["echo","held"]}"#;
    let (id4, ans4) =
        ask_door(&mut sim, call("misc.run", r#"{"argv":["echo","held"]}"#, plan4, "turn-1"), outside.clone());
    let ask4 = ans4["door"]["ask"].as_str().unwrap().to_owned();
    sim.operator(json!({"door.once": {"ask": ask4}})).unwrap();
    start_door(&mut sim, &id4, plan4, &outside);
    sim.apply(Event::ActionFound { id: id4.clone(), found: Found::Unclear { words: "no receipt".into() } });
    assert_eq!(action(&sim, &id4).status, ActionStatus::Held);
    assert!(owed_news(&action(&sim, &id4)));
    sim.apply(Event::DoorDelivered { id: id4.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id4)));
    sim.operator(json!({"action.drop": {"action": id4}})).unwrap();
    assert!(owed_news(&action(&sim, &id4)));
    sim.apply(Event::DoorDelivered { id: id4.clone(), answered: false });
    assert!(!owed_news(&action(&sim, &id4)));
}
