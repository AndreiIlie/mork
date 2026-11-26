// fac-1: a path with shell metacharacters reaches the target as one literal argument, locally and over ssh,
// and no credential appears anywhere kept, even echoed by a server.
#![cfg(feature = "faults")]
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::door::{REMOTE_PW, SENTINELS, answers, ctl_turn, door_args, everything};
use common::{CODEX_FOREMAN, Rig, reply};
use mork_types::card::Mode;
use serde_json::{Value, json};

const SAVED_CLAUDE_TOKEN: &str = "SENTINEL-saved-claude-token-23";
const AGY_TOKENS: [&str; 2] = ["SENTINEL-agy-access-token-31", "SENTINEL-agy-refresh-token-29"];

// The project's repo lets its fetches pass.
fn run_calls(rig: &mut Rig, calls: &[Vec<String>]) -> Vec<Value> {
    rig.write_config(&rig.config(CODEX_FOREMAN));
    let c = rig.card("ex-1", Mode::Full);
    rig.put_cards(vec![c]);
    common::builds::mork_repo(rig, 1);
    rig.scenario(json!([ctl_turn("ex-1/foreman", None, calls, reply("working", "Done", json!([]))),]));
    rig.start();
    rig.say("ex-1", "@foreman Call the door");
    rig.wait_turns("ex-1/foreman", 1);
    answers(rig, &rig.jobs("ex-1/foreman").remove(0))
}

#[test]
fn fac1_arguments_reach_the_target_as_literal_arguments() {
    let mut rig = Rig::new();
    rig.door();
    let r = rig.t.root.display().to_string();
    let path = format!("a b; touch {r}/x1 && echo $(touch {r}/x2) `touch {r}/x3` 'q' \"d\" | cat > {r}/x4 # $HOME");
    let calls = [
        door_args("host", "sh.run", json!({"command": "echo", "args": {"path": path}})),
        door_args("staging", "sh.run", json!({"command": "echo", "args": {"path": path}})),
    ];
    let a = run_calls(&mut rig, &calls);
    for x in &a {
        assert_eq!(x["door"]["status"], "done", "{x}");
        assert_eq!(x["door"]["text"], path.as_str(), "the literal argument came back");
    }
    for f in ["x1", "x2", "x3", "x4"] {
        assert!(!rig.t.root.join(f).exists(), "{f}: something the argument named ran");
    }
    // Over ssh the path is one single-quoted word of the remote string, whatever it holds, so the
    // login shell there reads it back as one literal argument and runs nothing it names.
    let ssh = rig.door_runs("ssh");
    let remote = ssh[0]["argv"].as_array().unwrap().last().unwrap().as_str().unwrap();
    let expect = format!("'printf' '%s' '{}'", path.replace('\'', r"'\''"));
    assert_eq!(remote, expect, "the path is one single-quoted argument of the remote string");
}

fn plant_agy_login(home: &std::path::Path) -> std::path::PathBuf {
    // fac-657, fac-679: agy's sign-in in its real StoredToken shape with non-secret metadata.
    let agy = home.join("agy");
    std::fs::create_dir_all(&agy).unwrap();
    let agy_login = json!({
        "token": {
            "access_token": AGY_TOKENS[0],
            "token_type": "Bearer",
            "refresh_token": AGY_TOKENS[1],
            "expiry": "2026-11-23T15:00:00.000000000+02:00",
            "expires_in": 3599,
        },
        "auth_method": "personal",
        "id_token_email": "agent@example.com",
    });
    let file = agy.join("antigravity-oauth-token");
    std::fs::write(&file, agy_login.to_string()).unwrap();
    file
}

#[test]
fn fac1_no_credential_leaks_anywhere() {
    let mut rig = Rig::new();
    rig.door();
    let accounts = rig.fhome().join("login-home/.claude/accounts");
    std::fs::create_dir_all(&accounts).unwrap();
    let saved_login = json!({"claudeAiOauth": {"accessToken": SAVED_CLAUDE_TOKEN}});
    std::fs::write(accounts.join("backup.json.bak"), saved_login.to_string()).unwrap();
    let agy_token = plant_agy_login(&rig.fhome().join("logins"));
    let mut world = rig.door_world_now();
    world["mysql"] = json!([
        {"match": "'out'", "password": "out"},
        {"match": "'err'", "password": "err", "exit": 1, "out": ""},
    ]);
    world["mongoexport"] = json!([{"password": "out"}]);
    rig.door_fake(world);
    let log = format!(
        "personal note with {REMOTE_PW} and {SAVED_CLAUDE_TOKEN} and {} and {}\n",
        AGY_TOKENS[0], AGY_TOKENS[1]
    );
    std::fs::write(rig.remote().join("app.log"), log).unwrap();
    let calls = [
        door_args("host", "sql.select", json!({"query": "SELECT id, name FROM users WHERE name = 'out'"})),
        door_args("host", "sql.select", json!({"query": "SELECT id FROM users WHERE name = 'err'"})),
        door_args("staging", "sql.select", json!({"query": "SELECT id, name FROM users WHERE name = 'out'"})),
        door_args("staging", "sql.select", json!({"query": "SELECT id FROM users WHERE name = 'err'"})),
        door_args("host", "mongo.find", json!({"collection": "requests"})),
        // Review finding 5: an engine-only credential in a local log stays out of its receipt.
        door_args("host", "file.tail", json!({"log": "creds"})),
        // Second review: the remote database's password in a remote log, masked by its dispatcher.
        door_args("staging", "file.tail", json!({"log": "app"})),
    ];
    let a = run_calls(&mut rig, &calls);
    let all = serde_json::to_string(&a).unwrap();
    assert!(all.contains("‹secret›"), "the leaks were masked, not lost: {all}");
    assert_eq!(a[1]["door"]["status"], "failed");
    assert_eq!(a[2]["door"]["status"], "done", "{}", a[2]);
    assert_eq!(a[3]["door"]["status"], "failed");
    assert_eq!(a[5]["door"]["text"], "connected with ‹secret›");
    assert_eq!(a[6]["door"]["text"], "personal note with ‹secret› and ‹secret› and ‹secret› and ‹secret›", "{}", a[6]);
    assert!(std::fs::read_dir(rig.fhome().join("door/receipts")).unwrap().count() >= 4, "local receipts kept");
    rig.term();
    // Planted where agy keeps it, the file alone may hold it.
    std::fs::remove_file(agy_token).unwrap();
    let seen = everything(&rig);
    for s in SENTINELS.into_iter().chain(AGY_TOKENS) {
        assert!(!all.contains(s), "{s} in an answer: {all}");
        assert!(!seen.contains(s), "{s} in a job folder, record, log or the state");
    }
}
