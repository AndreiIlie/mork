// why: an invalid fixture or refused setup command fails the test.
#![allow(clippy::unwrap_used)]

mod common;

use common::{Sim, reply};
use mork_core::agents::pools::{self, Offer};
use mork_core::agents::turns::due;
use mork_core::decide::Event;
use mork_core::lifecycle::lookback::live;
use mork_types::card::Stage;
use mork_types::config::{Cli, Effort, OPUS, SOL};
use mork_types::lookback::{Cursor, PassConfig, Read, Trigger};
use mork_types::state::{Owed, Retiring, Sender};
use mork_types::usage::{Account, Limit};
use mork_types::work::Purpose;
use serde_json::{Value, json};

const PROJECT: &str = "web";
const CARD: &str = "lookback-web-1";
const FOREMAN: &str = "lookback-web-1/foreman";
const DAY: i64 = 86_400_000;

fn set_pass(s: &mut Sim, project: &str, read: Read, config: Value) {
    let pass: PassConfig = serde_json::from_value(config).unwrap();
    s.config.projects.get_mut(project).unwrap().passes.insert(read, pass);
}

fn tick(s: &mut Sim) {
    s.apply(Event::Tick);
}

fn running(s: &Sim) -> bool {
    live(&s.state, &PROJECT.into()).is_some()
}

fn cloned(s: &mut Sim, card: &str) {
    s.settle_actions();
    s.answer_job(
        "pass clone",
        |p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == card),
        json!({"lookback": {}}),
    );
    assert_eq!(s.stage(card), Stage::Report);
}

fn note(s: &mut Sim, project: &str, kind: &str) -> String {
    s.operator(json!({"notes.add":{"project":project,"kind":kind,"text":"a lesson"}})).unwrap()["notes.add"]["id"]
        .to_string()
}

fn write(name: &str, memory: &str) -> Value {
    json!({"memory.write":{"memory":memory,"name":name,"text":"Content","description":name,"new":true}})
}

fn delete(name: &str) -> Value {
    json!({"memory.delete":{"memory":PROJECT,"name":name}})
}

fn act(s: &mut Sim, cmd: Value) -> Value {
    s.cmd(Sender::Agent(FOREMAN.into()), cmd).unwrap()
}

#[test]
fn scheduled_positive_thresholds_and_manual_override() {
    for mode in ["scheduled", "manual"] {
        for threshold in ["landings", "edits", "notes", "days"] {
            let mut s = Sim::new();
            set_pass(&mut s, PROJECT, Read::Pulse, json!({"mode":mode,threshold:2}));
            s.state.landings.insert(PROJECT.into(), 100);
            s.operator(write("old", PROJECT)).unwrap();
            note(&mut s, PROJECT, "lesson");
            tick(&mut s);
            assert!(!running(&s));
            for n in 1..=2 {
                match threshold {
                    "landings" => *s.state.landings.get_mut(PROJECT).unwrap() += 1,
                    "edits" => {
                        s.operator(write("new", PROJECT)).unwrap();
                    }
                    "notes" => {
                        note(&mut s, PROJECT, "lesson");
                    }
                    _ => s.now += DAY,
                }
                tick(&mut s);
                assert_eq!(running(&s), mode == "scheduled" && n == 2, "{mode} {threshold} {n}");
            }
            tick(&mut s);
            assert_eq!(s.state.cards.len(), usize::from(mode == "scheduled"));
        }
    }
}

