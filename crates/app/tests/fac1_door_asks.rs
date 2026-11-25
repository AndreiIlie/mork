// fac-1, fac-124: an outside call needs Andrei's exact-plan approval; invalid calls never become asks.
#![cfg(feature = "faults")]
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::door::{STAGING_ALIAS, answers, ctl_turn, door_args, remote_commands};
use common::{CODEX_FOREMAN, Client, Rig, reply, wait_until};
use factory_types::ask::AskKind;
use factory_types::card::Mode;
use serde_json::{Value, json};

const LONG: Duration = Duration::from_secs(60);

fn door_asks(rig: &Rig) -> Vec<factory_types::ask::Ask> {
    rig.state().asks.into_values().filter(|a| a.kind == AskKind::Door).collect()
}

fn ask_of(rig: &Rig, text: &str) -> String {
    let found = door_asks(rig).into_iter().find(|a| a.text.contains(text) || a.detail.contains(text));
    found.unwrap_or_else(|| panic!("no ask with {text}")).id.to_string()
}

// The card's foreman makes the requests: Andrei's `@foreman` wakes it. Its project's repo is
// one, so its fetches pass.
fn card(rig: &Rig) {
    rig.write_config(&rig.config(CODEX_FOREMAN));
    let c = rig.card("ex-1", Mode::Full);
    rig.put_cards(vec![c]);
    common::builds::factory_repo(rig, 1);
}

fn news(rig: &Rig, place: &str, to: &str) -> Vec<factory_types::state::Message> {
    rig.messages(place)
        .into_iter()
        .filter(|m| m.to.as_deref() == Some(to) && m.headline.starts_with("door: "))
        .collect()
}

// Later turns consume the replies to the door calls.
fn later() -> Value {
    json!({"fake": "codex", "agent": "ex-1/foreman", "steps": [{"do": "thread-started"}, {"do": "codex-reply", "value": reply("working", "Seen", json!([]))}]})
}

// Mixing reads and asks catches accidental approval prompts on the allowed read path.
fn probed() -> (Rig, Vec<Value>) {
    let mut rig = Rig::new();
    rig.door();
    card(&rig);
    let calls = vec![
        door_args("host", "sql.select", json!({"query": "SELECT name FROM users WHERE name = 'read-probe'"})),
        door_args("staging", "misc.run", json!({"argv": ["printf", "%s", "parallel ran"]})),
        door_args("staging", "misc.run", json!({"argv": ["printf", "%s", "refuse ran"]})),
        door_args("staging", "misc.run", json!({"argv": ["printf", "%s", "misc ran"]})),
        door_args("staging", "sql.drop", json!({"table": "users"})),
        door_args("host", "sql.select", json!({"query": "DELETE FROM users"})),
        door_args("staging", "file.cat", json!({"root": "app", "path": "../../etc/passwd"})),
        door_args("staging", "file.cat", json!({"root": "app", "path": "passwd"})),
        door_args("nowhere", "sql.select", json!({"query": "SELECT 1"})),
        vec!["door.once".into(), "ex-1/door/1".into()],
    ];
    rig.scenario(json!([
        ctl_turn("ex-1/foreman", Some("Probe the door"), &calls, reply("working", "Probed", json!([]))),
        later(),
    ]));
    rig.start();
    rig.say("ex-1", "@foreman Probe the door");
    rig.wait_turns("ex-1/foreman", 1);
    let a = answers(&rig, &rig.jobs("ex-1/foreman").remove(0));
    (rig, a)
}

