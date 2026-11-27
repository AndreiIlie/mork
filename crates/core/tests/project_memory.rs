// why: missing fixture data or a refused setup command fails the test.
#![allow(clippy::unwrap_used)]

mod common;

use common::{Sim, reply};
use mork_core::agents::turns;
use mork_core::views::search::{Found, Search};
use mork_types::action::ActionWhat;
use mork_types::card::Stage;
use mork_types::state::{Retiring, Sender};
use serde_json::{Value, json};

const CARD: &str = "web-1";
const PLANNER: &str = "web-1/planner";

fn seat(key: &str) -> Sender {
    Sender::Agent(key.into())
}

fn write(memory: &str, name: &str, description: &str, text: &str) -> Value {
    json!({"memory.write": {"memory": memory, "name": name, "description": description, "text": text}})
}

fn delete(name: &str) -> Value {
    json!({"memory.delete": {"memory": "web", "name": name}})
}

fn closing(s: &mut Sim, key: &str) {
    s.state.agents.get_mut(key).unwrap().retiring =
        Some(Retiring { asked: true, live: true, remove: false, resumed: false });
}

fn close(s: &mut Sim, id: &str) {
    s.operator(json!({"close": {"card": id, "why": "Finished"}})).unwrap();
    assert_eq!(s.stage(id), Stage::Closed);
}

fn names(s: &mut Sim, from: Sender, words: &str) -> Vec<String> {
    let result = s.cmd(from, json!({"memory.search": {"words": words}})).unwrap();
    result["memory.search"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect()
}

fn rows(s: &Sim, kind: &str, card: Option<&str>) -> Vec<Value> {
    s.journal.iter().filter(|(_, k, c, _)| k == kind && c.as_deref() == card).map(|(_, _, _, d)| d.clone()).collect()
}

#[test]
fn bm25_uses_whole_words_field_weights_and_rarity_within_scope() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    for (name, desc, body) in [
        ("door", "common", "one"),
        ("desc", "door", "common"),
        ("body", "plain", "door"),
        ("doorway", "common", "rare"),
    ] {
        s.operator(write("web", name, desc, body)).unwrap();
    }
    s.operator(write("ex", "hidden", "door", "rare")).unwrap();
    assert_eq!(names(&mut s, seat(PLANNER), "DOOR-door"), ["door", "desc", "body"]);
    s.operator(write("web", "rarely", "plain", "scarce")).unwrap();
    s.operator(write("web", "often", "plain", "common")).unwrap();
    assert_eq!(names(&mut s, seat(PLANNER), "scarce common")[0], "rarely");
    assert!(names(&mut s, Sender::Operator, "door").contains(&"hidden".to_owned()));
    s.config.projects.insert("ex-a".into(), s.config.projects["ex"].clone());
    for (memory, name) in [("ex-a", "alpha"), ("ex", "omega"), ("ex", "beta")] {
        s.operator(write(memory, name, "Equal", "weights")).unwrap();
    }
    assert_eq!(names(&mut s, Sender::Operator, "equal"), ["beta", "omega", "alpha"]);
}

#[test]
fn closing_permissions_and_journaled_changes_after_card_end() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    let initial = write("web", "lesson", "First", "One");
    assert!(s.cmd(seat(PLANNER), initial.clone()).is_err());
    closing(&mut s, PLANNER);
    for memory in ["general", "ex"] {
        assert!(s.cmd(seat(PLANNER), write(memory, "lesson", "First", "One")).is_err());
        assert!(s.cmd(seat(PLANNER), json!({"memory.delete":{"memory":memory,"name":"lesson"}})).is_err());
    }
    s.state.agents.get_mut(PLANNER).unwrap().retiring.as_mut().unwrap().resumed = true;
    assert!(s.cmd(seat(PLANNER), initial.clone()).is_err());
    closing(&mut s, PLANNER);
    close(&mut s, CARD);
    let answer = s.cmd(seat(PLANNER), initial).unwrap();
    assert_eq!(answer["memory.write"], json!({"memory":"web","name":"lesson","rewrote":false}));
    let rewrite = write("web", "lesson", "Second", "Two");
    s.cmd(Sender::Orchestrator("web".into()), rewrite).unwrap();
    s.operator(delete("lesson")).unwrap();
    let first = json!({"description":"First","text":"One"});
    let second = json!({"description":"Second","text":"Two"});
    let create = &rows(&s, "memory.write", Some(CARD))[0];
    assert_eq!(create, &json!({"memory":"web","name":"lesson","by":PLANNER,"before":null,"after":first}));
    let change = &rows(&s, "memory.write", None)[0];
    assert_eq!((&change["before"], &change["after"], &change["by"]), (&first, &second, &json!("orchestrator:web")));
    let remove = &rows(&s, "memory.delete", None)[0];
    assert_eq!(
        remove,
        &json!({"memory":"web","name":"lesson","by":"operator","why":"deleted","before":second,"after":null})
    );
}