#[test]
fn close_and_early_done_rearm_notes_without_moving_last_completion_or_pulse_range() {
    let mut s = Sim::new();
    set_pass(&mut s, PROJECT, Read::Pulse, json!({"mode":"scheduled","notes":1}));
    tick(&mut s);
    note(&mut s, PROJECT, "lesson");
    tick(&mut s);
    s.operator(json!({"close":{"card":CARD,"why":"cancel"}})).unwrap();
    tick(&mut s);
    assert!(!running(&s));
    assert!(s.state.lookbacks.is_empty());
    note(&mut s, PROJECT, "lesson");
    tick(&mut s);
    let second = "lookback-web-2";
    cloned(&mut s, second);
    s.turn(&format!("{second}/foreman"), reply("report-ready", "Ready"));
    assert_eq!(s.stage(second), Stage::Done);
    assert!(!s.state.asks.values().any(|a| a.kind == mork_types::ask::AskKind::Done));
    let cursor = s.state.lookbacks[PROJECT][&Read::Pulse].clone();
    tick(&mut s);
    assert!(!running(&s));
    for dismiss in
        [json!({"close":{"card":"lookback-web-3","why":"cancel"}}), json!({"done":{"card":"lookback-web-4"}})]
    {
        note(&mut s, PROJECT, "lesson");
        tick(&mut s);
        s.operator(dismiss).unwrap();
        tick(&mut s);
        assert!(!running(&s));
        assert_eq!(s.state.lookbacks[PROJECT][&Read::Pulse], cursor);
    }
    note(&mut s, PROJECT, "lesson");
    tick(&mut s);
    assert_eq!(s.card("lookback-web-5").lookback.unwrap().from, cursor.commit);
}

#[test]
fn days_use_existing_completed_cursor_on_first_tick() {
    let mut s = Sim::new();
    let c = Cursor {
        project: PROJECT.into(),
        read: Read::Pulse,
        started_ms: None,
        ended_ms: s.now - 2 * DAY,
        commit: None,
    };
    s.state.lookbacks.entry(PROJECT.into()).or_default().insert(Read::Pulse, c);
    set_pass(&mut s, PROJECT, Read::Pulse, json!({"mode":"scheduled","days":2}));
    tick(&mut s);
    assert!(running(&s));
}

fn picks(s: &mut Sim, entries: &[(Cli, &str, &[&str])]) {
    let pass: Vec<_> = entries
        .iter()
        .map(|(cli, model, efforts)| json!({"cli":cli, "model":model,"label":model,"efforts":efforts}))
        .collect();
    s.config.picks = Some(serde_json::from_value(json!({"pass":pass})).unwrap());
}

fn auto(s: &mut Sim, cli: Cli, model: &str) {
    picks(s, &[(cli, model, &["high"])]);
    s.claude_account = Some("live");
    set_pass(s, PROJECT, Read::Pulse, json!({"mode":"auto"}));
    tick(s);
}

fn land(s: &mut Sim, project: &str, count: u64) {
    s.state.landings.insert(project.into(), count);
}

fn limit(
    s: &mut Sim,
    cli: Cli,
    bucket: &str,
    window: &str,
    percent: Option<u32>,
    reached: bool,
    account: Option<&str>,
) {
    let length = mork_types::usage::length_ms(window).unwrap();
    let l = Limit {
        cli,
        bucket: bucket.into(),
        window: window.into(),
        percent,
        reached,
        seen_ms: s.now,
        resets_at: Some((s.now + length / 2) / 1000),
        account: account.map(|id| Account { id: id.into(), plan: "max".into() }),
    };
    s.state.limits.insert(l.key(), l);
}

fn check_start(s: &mut Sim) {
    let view = s.operator(json!({"projects":{}})).unwrap();
    let project = view["projects"]["projects"].as_array().unwrap().iter().find(|p| p["name"] == PROJECT).unwrap();
    let row = s.journal.iter().find(|(_, kind, _, _)| kind == "card.start").unwrap();
    let data = &row.3;
    assert_eq!(row.2.as_deref(), Some(CARD));
    assert_eq!(data["score"]["landings"], project["passes"][0]["score"]["landings"]);
    assert_eq!(data["bar"], project["passes"][0]["bar"]);
    assert_eq!(data["bar"], 70);
    assert_eq!(data["spare"], 30);
    assert_eq!(data["pool"], "codex");
    assert_eq!(data["model"], SOL);
    assert_eq!(data["effort"], "high");
    assert!(data["pair_score"].as_f64().unwrap() > 0.0);
}

