// fac-688: visual mode on `--no-planner` cards: baseline step, brief mock, reopen, toggles, and prior-run inheritance.

mod common;

use common::{Sim, reply};
use factory_core::agents::reply::outcomes;
use factory_core::agents::turns;
use factory_core::decide::{Event, JobEnd, SessionSeen};
use factory_core::lifecycle::{baseline, mock};
use factory_types::ask::AskKind;
use factory_types::card::{MockStep, Stage};
use factory_types::protocol::ErrorCode;
use factory_types::reply::Outcome;
use factory_types::state::{MessageKind, Owed, Sender};
use factory_types::work::Purpose;
use factory_types::{AgentKey, JobId};
use serde_json::{Value, json};

const CARD: &str = "web-1";

fn finish_turn(s: &mut Sim, id: &JobId, reply: Value) {
    let session = SessionSeen { started: true, ..SessionSeen::default() };
    s.apply(Event::JobEnded { id: id.clone(), end: JobEnd::Finished { reply: Ok(reply), session } });
}

// why: test helper: a refused start or missing clone job is a test failure.
#[allow(clippy::unwrap_used)]
fn start_brief(s: &mut Sim, extra: Value) {
    s.config.factory.vite = Some("vite".into());
    s.board(CARD, "web");
    let mut start = json!({"card": CARD});
    if let (Some(dst), Some(src)) = (start.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    s.andrei(json!({"start": start})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    assert_eq!(s.stage(CARD), Stage::Brief);
}

fn recap_reply() -> Value {
    let mut r = reply("brief-ready", "The plan");
    r["hand"]["message"] = json!("- where: src\n---\nThe plan.");
    r
}

fn mock_offer_reply() -> Value {
    let mut r = reply("brief-ready", "Mock ready");
    r["hand"]["message"] = json!("Mock ready at http://web-1.localhost:41001/");
    r
}

fn request_headlines(s: &Sim) -> Vec<String> {
    s.engine_to(CARD, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).map(|m| m.headline).collect()
}

// why: test helper: a missing expected ask is a test failure.
#[allow(clippy::unwrap_used)]
fn open_ask(s: &Sim, kind: AskKind) -> &factory_types::ask::Ask {
    s.state.asks.values().find(|a| a.open && a.kind == kind).unwrap()
}

fn spawn_baseline(s: &mut Sim, foreman: &str) {
    let baseline_turn = s.turn_started(foreman);
    assert!(s.state.jobs[&baseline_turn].stdin.contains("## The baseline"));
    assert!(
        s.cmd(
            Sender::Agent(foreman.to_owned().into()),
            json!({"spawn": {"card": CARD, "role": "baseline", "task": "Reproduce the page"}}),
        )
        .is_ok()
    );
    let base_seat = format!("{CARD}/baseline-1");
    assert!(s.owed(&base_seat), "baseline seat must run while foreman is mid-turn during Baseline");
    let base_job = s.turn_started(&base_seat);
    finish_turn(s, &baseline_turn, reply("working", "Spawned baseline"));
    finish_turn(s, &base_job, reply("built", "Baseline reproduced"));
}

fn assert_visual_plan_stays_andreis(s: &mut Sim, foreman: &str) {
    let ask = open_ask(s, AskKind::ApproveBrief);
    assert_eq!(ask.place.as_str(), CARD);
    assert!(ask.text.contains("mock:"), "approve-brief ask must include mock link: {}", ask.text);
    assert!(!ask.given, "visual no-planner plan ask must never be auto-given");
    let refused = s.cmd(Sender::Orchestrator("web".into()), json!({"approve": {"card": CARD}}));
    assert!(matches!(refused, Err(e) if e.code == ErrorCode::Buzz));
    assert!(s.andrei(json!({"approve": {"card": CARD}})).is_ok());
    assert_eq!(s.stage(CARD), Stage::Build);
    assert_eq!(s.card(CARD).mock.map(|m| m.step), Some(MockStep::Approved));
    assert!(s.owed(foreman));
}

#[test]
fn visual_no_planner_runs_baseline_then_brief_and_andreis_approve() {
    let mut s = Sim::new();
    let foreman = format!("{CARD}/foreman");
    start_brief(&mut s, json!({"visual": true, "no-planner": true, "delegated": true}));

    assert!(!s.state.agents.contains_key(format!("{CARD}/planner").as_str()));
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Baseline);
    assert_eq!(request_headlines(&s), vec![baseline::HEADLINE]);
    assert!(!outcomes(&s.state, &s.state.agents[foreman.as_str()]).contains(&Outcome::BriefReady));

    spawn_baseline(&mut s, &foreman);
    s.turn(&foreman, reply("built", "Baseline done"));
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Open);
    assert!(!s.state.agents.contains_key(format!("{CARD}/baseline-1").as_str()));
    assert_eq!(request_headlines(&s), vec![baseline::HEADLINE, "Agree the specification"]);
    assert!(outcomes(&s.state, &s.state.agents[foreman.as_str()]).contains(&Outcome::BriefReady));

    let brief_turn = s.turn_started(&foreman);
    let brief_prompt = &s.state.jobs[&brief_turn].stdin;
    assert!(brief_prompt.contains("## The mock"));
    assert!(brief_prompt.contains("Edit no tracked repository files before approval"));
    assert!(!brief_prompt.contains("Write, edit and commit nothing before approval"));
    finish_turn(&mut s, &brief_turn, recap_reply());
    assert_visual_plan_stays_andreis(&mut s, &foreman);
}