fn assert_held_history(s: &Sim) {
    let mut search = Search::new("withheld", Default::default(), &[]);
    search.offer(Found::Card(Box::new(s.card(CARD))));
    for (time, kind, c, data) in &s.journal {
        if c.as_deref() == Some(CARD) {
            assert!(!data.to_string().to_lowercase().contains("withheld"));
            search.offer(Found::Event { card: CARD.into(), time: *time, kind: kind.clone(), data: data.clone() });
        }
    }
    assert_eq!(search.answer()["history.search"]["hits"], json!([]));
    assert_eq!(rows(s, "memory.staged", Some(CARD)), [json!({"by":PLANNER,"op":"write"})]);
}

#[test]
fn committed_index_and_blind_reviewer_reads_exclude_held_lessons_until_landing() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    for (memory, name) in [("general", "shared"), ("web", "zulu"), ("web", "alpha"), ("ex", "elsewhere")] {
        s.operator(write(memory, name, "Visible description", "Body")).unwrap();
    }
    closing(&mut s, PLANNER);
    let edit = write("web", "withheld", "Withheld detail", "Withheld body");
    assert_eq!(s.cmd(seat(PLANNER), edit).unwrap()["memory.write"]["staged"], true);
    s.built(CARD, "head", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.suite(CARD, 0);
    s.round_asked(CARD);
    let review = s.review_card(CARD);
    let foreman = format!("{review}/foreman");
    let reviewer = format!("{review}/reviewer-1");
    s.turn_sending(
        &foreman,
        &[json!({"spawn":{"card":review,"role":"reviewer","topic":"tests","task":"Review tests"}})],
        reply("working", "Review"),
    );
    let prompt = turns::plan(&s.state, &reviewer.as_str().into(), s.ctx(), false).unwrap().prompt;
    assert!(prompt.contains(
        "general/shared: Visible description\nweb/alpha: Visible description\nweb/zulu: Visible description\n"
    ));
    assert!(!prompt.contains("ex/elsewhere") && !prompt.contains("withheld"));
    assert!(names(&mut s, seat(&reviewer), "withheld").is_empty());
    assert!(s.cmd(seat(&reviewer), json!({"memory.read":{"memory":"web","name":"withheld"}})).is_err());
    for cmd in [json!({"memory.stats":{"memory":"web"}}), json!({"card.show":{"card":CARD}})] {
        assert!(!s.cmd(seat(&reviewer), cmd).unwrap().to_string().contains("withheld"));
    }
    assert_held_history(&s);
    s.turn(&reviewer, reply("review-pass", "Passed"));
    s.round_ends(CARD, &[], &[]);
    let moved = s.action_of(|w| matches!(w, ActionWhat::MoveBase { card, .. } if card.as_str() == CARD));
    s.returned(&moved.id, Ok(json!({})));
    assert_eq!(s.stage(CARD), Stage::Done);
    assert_eq!(s.state.memory_files["web/withheld"].text, "Withheld body");
    assert!(s.card(CARD).staged_memory.is_empty());
}