#[test]
fn auto_checks_activity_the_bar_and_each_applicable_window() {
    let mut s = Sim::new();
    auto(&mut s, Cli::Codex, SOL);
    s.config
        .seats
        .insert("foreman".into(), serde_json::from_value(json!({"report":{"cli":"claude","model":OPUS}})).unwrap());
    set_pass(&mut s, "ex", Read::Pulse, json!({"mode":"auto"}));
    tick(&mut s);
    s.now += 30 * DAY;
    limit(&mut s, Cli::Codex, "account", "week", Some(20), false, None);
    tick(&mut s);
    assert!(!running(&s), "days alone started a pass");
    land(&mut s, PROJECT, 1);
    limit(&mut s, Cli::Codex, "account", "week", Some(49), false, None);
    tick(&mut s);
    assert!(!running(&s), "activity below the bar started a pass");
    land(&mut s, PROJECT, 100);
    land(&mut s, "ex", 80);
    for (week, short, reached, starts) in [
        (None, None, false, false),
        (Some(50), None, false, false),
        (Some(20), Some(61), false, false),
        (Some(20), None, true, false),
        (Some(20), None, false, true),
    ] {
        limit(&mut s, Cli::Codex, "account", "week", week, false, None);
        limit(&mut s, Cli::Codex, "account", "5h", short, reached, None);
        tick(&mut s);
        assert_eq!(running(&s), starts);
    }
    check_start(&mut s);
    cloned(&mut s, CARD);
    s.turn_started(FOREMAN);
    let chosen = s.state.agents[FOREMAN].choice.as_ref().unwrap();
    assert_eq!((chosen.cli, chosen.model.as_str(), chosen.effort), (Cli::Codex, SOL, Effort::High));
    tick(&mut s);
    assert!(live(&s.state, &"ex".into()).is_none());
}

#[test]
fn dream_beats_an_eligible_pulse_on_new_project_edits_and_lessons() {
    let mut s = Sim::new();
    auto(&mut s, Cli::Codex, SOL);
    for read in [Read::DeepSweep, Read::Dream] {
        set_pass(&mut s, PROJECT, read, json!({"mode":"auto"}));
    }
    tick(&mut s);
    s.now += 14 * DAY;
    land(&mut s, PROJECT, 1);
    for n in 0..5 {
        note(&mut s, PROJECT, "lesson");
        s.operator(write(&format!("new-{n}"), PROJECT)).unwrap();
    }
    limit(&mut s, Cli::Codex, "account", "week", Some(0), false, None);
    tick(&mut s);
    assert_eq!(s.card(CARD).lookback.unwrap().read, Read::Dream);
    let score = &s.journal.iter().find(|(_, k, _, _)| k == "card.start").unwrap().3["score"];
    assert_eq!(score["notes"], 50.0);
    assert_eq!(score["edits"], 20.0);
    assert!(score["total"].as_f64().unwrap() >= 120.0);
}

#[test]
fn quota_uses_the_signed_in_account_and_the_launching_models_pool() {
    for (cli, good, bad) in [(Cli::Claude, "account", "account"), (Cli::Agy, "3p", "account")] {
        let mut s = Sim::new();
        auto(&mut s, cli, OPUS);
        land(&mut s, PROJECT, 100);
        limit(&mut s, cli, bad, "week", Some(5), false, (cli == Cli::Claude).then_some("old"));
        tick(&mut s);
        assert!(!running(&s), "another account or pool supplied headroom");
        limit(&mut s, cli, good, "week", Some(80), false, (cli == Cli::Claude).then_some("live"));
        tick(&mut s);
        assert!(!running(&s));
        limit(&mut s, cli, good, "week", Some(10), false, (cli == Cli::Claude).then_some("live"));
        tick(&mut s);
        assert!(running(&s));
    }
}

fn offers(s: &Sim) -> Vec<Offer> {
    pools::offers(
        &s.state.limits,
        s.now,
        s.claude_account,
        &s.config.seat("researcher").unwrap(),
        &s.config.picks.as_ref().unwrap().pass,
    )
}