#[test]
fn fac1_refusals_and_misc_are_andreis_asks() {
    let (rig, a) = probed();
    let status: Vec<&str> = a.iter().map(|x| x["door"]["status"].as_str().unwrap_or("")).collect();
    let want = ["done", "ask raised", "ask raised", "ask raised", "unsupported", "not allowed", "not allowed"];
    assert_eq!(&status[..7], want, "{a:#?}");
    assert_eq!(&status[7..9], ["failed", "not allowed"], "{a:#?}");
    for x in &a[1..4] {
        assert_eq!(x["door"]["words"], "waiting for Andrei");
    }
    let text = a[7]["door"]["text"].as_str().unwrap();
    assert!(text.contains("passwd"), "a symlink under the root isn't followed: {}", a[7]);
    assert_eq!(a[9]["err"]["code"], "buzz", "an agent can't say once: {}", a[9]);
    let asks = door_asks(&rig);
    assert_eq!(asks.len(), 3, "{asks:#?}");
    for x in &asks {
        assert!(x.open && x.place == "ex-1", "{x:?}");
        assert!(x.text.starts_with("ex-1/foreman wants "), "{}", x.text);
        assert!(x.detail.contains("allowlist: ") && !x.detail.contains("model check: "), "{}", x.detail);
    }
    assert!(ask_of(&rig, "misc.run").starts_with("ex-1/door/"));
    assert_eq!(rig.door_runs("mysql").len(), 1, "the allowed read ran");
    assert_eq!(remote_commands(&rig, "misc ran"), 0);
}

// A replay is refused, and two clicks at once run a call once.
#[test]
fn fac1_once_runs_once_and_no_refuses() {
    let (rig, _) = probed();
    let misc = ask_of(&rig, "misc ran");
    assert!(rig.send(json!({"door.once": {"ask": misc}})).get("ok").is_some());
    wait_until("the misc call's news", LONG, || !news(&rig, "ex-1", "foreman").is_empty());
    let n = news(&rig, "ex-1", "foreman").remove(0);
    assert!(n.text.contains("\"misc ran\"") && n.text.contains("\"done\""), "{}", n.text);
    rig.wait_turns("ex-1/foreman", 2);
    assert!(rig.prompts("ex-1/foreman")[1].contains("misc ran"), "the news is in its next turn");
    assert_eq!(remote_commands(&rig, "misc ran"), 1);
    let again = rig.send(json!({"door.once": {"ask": misc}}));
    assert_eq!(again["err"]["code"], "refused", "{again}");
    let outside = ask_of(&rig, "refuse ran");
    assert!(rig.send(json!({"door.no": {"ask": outside}})).get("ok").is_some());
    let told_no = || news(&rig, "ex-1", "foreman").iter().any(|m| m.text.contains("Andrei said no"));
    wait_until("the no's news", LONG, told_no);
    let parallel = ask_of(&rig, "parallel ran");
    let mut c1 = Client::connect(&rig.fhome().join("factory.sock"));
    let mut c2 = Client::connect(&rig.fhome().join("factory.sock"));
    c1.request(json!({"door.once": {"ask": parallel}}));
    c2.request(json!({"door.once": {"ask": parallel}}));
    let (r1, r2) = (c1.answer(), c2.answer());
    let oks = [&r1, &r2].iter().filter(|r| r.get("ok").is_some()).count();
    assert_eq!(oks, 1, "{r1} {r2}");
    wait_until("the parallel call's run", LONG, || remote_commands(&rig, "parallel ran") == 1);
    rig.wait_loop();
    rig.wait_actions_done();
    assert_eq!(remote_commands(&rig, "parallel ran"), 1, "it ran once");
}

// An approval binds to the plan Andrei saw: a configuration changed since voids it.
#[test]
fn fac1_a_changed_configuration_voids_an_approval() {
    let mut rig = Rig::new();
    rig.door();
    card(&rig);
    let call = door_args("staging", "misc.run", json!({"argv": ["printf", "%s", "x"]}));
    rig.scenario(json!([
        ctl_turn("ex-1/foreman", Some("Run misc"), &[call], reply("working", "Asked", json!([]))),
        later(),
    ]));
    rig.start();
    rig.say("ex-1", "@foreman Run misc");
    rig.wait_turns("ex-1/foreman", 1);
    let ask = ask_of(&rig, "misc.run");
    let targets = rig.fhome().join("targets.toml");
    // The alias the call would run over is part of its plan: change it, and the plan it was
    // approved on is no longer the one the file gives.
    let text = std::fs::read_to_string(&targets).unwrap().replace(STAGING_ALIAS, "ex-other-box");
    std::fs::write(&targets, text).unwrap();
    assert!(rig.send(json!({"door.once": {"ask": ask}})).get("ok").is_some());
    wait_until("the news", LONG, || !news(&rig, "ex-1", "foreman").is_empty());
    let n = news(&rig, "ex-1", "foreman").remove(0);
    assert!(n.text.contains("not allowed") && n.text.contains("configuration changed"), "{}", n.text);
    assert_eq!(remote_commands(&rig, "x"), 0, "nothing ran");
}