#[test]
fn linked_reviewer_stages_on_subject_even_after_review_card_closes() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    s.built(CARD, "head", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    let rc = s.review_card(CARD);
    let reviewer = format!("{rc}/reviewer-1");
    s.turn_sending(
        &format!("{rc}/foreman"),
        &[json!({"spawn":{"card":rc,"role":"reviewer","topic":"tests","task":"Review tests"}})],
        reply("working", "Review"),
    );
    closing(&mut s, &reviewer);
    s.cmd(seat(&reviewer), write("web", "one", "First", "Earlier")).unwrap();
    close(&mut s, &rc);
    s.cmd(seat(&reviewer), write("web", "two", "Second", "Later")).unwrap();
    assert!(s.state.memory_files.is_empty());
    assert!(s.card(&rc).staged_memory.is_empty());
    assert_eq!(s.card(CARD).staged_memory.len(), 2);
    close(&mut s, CARD);
    assert_eq!(rows(&s, "memory.write", Some(&rc)).len(), 2);
    assert_eq!(s.state.memory_files["web/two"].writer, reviewer);
}

#[test]
fn held_create_rewrite_delete_share_validation_diffs_and_counts() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    closing(&mut s, PLANNER);
    let name = "pending";
    s.cmd(seat(PLANNER), write("web", name, "First", "One")).unwrap();
    assert_eq!(s.cmd(seat(PLANNER), write("web", name, "Next", "Two")).unwrap()["memory.write"]["rewrote"], true);
    assert_eq!(s.cmd(seat(PLANNER), delete(name)).unwrap()["memory.delete"]["staged"], true);
    assert!(s.cmd(seat(PLANNER), delete(name)).is_err());
    close(&mut s, CARD);
    assert!(!s.state.memory_files.contains_key("web/pending"));
    let writes = rows(&s, "memory.write", Some(CARD));
    let deletes = rows(&s, "memory.delete", Some(CARD));
    assert_eq!(writes[0]["before"], Value::Null);
    assert_eq!(writes[0]["after"], json!({"description":"First","text":"One"}));
    assert_eq!(writes[1]["before"], writes[0]["after"]);
    assert_eq!(writes[1]["after"], json!({"description":"Next","text":"Two"}));
    assert_eq!(deletes[0]["before"], writes[1]["after"]);
    assert_eq!(deletes[0].get("after"), Some(&Value::Null));
    let tally = &s.state.memory_tallies["web"];
    assert_eq!((tally.written, tally.deleted), (2, 1));
}

#[test]
fn conflicts_survive_deletion_and_ignore_stale_edits_in_validation() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    closing(&mut s, PLANNER);
    s.operator(write("web", "old", "First", "One")).unwrap();
    s.cmd(seat(PLANNER), delete("old")).unwrap();
    s.operator(write("web", "old", "New", "Two")).unwrap();
    s.cmd(seat(PLANNER), delete("old")).unwrap();
    s.cmd(seat(PLANNER), write("web", "absent", "Held", "Later")).unwrap();
    s.operator(write("web", "absent", "External", "Interim")).unwrap();
    s.operator(delete("absent")).unwrap();
    close(&mut s, CARD);
    let skips = rows(&s, "memory.skipped", Some(CARD));
    assert_eq!(skips.len(), 2);
    assert_eq!(skips[0]["name"], "old");
    assert_eq!(skips[1]["after"], json!({"description":"Held","text":"Later"}));
    assert_eq!(rows(&s, "memory.delete", Some(CARD))[0]["before"]["text"], "Two");
    assert!(s.state.memory_files.is_empty());
}

#[test]
fn first_cards_commit_marks_the_other_cards_pending_edit() {
    let mut s = Sim::new();
    for card in [CARD, "web-2"] {
        s.through_brief(card, "web");
        let key = format!("{card}/planner");
        closing(&mut s, &key);
        s.cmd(seat(&key), write("web", "shared", "From card", card)).unwrap();
    }
    close(&mut s, CARD);
    close(&mut s, "web-2");
    assert_eq!(s.state.memory_files["web/shared"].text, CARD);
    assert_eq!(rows(&s, "memory.skipped", Some("web-2"))[0]["after"]["text"], "web-2");
}