#[test]
fn untouched_stale_and_measured_zero_have_distinct_offers() {
    let mut s = Sim::new();
    auto(&mut s, Cli::Agy, OPUS);
    assert!(!offers(&s)[0].open);
    limit(&mut s, Cli::Agy, "3p", "week", Some(0), false, None);
    s.state.limits.values_mut().next().unwrap().resets_at = None;
    assert_eq!(offers(&s)[0].spare, 50);
    s.now += 600_001;
    assert!(!offers(&s)[0].open);
    limit(&mut s, Cli::Agy, "3p", "week", Some(55), false, None);
    let pair = &offers(&s)[0];
    assert!(pair.open);
    assert_eq!(pair.spare, 0);
    picks(&mut s, &[(Cli::Claude, OPUS, &["high"])]);
    limit(&mut s, Cli::Claude, "account", "week", Some(10), false, Some("live"));
    s.now += 7 * DAY / 4;
    assert_eq!(offers(&s)[0].spare, 60, "80% projected unused times 75% elapsed");
    s.now += 7 * DAY / 4;
    assert!(!offers(&s)[0].open);
    limit(&mut s, Cli::Claude, "account", "week", Some(0), false, Some("live"));
    assert!(offers(&s)[0].open);
    s.now += 7 * DAY / 2 + 1;
    assert!(!offers(&s)[0].open, "zero Claude usage from the previous week reopened the pool");
}

#[test]
fn auto_pass_bar_and_launch_share_one_draw_across_pools() {
    let mut s = Sim::new();
    auto(&mut s, Cli::Codex, SOL);
    picks(&mut s, &[(Cli::Codex, SOL, &["low", "high"]), (Cli::Agy, OPUS, &["medium"])]);
    land(&mut s, PROJECT, 100);
    limit(&mut s, Cli::Codex, "account", "week", Some(10), false, None);
    limit(&mut s, Cli::Agy, "3p", "week", Some(60), false, None);
    let projects = s.operator(json!({"projects":{}})).unwrap();
    let project = projects["projects"]["projects"].as_array().unwrap().iter().find(|p| p["name"] == PROJECT).unwrap();
    tick(&mut s);
    let row = |kind| &s.journal.iter().find(|(_, k, c, _)| k == kind && c.as_deref() == Some(CARD)).unwrap().3;
    assert_eq!(row("card.start")["bar"], project["passes"][0]["bar"]);
    assert_eq!(row("route")["pick"]["model"], SOL);
    assert_eq!(row("route")["pick"]["effort"], row("card.start")["effort"]);
    assert_eq!(row("card.start")["spare"], 40);
}

#[test]
fn auto_researcher_uses_the_current_offer_outside_child_picks() {
    let mut s = Sim::new();
    auto(&mut s, Cli::Codex, SOL);
    land(&mut s, PROJECT, 100);
    limit(&mut s, Cli::Codex, "account", "week", Some(0), false, None);
    tick(&mut s);
    cloned(&mut s, CARD);
    let (_, prompt) = s.turn_started_with_argv(FOREMAN, &["x"]);
    assert!(prompt.contains("from picks.pass"));
    picks(&mut s, &[(Cli::Agy, OPUS, &["medium"])]);
    let spawn = json!({"spawn":{"card":CARD,"role":"researcher","task":"Read the code"}});
    let sender = Sender::Agent(FOREMAN.into());
    assert!(s.cmd(sender.clone(), spawn.clone()).unwrap_err().message.contains("no quota pool"));
    limit(&mut s, Cli::Agy, "3p", "week", Some(60), false, None);
    let mut named = spawn.clone();
    named["spawn"]["model"] = json!(OPUS);
    assert!(s.cmd(sender, named).unwrap_err().message.contains("engine chooses"));
    let child = act(&mut s, spawn)["spawn"]["agent"].as_str().unwrap().to_owned();
    assert!(child.ends_with("/researcher-1"));
    s.turn_started(&child);
    let chosen = s.state.agents[child.as_str()].choice.as_ref().unwrap();
    assert_eq!(
        (chosen.cli, chosen.model.as_str(), chosen.effort, chosen.by.as_str()),
        (Cli::Agy, OPUS, Effort::Medium, "router")
    );
}