fn approve_visual_no_planner(s: &mut Sim, foreman: &str) {
    start_brief(s, json!({"visual": true, "no-planner": true}));
    s.turn(foreman, reply("built", "No existing page"));
    s.turn(foreman, recap_reply());
    assert!(s.andrei(json!({"approve": {"card": CARD}})).is_ok());
}

fn assert_offer_retracted_and_approved(s: &mut Sim, foreman: &str) {
    let ask = open_ask(s, AskKind::Mock);
    assert_eq!(ask.place.as_str(), CARD);
    assert!(ask.text.contains("comment in its room"));
    assert!(s.card(CARD).mock.is_some_and(|m| m.offered));

    assert!(s.andrei(json!({"say": {"place": CARD, "text": "Tighten the header spacing"}})).is_ok());
    let next = s.turn_started(foreman);
    assert!(s.card(CARD).mock.is_some_and(|m| !m.offered));
    let refused = s.andrei(json!({"approve": {"card": CARD}}));
    assert!(matches!(refused, Err(e) if e.message.contains("its foreman offers it")));

    finish_turn(s, &next, mock_offer_reply());
    assert!(s.andrei(json!({"approve": {"card": CARD}})).is_ok());
    assert_eq!(s.card(CARD).mock.map(|m| m.step), Some(MockStep::Approved));
    assert!(s.engine_to(CARD, "foreman").iter().any(|m| m.headline == "Continue the build"));
}

#[test]
fn reopened_mock_on_no_planner_card_uses_foreman_in_card_room() {
    let mut s = Sim::new();
    let foreman = format!("{CARD}/foreman");
    approve_visual_no_planner(&mut s, &foreman);

    let pre_reopen = s.turn_started(&foreman);
    s.andrei(json!({"mock": {"card": CARD}})).unwrap();
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Open);
    finish_turn(&mut s, &pre_reopen, reply("built", "Finished old build turn"));
    assert!(!s.card(CARD).gates.built_pending);
    assert!(s.engine_to(CARD, "foreman").iter().any(|m| m.text.contains("The mock is still open")));

    assert!(s.owed(&foreman), "no-planner foreman must not be held by mock::holds");
    assert!(outcomes(&s.state, &s.state.agents[foreman.as_str()]).contains(&Outcome::BriefReady));
    let first = s.turn_started(&foreman);
    assert!(s.state.jobs[&first].stdin.contains("Edit no tracked repository files"));
    finish_turn(&mut s, &first, reply("built", "Tried to submit while mock open"));
    assert!(matches!(s.state.agents[foreman.as_str()].owed, Some(Owed::Correction { .. })));

    s.turn(&foreman, mock_offer_reply());
    assert_offer_retracted_and_approved(&mut s, &foreman);
}

