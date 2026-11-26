// fac-696: an ask's options and pick, and a recap's sketch, travel typed from the hand to the views.

mod common;

use common::{Sim, reply};
use mork_core::views::outside::Outside;
use mork_core::views::screen::screen;
use mork_types::ask::{Ask, AskKind, Choices};
use mork_types::card::Stage;
use mork_types::state::Sender;
use mork_types::work::Purpose;
use serde_json::{Value, json};

const CARD: &str = "web-1";
const FOREMAN: &str = "web-1/foreman";

// why: test helper: a refused start or missing clone job is a test failure.
#[allow(clippy::unwrap_used)]
fn start_no_planner() -> Sim {
    let mut s = Sim::new();
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD, "no-planner": true}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Brief);
    s
}

// why: test helper: a missing expected ask is a test failure.
#[allow(clippy::unwrap_used)]
fn open_ask(s: &Sim, kind: AskKind) -> Ask {
    s.state.asks.values().find(|a| a.open && a.kind == kind).cloned().unwrap()
}

fn asking(pick: u32) -> Value {
    let mut r = reply("working", "Which one?");
    r["hand"]["message"] = json!("The list shows the total twice.\nRemove the one above?");
    r["hand"]["ask"] = json!(true);
    r["hand"]["options"] = json!(["Yes: keep the one below.", "No: keep both."]);
    r["hand"]["pick"] = json!(pick);
    r["hand"]["reason"] = json!("the one below sits next to its rows.");
    r
}

#[test]
fn an_asks_options_and_pick_are_kept_typed_and_composed_for_readers_of_words() {
    let mut s = start_no_planner();
    s.turn(FOREMAN, asking(1));
    let ask = open_ask(&s, AskKind::Answer);
    let choices = Choices {
        ask: "The list shows the total twice.\nRemove the one above?".into(),
        options: vec!["Yes: keep the one below.".into(), "No: keep both.".into()],
        pick: Some(1),
        reason: "the one below sits next to its rows.".into(),
    };
    assert_eq!(ask.choices.as_ref(), Some(&choices));
    assert_eq!(
        ask.text,
        "The list shows the total twice.\nRemove the one above?\n\n1. Yes: keep the one below.\n2. No: keep both.\n\nI'd pick 1: the one below sits next to its rows."
    );
    let sc = screen(&s.state, &Outside::default(), s.ctx());
    let hand = sc.hands.iter().find(|h| h.ask == ask.id).expect("the ask's hand");
    assert_eq!(hand.choices.as_ref(), Some(&choices));
    assert_eq!(hand.words, ask.text);
}

#[test]
fn a_pick_naming_no_option_is_dropped() {
    let mut s = start_no_planner();
    s.turn(FOREMAN, asking(3));
    let choices = open_ask(&s, AskKind::Answer).choices.expect("its choices");
    assert_eq!((choices.pick, choices.reason.as_str()), (None, ""));
}

#[test]
fn a_recaps_typed_sketch_heads_its_plan_and_shows_on_its_approve_hand() {
    let mut s = start_no_planner();
    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("The long plan.");
    recap["hand"]["changes"] = json!(["where: the bills list", "removed: the total above"]);
    recap["hand"]["code_shape"] = json!(["views: drop the upper total"]);
    s.turn(FOREMAN, recap);
    let ask = open_ask(&s, AskKind::ApproveBrief);
    let sketch = "What changes\n- where: the bills list\n- removed: the total above\n\nCode shape\n- views: drop the upper total";
    assert_eq!(ask.text, format!("approve the plan\n\n{sketch}"));
    assert_eq!(s.card(CARD).draft.as_deref(), Some(format!("{sketch}\n\nThe long plan.").as_str()));
    let posted = s.messages(CARD).into_iter().rfind(|m| m.hand.is_some()).expect("the recap's hand");
    assert_eq!(posted.text, format!("{sketch}\n\nThe long plan."), "the room shows the sketch with the plan");
    let sc = screen(&s.state, &Outside::default(), s.ctx());
    let brief = sc.hands.iter().find(|h| h.ask == ask.id).and_then(|h| h.brief.clone()).expect("its sections");
    assert_eq!(brief.changes, ["where: the bills list", "removed: the total above"]);
    assert_eq!(brief.code_shape, ["views: drop the upper total"]);
    assert_eq!(brief.mock, None);
}

#[test]
fn an_answers_choice_is_kept_and_one_the_ask_lacks_is_refused() {
    let mut s = start_no_planner();
    s.turn(FOREMAN, asking(1));
    let refused = s.operator(json!({"say": {"place": CARD, "text": "3", "choice": 3}}));
    assert!(refused.is_err(), "option 3 isn't offered: {refused:?}");
    assert!(open_ask(&s, AskKind::Answer).open);
    s.operator(json!({"say": {"place": CARD, "text": "2", "choice": 2}})).expect("option 2 is offered");
    let answer = s.messages(CARD).into_iter().rfind(|m| m.text == "2").expect("the answer");
    assert_eq!(answer.choice, Some(2));
    assert!(!s.state.asks.values().any(|a| a.open && a.kind == AskKind::Answer));
}

#[test]
fn an_orchestrators_choice_needs_the_ask_given_to_it() {
    let mut s = start_no_planner();
    s.turn(FOREMAN, asking(1));
    let said = json!({"say": {"place": CARD, "text": "1", "choice": 1}});
    let refused = s.cmd(Sender::Orchestrator("web".into()), said);
    assert!(refused.is_err(), "the ask isn't given to the orchestrator: {refused:?}");
    assert!(s.messages(CARD).iter().all(|m| m.choice.is_none()), "no choice is kept");
}