#[test]
fn auto_waits_for_real_turns_and_their_owed_closing_turn_through_teardown() {
    let mut s = Sim::new();
    auto(&mut s, Cli::Codex, SOL);
    land(&mut s, PROJECT, 100);
    limit(&mut s, Cli::Codex, "account", "week", Some(10), false, None);
    s.through_brief("web-1", "web");
    let real = "web-1/foreman";
    tick(&mut s);
    assert!(!running(&s));
    let job = s.turn_started(real);
    tick(&mut s);
    assert!(!running(&s));
    s.end_job(&job, reply("working", "Working"));
    let a = s.state.agents.get_mut(real).unwrap();
    a.ran.turns = 3;
    a.ran.tools = true;
    a.session_started = true;
    a.last_turn_ms = Some(s.now);
    s.operator(json!({"close":{"card":"web-1","why":"cancel"}})).unwrap();
    assert!(matches!(s.state.agents[real].owed, Some(Owed::Lesson)));
    tick(&mut s);
    assert!(!running(&s));
    let a = s.state.agents.get_mut(real).unwrap();
    a.retiring = None;
    a.owed = Some(Owed::Wake);
    tick(&mut s);
    assert!(running(&s));
    cloned(&mut s, CARD);
    let pass_due = |s: &Sim| due(&s.state, &s.config, s.now).contains(&FOREMAN.into());
    assert!(pass_due(&s));
    s.through_brief("ex-1", "ex");
    assert!(!pass_due(&s));
    let pass = s.state.agents.get_mut(FOREMAN).unwrap();
    pass.retiring = Some(Retiring { remove: true, asked: false, resumed: false, live: true });
    pass.owed = Some(Owed::Lesson);
    pass.last_turn_ms = Some(s.now);
    assert!(!pass_due(&s));
    s.state.cards.get_mut(CARD).unwrap().lookback.as_mut().unwrap().trigger = Trigger::Scheduled;
    assert!(pass_due(&s));
}

fn check_memory_journal(s: &mut Sim) {
    act(s, write("merged", PROJECT));
    assert_eq!(s.state.memory_files["web/merged"].writer, FOREMAN);
    let file = s.state.memory_files["web/merged"].clone();
    act(s, json!({"memory.read":{"memory":PROJECT,"name":"merged"}}));
    assert_eq!(s.state.memory_files["web/merged"], file);
    act(s, delete("merged"));
    assert!(!s.state.memory_files.contains_key("web/merged"));
    let row = s.journal.iter().rev().find(|(_, k, _, _)| k == "memory.delete").unwrap();
    assert_eq!(row.2.as_deref(), Some(CARD));
    assert_eq!(row.3["before"]["text"], "Content");
    assert!(row.3["after"].is_null());
    let row = s.journal.iter().rev().find(|(_, k, _, _)| k == "memory.write").unwrap();
    assert!(row.3["before"].is_null());
    assert_eq!(row.3["after"]["text"], "Content");
}

