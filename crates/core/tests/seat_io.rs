// fac-654: a seat's `$MORK_IO` is named by its first turn and kept for its run, whatever turn follows; a reply's
// images, and a say's, are the copies the engine published for the sender's running job.

// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{HOME, Sim, reply};
use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_types::state::{Owed, Sender};
use mork_types::{AgentKey, JobId};
use serde_json::{Value, json};

const CARD: &str = "web-1";
const FOREMAN: &str = "web-1/foreman";

fn io(s: &Sim, key: &str) -> Option<String> {
    s.state.agents.get(&AgentKey::new(key)).and_then(|a| a.io.clone())
}

// The seat's jobs, the earliest first.
fn turns(s: &Sim, key: &str) -> Vec<JobId> {
    let mut jobs: Vec<_> = s.state.jobs.values().filter(|j| j.owner.as_str() == key).collect();
    jobs.sort_by_key(|j| j.spec.created_ms);
    jobs.iter().map(|j| j.id.clone()).collect()
}

// A card in build, its foreman's folder named by its first turn, at its brief.
fn building() -> (Sim, String) {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    let first = turns(&s, FOREMAN)[0].as_str().to_owned();
    assert_eq!(io(&s, FOREMAN).as_deref(), Some(first.as_str()), "the foreman's first turn named its folder");
    (s, first)
}

// The operator's message to the foreman, waking it.
fn wake(s: &mut Sim) {
    s.operator(json!({"say": {"place": CARD, "to": "foreman", "text": "Go on."}})).unwrap();
}

fn ends(s: &mut Sim, id: &JobId, reply: Value) {
    let session = SessionSeen { started: true, ..SessionSeen::default() };
    s.apply(Event::JobEnded { id: id.clone(), end: JobEnd::Finished { reply: Ok(reply), session } });
}

fn with_images(images: &[String]) -> Value {
    let mut r = reply("working", "Attached");
    r["hand"]["images"] = json!(images);
    r
}

fn say(image: String) -> Value {
    json!({"say": {"place": CARD, "to": "planner", "kind": "update", "text": "Look.", "images": [image]}})
}

fn correction_owed(s: &Sim, key: &str) -> Option<String> {
    match s.state.agents.get(&AgentKey::new(key)).and_then(|a| a.owed.clone()) {
        Some(Owed::Correction { reason, .. }) => Some(reason),
        _ => None,
    }
}

#[test]
fn a_seats_folder_is_kept_through_its_corrections_retries_and_fresh_sessions() {
    let (mut s, first) = building();
    wake(&mut s);
    let id = s.turn_started(FOREMAN);
    ends(&mut s, &id, with_images(&[format!("{HOME}/io/{first}/shot.png")]));
    let why = correction_owed(&s, FOREMAN).expect("an image the engine didn't publish is refused");
    assert!(why.contains("isn't a regular file of your io/ folder, $MORK_IO"), "{why}");
    s.turn(FOREMAN, reply("working", "Corrected"));
    wake(&mut s);
    let id = s.turn_started(FOREMAN);
    s.apply(Event::JobEnded { id, end: JobEnd::Cut { session: SessionSeen::default() } });
    s.turn(FOREMAN, reply("working", "Retried"));
    wake(&mut s);
    let id = s.turn_started(FOREMAN);
    s.apply(Event::JobEnded { id, end: JobEnd::Failed { words: "it broke".into() } });
    wake(&mut s);
    s.turn(FOREMAN, reply("working", "Back"));
    s.operator(json!({"fresh": {"agent": FOREMAN}})).unwrap();
    wake(&mut s);
    s.turn(FOREMAN, reply("working", "Fresh"));
    assert_eq!(turns(&s, FOREMAN).len(), 8, "its brief's turn, and seven since");
    assert_eq!(io(&s, FOREMAN).as_deref(), Some(first.as_str()), "every turn since kept its first turn's folder");
}

#[test]
fn a_new_run_names_a_new_folder() {
    let (mut s, first) = building();
    s.operator(json!({"close": {"card": CARD, "why": "Again."}})).unwrap();
    s.settle_actions();
    for id in s.open_jobs(|_| true) {
        s.end_job(&id, json!({}));
    }
    s.settle_actions();
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    assert_eq!(io(&s, FOREMAN), None, "a new run's foreman has no folder before its first turn");
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, mork_types::work::Purpose::Clone { .. }));
    s.end_job(&clone[0], json!({}));
    s.settle_actions();
    wake(&mut s);
    let named = s.turn_started(FOREMAN);
    assert_ne!(named.as_str(), first, "the new run's turn is a new job");
    assert_eq!(io(&s, FOREMAN).as_deref(), Some(named.as_str()), "its first turn of the new run named its folder");
}

#[test]
fn a_reply_names_its_own_jobs_published_copies_alone() {
    let (mut s, first) = building();
    let own = |id: &JobId, rest: &str| format!("{HOME}/jobs/{id}/published/{rest}", id = id.as_str());
    let refused: [fn(&JobId, &str) -> String; 5] = [
        |_, seat| format!("{HOME}/io/{seat}/shot.png"),
        |_, seat| format!("{HOME}/jobs/{seat}/published/1-shot.png"),
        |id, _| format!("{HOME}/jobs/{}/published/sub/1-shot.png", id.as_str()),
        |id, _| format!("{HOME}/jobs/{}/published/..", id.as_str()),
        |id, _| format!("{HOME}/jobs/{}/io/shot.png", id.as_str()),
    ];
    for path in refused {
        wake(&mut s);
        let id = s.turn_started(FOREMAN);
        let image = path(&id, &first);
        ends(&mut s, &id, with_images(std::slice::from_ref(&image)));
        assert!(correction_owed(&s, FOREMAN).is_some(), "{image} was taken");
        s.turn(FOREMAN, reply("working", "Corrected"));
    }
    wake(&mut s);
    let id = s.turn_started(FOREMAN);
    let mut r = with_images(&[own(&id, "1-shot.png")]);
    r["messages"] = json!([{"to": "planner", "kind": "update", "text": "Look.", "images": [own(&id, "2-two.png")]}]);
    ends(&mut s, &id, r);
    assert_eq!(correction_owed(&s, FOREMAN), None, "its own published copies are taken");
    let sent = s.state.messages.iter().find(|m| m.images == [own(&id, "2-two.png")]);
    assert!(sent.is_some(), "the message carries its image: {:?}", s.state.messages.last());
}

#[test]
fn a_message_names_its_senders_published_copies_alone() {
    let (mut s, first) = building();
    wake(&mut s);
    let id = s.turn_started(FOREMAN);
    let from = Sender::Agent(AgentKey::new(FOREMAN));
    let own = |rest: &str| format!("{HOME}/jobs/{}/published/{rest}", id.as_str());
    let refused = [
        format!("{HOME}/io/{first}/images/a.png"),
        format!("{HOME}/io/other/a.png"),
        format!("{HOME}/jobs/{}/io/a.png", id.as_str()),
        format!("{HOME}/jobs/20200101-other/published/say-1-a.png"),
        own("sub/say-1-a.png"),
        own(".."),
        own(""),
    ];
    for image in refused {
        let e = s.cmd(from.clone(), say(image.clone())).expect_err(&image);
        assert!(e.message.contains("isn't in your io/ folder, $MORK_IO"), "{image}: {}", e.message);
    }
    s.cmd(from, say(own("say-1-a.png"))).unwrap();
    assert!(s.state.messages.iter().any(|m| m.images == [own("say-1-a.png")]), "the message carries its copy");
    ends(&mut s, &id, reply("working", "Said"));
}
