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
    let view = mork_core::views::conversation::messages(&s.state, CARD, Some(0));
    let pinned = view.pinned.iter().find(|m| m.open_ask.as_ref() == Some(&ask.id)).expect("pinned question");
    assert_eq!(pinned.seq, hand.seq);
    assert_eq!(hand.message_place.as_str(), CARD);
    assert_eq!(pinned.text, choices.ask);
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
    assert_eq!(ask.seq, Some(posted.seq));
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

fn room(s: &Sim, id: &str) -> mork_types::view::PlaceView {
    mork_core::views::place::place(&s.state, id, Some(0), &Outside::default(), s.ctx())
}

#[test]
fn room_controls_parent_and_standing_decisions_follow_the_card() {
    let mut s = start_no_planner();
    s.config.projects.get_mut("web").unwrap().preview = Some("serve".into());
    let card = s.state.cards.get_mut(CARD).unwrap();
    card.preview = true;
    let port = card.port.unwrap();
    s.state.board.get_mut(CARD).unwrap().description = "operator 01.01: Decision: Keep both rooms.\n\nWords.".into();
    let mut child = s.state.agents[FOREMAN].clone();
    child.key = "web-1/researcher-1".into();
    child.role = mork_types::reply::Role::Researcher;
    child.place = "web-1/researcher-1/dm".into();
    child.parent = Some(FOREMAN.into());
    s.state.agents.insert(child.key.clone(), child);
    let view = room(&s, CARD);
    let row = view.sessions.iter().find(|a| a.agent == "web-1/researcher-1").unwrap();
    assert_eq!(row.parent.as_ref().unwrap().as_str(), FOREMAN);
    assert_eq!(view.decisions, ["Keep both rooms."]);
    let about = view.card.unwrap().about;
    assert_eq!(about.actions, ["close"]);
    let preview = about.preview.unwrap();
    assert!(preview.on && preview.enabled);
    assert_eq!(preview.url, format!("http://web-1.localhost:{port}/"));
}

fn board_with_two_cards() -> Sim {
    let mut s = Sim::new();
    s.board("web-2", "web");
    let carried = s.state.board["web-2"].clone();
    s.board(CARD, "web");
    s.state.board.insert(carried.id.clone(), carried);
    s
}

#[test]
fn first_run_and_restart_offer_only_the_modes_and_defaults_the_engine_applies() {
    use mork_core::decide::Event;
    use mork_core::state::unloaded::{Records, wanted};
    use mork_types::card::{Card, Carried, Mode};
    let mut s = board_with_two_cards();
    s.operator(json!({"auto":{"project":"web"}})).unwrap();
    let fresh = room(&s, CARD);
    assert!(fresh.card.is_none());
    let start = fresh.start.unwrap();
    assert_eq!((start.level, start.from.as_str()), (1, "project"));
    assert!(start.modes.full && start.modes.report);
    assert!(screen(&s.state, &Outside::default(), s.ctx()).boards.iter().any(|b| b.project == "web" && b.auto));

    let id = mork_types::CardId::new(CARD);
    let mut prior = Card::new(id.clone(), "web", "Earlier run", Mode::Full, 1, s.now);
    prior.stage = Stage::Closed;
    prior.carries.push(Carried { id: "web-2".into(), ..Default::default() });
    s.state.cards.insert(id.clone(), prior.clone());
    let start = room(&s, CARD).start.unwrap();
    assert_eq!((start.level, start.from.as_str()), (1, "project"));
    assert_eq!(start.carries, [mork_types::CardId::new("web-2")]);
    assert!(start.modes.full && start.modes.report);

    s.state.cards.remove(&id);
    prior.head = Some("commit".into());
    prior.delegated = true;
    prior.self_sustained = true;
    prior.visual = true;
    s.state.unloaded.cards.insert(id.clone());
    let cmd = serde_json::from_value(json!({"watch":{"places":["screen",CARD],"full":true}})).unwrap();
    assert!(wanted(&s.state, &Event::Command { from: Sender::Operator, cmd }).cards.contains(&id));
    s.state.absorb(&id, Records { card: Some(prior), ..Default::default() });
    assert!(room(&s, "web-2").start.is_none());
    let start = room(&s, CARD).start.unwrap();
    assert_eq!((start.level, start.from.as_str()), (2, "run"));
    assert!(start.visual && start.modes.full && !start.modes.report);
    assert_eq!(start.carries, [mork_types::CardId::new("web-2")]);
}

#[test]
fn report_restart_is_available_when_a_formerly_carried_card_is_running() {
    let mut s = board_with_two_cards();
    s.operator(json!({"start": {"card": CARD, "carries": ["web-2"]}})).unwrap();
    s.operator(json!({"close": {"card": CARD, "why": "Restart separately"}})).unwrap();
    for id in s.open_jobs(|p| matches!(p, Purpose::Bundle { .. })) {
        s.end_job(&id, json!({"branch": CARD, "bundle": false, "head": null, "clean": true, "dirty": []}));
    }
    s.settle_actions();
    s.operator(json!({"start": {"card": "web-2"}})).unwrap();

    let start = room(&s, CARD).start.expect("the report start is available");
    assert!(!start.modes.full && start.modes.report);
    s.operator(json!({"start": {"card": CARD, "mode": "report"}})).unwrap();
    assert!(s.card(CARD).carries.is_empty());
}