#[test]
fn dream_changes_project_memory_and_leaves_concurrent_edits_for_the_next_pass() {
    let mut s = Sim::new();
    set_pass(&mut s, PROJECT, Read::Dream, json!({"mode":"scheduled","edits":2}));
    s.operator(write("rewritten", PROJECT)).unwrap();
    s.operator(write("removed", PROJECT)).unwrap();
    s.operator(json!({"lookback":{"project":PROJECT,"read":"dream"}})).unwrap();
    cloned(&mut s, CARD);
    check_memory_journal(&mut s);
    for memory in ["general", "ex"] {
        assert!(s.cmd(Sender::Agent(FOREMAN.into()), write("other", memory)).is_err());
    }
    let start = s.card(CARD).started_ms;
    s.now = start - 1000;
    s.operator(write("rewritten", PROJECT)).unwrap();
    assert_eq!(s.now, start);
    s.operator(delete("removed")).unwrap();
    for name in ["rewritten", "removed"] {
        assert!(s.cmd(Sender::Agent(FOREMAN.into()), write(name, PROJECT)).unwrap_err().message.contains("changed"));
    }
    let before = s.card(CARD).lookback.unwrap().at_start.unwrap();
    s.turn(FOREMAN, reply("report-ready", "Consolidated"));
    s.operator(json!({"done":{"card":CARD}})).unwrap();
    let base = &s.state.baselines[&(PROJECT.into(), Read::Dream)];
    assert_eq!(base.counts.memory_edits, before.memory_edits + 2);
    tick(&mut s);
    assert!(running(&s));
    cloned(&mut s, "lookback-web-2");
    s.cmd(Sender::Agent("lookback-web-2/foreman".into()), write("own", PROJECT)).unwrap();
    s.turn("lookback-web-2/foreman", reply("report-ready", "Done"));
    tick(&mut s);
    assert!(!running(&s));
}

#[test]
fn only_dream_foreman_can_fold_project_lessons() {
    let mut s = Sim::new();
    s.operator(json!({"lookback":{"project":PROJECT,"read":"dream"}})).unwrap();
    cloned(&mut s, CARD);
    let lesson = note(&mut s, PROJECT, "lesson");
    let feedback = note(&mut s, PROJECT, "feedback");
    let other = note(&mut s, "ex", "lesson");
    let remove = |p, ids: Vec<&str>| json!({"notes.delete":{"project":p,"ids":ids}});
    assert!(s.cmd(Sender::Agent(FOREMAN.into()), remove(PROJECT, vec![&lesson, &feedback])).is_err());
    assert!(s.cmd(Sender::Agent(FOREMAN.into()), remove("ex", vec![&other])).is_err());
    act(&mut s, remove(PROJECT, vec![&lesson]));
    let row = s.journal.iter().rev().find(|(_, k, _, _)| k == "notes.deleted").unwrap();
    assert_eq!(row.3["lessons"][&lesson], "a lesson");
    s.config.picks = Some(
        serde_json::from_value(
            json!({"researcher":[{"cli":"codex","model":"gpt-6.2-sol","label":"Sol","efforts":["high"]}]}),
        )
        .unwrap(),
    );
    act(
        &mut s,
        json!({"spawn":{"card":CARD,"role":"researcher","task":"Verify project memory","model":"gpt-6.2-sol","effort":"high"}}),
    );
    let researcher = Sender::Agent("lookback-web-1/researcher-1".into());
    s.operator(write("sample", PROJECT)).unwrap();
    assert!(s.cmd(researcher.clone(), write("other", PROJECT)).is_err());
    s.cmd(researcher, json!({"memory.read":{"memory":PROJECT,"name":"sample"}})).unwrap();
    assert_eq!(s.state.memory_files["web/sample"].reads, 0);
    s.operator(json!({"lookback":{"project":"ex","read":"pulse"}})).unwrap();
    cloned(&mut s, "lookback-ex-1");
    assert!(s.cmd(Sender::Agent("lookback-ex-1/foreman".into()), write("other", "ex")).is_err());
}

#[test]
fn manual_run_loaded_without_start_counts_uses_current_counts_on_done() {
    let mut s = Sim::new();
    s.operator(json!({"lookback":{"project":PROJECT}})).unwrap();
    s.state.cards.get_mut(CARD).unwrap().lookback.as_mut().unwrap().at_start = None;
    cloned(&mut s, CARD);
    s.operator(write("existing", PROJECT)).unwrap();
    let lesson = note(&mut s, PROJECT, "lesson");
    s.state.landings.insert(PROJECT.into(), 6);
    s.turn(FOREMAN, reply("report-ready", "Ready"));
    s.operator(json!({"done":{"card":CARD}})).unwrap();
    let counts = s.state.baselines[&(PROJECT.into(), Read::Pulse)].counts;
    assert_eq!((counts.landings, counts.memory_edits, counts.note), (6, 1, lesson.parse().unwrap()));
}