// Disabling the misc door must invalidate an outstanding once (fac-124).
#[test]
fn fac124_a_revoked_misc_door_voids_an_approval() {
    let mut rig = Rig::new();
    rig.door();
    card(&rig);
    let call = door_args("staging", "misc.run", json!({"argv": ["printf", "%s", "revoke-probe"]}));
    rig.scenario(json!([
        ctl_turn("ex-1/foreman", Some("Run misc"), &[call], reply("working", "Asked", json!([]))),
        later(),
    ]));
    rig.start();
    rig.say("ex-1", "@foreman Run misc");
    rig.wait_turns("ex-1/foreman", 1);
    let ask = ask_of(&rig, "revoke-probe");
    let targets = rig.fhome().join("targets.toml");
    let text = std::fs::read_to_string(&targets).unwrap().replace("misc = true", "misc = false");
    std::fs::write(&targets, text).unwrap();
    assert!(rig.send(json!({"door.once": {"ask": ask}})).get("ok").is_some());
    wait_until("the news", LONG, || !news(&rig, "ex-1", "foreman").is_empty());
    let n = news(&rig, "ex-1", "foreman").remove(0);
    assert!(n.text.contains("not allowed") && n.text.contains("no misc door"), "{}", n.text);
    assert_eq!(remote_commands(&rig, "revoke-probe"), 0, "nothing ran");
}

// The generic action commands never run a door call, nor settle one for Andrei.
#[test]
fn fac1_generic_commands_never_bypass_the_door() {
    let mut rig = Rig::new();
    rig.door();
    card(&rig);
    let call = door_args("staging", "misc.run", json!({"argv": ["printf", "%s", "x"]}));
    // act-1 is the card's project's fetch; the call is act-2.
    let act = |cmd: &str| vec![cmd.to_owned(), "act-2".to_owned(), "--note".to_owned(), "seen".to_owned()];
    let drop = vec!["action.drop".to_owned(), "act-2".to_owned()];
    rig.scenario(json!([ctl_turn(
        "ex-1/foreman",
        Some("Run misc"),
        &[call, drop, act("action.done")],
        reply("working", "Asked", json!([]))
    ),]));
    rig.start();
    rig.say("ex-1", "@foreman Run misc");
    rig.wait_turns("ex-1/foreman", 1);
    let a = answers(&rig, &rig.jobs("ex-1/foreman").remove(0));
    assert_eq!(a[0]["door"]["status"], "ask raised");
    assert_eq!(a[1]["err"]["code"], "buzz", "{}", a[1]);
    assert_eq!(a[2]["err"]["code"], "buzz", "{}", a[2]);
    for cmd in [json!({"action.retry": {"action": "act-2"}}), json!({"action.done": {"action": "act-2", "note": "x"}})]
    {
        let r = rig.send(cmd.clone());
        assert_eq!(r["err"]["code"], "refused", "{cmd}: {r}");
    }
    assert_eq!(remote_commands(&rig, "x"), 0);
    // Andrei's drop is his no.
    assert!(rig.send(json!({"action.drop": {"action": "act-2"}})).get("ok").is_some());
    let st = rig.state();
    assert!(st.asks.values().all(|x| !x.open || x.kind != AskKind::Door), "its ask closed");
    assert_eq!(factory_core::door::status_of(&st.actions["act-2"]), "not allowed");
    let _: Value = json!(null);
}