#[test]
fn reopened_mock_refuses_empty_offer_and_holds_foreman_dm() {
    let mut s = Sim::new();
    let foreman = format!("{CARD}/foreman");
    approve_visual_no_planner(&mut s, &foreman);

    let dm = format!("{CARD}/foreman/dm");
    s.andrei(json!({"say": {"place": dm, "text": "Check the DM"}})).unwrap();
    let dm_job = s.turn_started(&dm);
    s.andrei(json!({"mock": {"card": CARD}})).unwrap();
    assert!(mock::holds(&s.state, &s.state.agents[dm.as_str()]));
    let dm_plan = turns::plan(&s.state, &AgentKey::new(&dm), s.ctx(), false);
    assert!(dm_plan.is_none());

    let mut empty = reply("brief-ready", "Empty offer");
    empty["hand"]["message"] = json!("   ");
    s.turn(&foreman, empty);
    assert!(matches!(s.state.agents[foreman.as_str()].owed, Some(Owed::Correction { .. })));
    assert!(!s.card(CARD).mock.unwrap().offered);

    finish_turn(&mut s, &dm_job, reply("working", "Answered DM"));
    s.andrei(json!({"say": {"place": dm, "text": "Follow up in DM"}})).unwrap();
    assert!(!s.owed(&dm), "foreman DM seat must wait while the reopened mock is open");
    let dm_plan = turns::plan(&s.state, &AgentKey::new(&dm), s.ctx(), false).unwrap();
    assert!(dm_plan.prompt.contains("The card's room foreman makes and offers the mock"));
    assert!(!dm_plan.prompt.contains("Offer the mock with its link"));
}

#[test]
fn visual_toggles_and_prior_inheritance_work_on_no_planner_cards() {
    let mut s = Sim::new();
    let foreman = format!("{CARD}/foreman");
    start_brief(&mut s, json!({"no-planner": true}));
    let in_flight = s.turn_started(&foreman);

    s.andrei(json!({"visual": {"card": CARD}})).unwrap();
    assert!(s.card(CARD).visual);
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Baseline);
    finish_turn(&mut s, &in_flight, recap_reply());
    assert!(!s.card(CARD).recap, "recap from turn before baseline must be withheld");

    s.turn(&foreman, reply("built", "Baseline done"));
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Open);
    assert!(s.engine_to(CARD, "foreman").iter().any(|m| m.headline == "The baseline is ready"));

    s.andrei(json!({"close": {"card": CARD, "why": "restart"}})).unwrap();
    for id in s.open_jobs(|p| matches!(p, Purpose::Bundle { .. })) {
        s.end_job(&id, json!({}));
    }
    s.settle_actions();
    s.board(CARD, "web");
    s.andrei(json!({"start": {"card": CARD, "no-planner": true}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == CARD))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    assert!(s.card(CARD).visual, "a later no-planner run inherits prior visual mode");
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Baseline);

    s.andrei(json!({"visual": {"card": CARD, "off": true}})).unwrap();
    assert!(!s.card(CARD).visual);
    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Skipped);
    let last = s.engine_to(CARD, "foreman").into_iter().rfind(|m| m.kind == MessageKind::Request).unwrap();
    assert_eq!(last.headline, "Agree the specification");
    assert!(last.text.contains("has no planner: you hold its specification"));
}

#[test]
fn no_planner_foreman_waits_for_unsettled_baseline_job_once_mock_opens() {
    let mut s = Sim::new();
    let foreman = format!("{CARD}/foreman");
    start_brief(&mut s, json!({"visual": true, "no-planner": true}));
    let turn = s.turn_started(&foreman);
    s.cmd(
        Sender::Agent(foreman.clone().into()),
        json!({"spawn": {"card": CARD, "role": "baseline", "task": "Reproduce the page"}}),
    )
    .unwrap();
    let base_job = s.turn_started(&format!("{CARD}/baseline-1"));
    finish_turn(&mut s, &turn, reply("built", "Ended baseline early"));

    assert_eq!(s.card(CARD).mock.unwrap().step, MockStep::Open);
    assert!(baseline::waits(&s.state, &s.state.agents[foreman.as_str()]));
    assert!(!s.owed(&foreman));

    finish_turn(&mut s, &base_job, reply("built", "Finished late"));
    assert!(!baseline::waits(&s.state, &s.state.agents[foreman.as_str()]));
    assert!(s.owed(&foreman));
}
