// fac-265: the haiku's wall holds Andrei's Claude Code sessions folder and sockets, and nothing else of his;
// no other seat's wall holds either.
#![cfg(feature = "faults")]
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::Write;
use std::os::unix::net::UnixListener;
use std::time::Duration;

use common::Rig;
use factory_engine::jobs::bounded::{self, Bounds};
use factory_engine::jobs::context::Context;
use factory_engine::jobs::waker;
use factory_types::job::{JobKind, JobSpec};
use factory_types::{AgentKey, JobId};
use serde_json::{Value, json};

// A context whose app home and runtime folder are the rig's, where Andrei's would be.
fn context(rig: &Rig) -> Context {
    let config = factory_engine::config::read(&rig.t.config_dir().join("factory.toml")).unwrap();
    let mut ctx =
        Context::new(factory_engine::paths::Home::new(rig.fhome()), config, common::exe(), Default::default());
    ctx.app_home = rig.t.home().display().to_string();
    ctx.app_runtime = rig.t.xdg().display().to_string();
    // Every source path a wall binds read-only or read-write must be there, as the app makes them.
    for d in [ctx.home.run_dir(), ctx.home.builds(), ctx.home.agent_home(), ctx.home.waker_home()] {
        std::fs::create_dir_all(d).unwrap();
    }
    ctx
}

fn probed(ctx: &Context, mut spec: JobSpec, what: &Value) -> Value {
    let probe = common::exe().parent().unwrap().join("probe");
    spec.argv = vec![probe.display().to_string(), what.to_string()];
    let bounds = Bounds { max_out: 1 << 20, max_err: 1 << 16, timeout: Duration::from_secs(60) };
    let got = bounded::walled(ctx.home.root(), "probe", &spec, b"", bounds).unwrap();
    let out = String::from_utf8_lossy(&got.stdout).to_string();
    let err = String::from_utf8_lossy(&got.stderr).to_string();
    out.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["type"] == "probe")
        .unwrap_or_else(|| panic!("no probe line: {out} {err}"))
}

fn listen(path: &std::path::Path) -> UnixListener {
    let l = UnixListener::bind(path).unwrap();
    let clone = l.try_clone().unwrap();
    std::thread::spawn(move || {
        for mut s in clone.incoming().flatten() {
            let _ = s.write_all(b"hello\n");
        }
    });
    l
}

// Andrei's files and sockets, the seats' shared home and the waker's own, with dummy contents.
fn plant(rig: &Rig) {
    let (home, runtime) = (rig.t.home(), rig.t.xdg());
    let (shared, waker) = (rig.agent_home(), rig.fhome().join("waker-home"));
    for d in [home.join(".claude/sessions"), runtime.join("cc-socks"), shared.join(".claude"), waker.join(".claude")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(home.join(".claude/sessions/4242.json"), "{\"name\":\"orch\"}").unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), "a dummy secret").unwrap();
    std::fs::write(home.join("dummy-secret"), "a dummy secret").unwrap();
    std::fs::write(runtime.join("other-file"), "a dummy secret").unwrap();
    // The seats' shared home: their login and a settings file any seat can write; the waker has a login of its own.
    std::fs::write(shared.join(".claude/.credentials.json"), "a dummy login").unwrap();
    std::fs::write(shared.join(".claude/settings.json"), "a dummy hook").unwrap();
    std::fs::write(waker.join(".claude/settings.json"), "the waker's own").unwrap();
    std::fs::write(waker.join(".claude/.credentials.json"), "the waker's login").unwrap();
}

#[test]
fn fac_265_only_the_waker_sees_the_sessions_and_sockets() {
    if common::wall_test("fac-265") {
        return;
    }
    let rig = Rig::new();
    let ctx = context(&rig);
    let (home, runtime) = (rig.t.home(), rig.t.xdg());
    let (shared, waker) = (rig.agent_home(), rig.fhome().join("waker-home"));
    plant(&rig);
    let _peer = listen(&runtime.join("cc-socks/peer.sock"));
    let _bus = listen(&runtime.join("bus"));
    let sessions = waker.join(".claude/sessions");
    let peer = runtime.join("cc-socks/peer.sock").display().to_string();
    let s = |p: std::path::PathBuf| p.display().to_string();
    let what = json!({
        "read": [s(sessions.join("4242.json")), s(waker.join(".claude/.credentials.json")),
                 s(waker.join(".claude/settings.json")), s(shared.join(".claude/settings.json")),
                 s(shared.join(".claude/.credentials.json")), s(home.join(".claude/sessions/4242.json")),
                 s(home.join(".claude/.credentials.json")), s(home.join("dummy-secret")), s(runtime.join("other-file"))],
        "exists": [s(waker.clone())],
        "unix": [peer, s(runtime.join("bus"))],
        "writable": [s(sessions.clone()), s(runtime.join("cc-socks")), s(waker.clone()), s(shared.clone()),
                     s(shared.join(".claude"))],
    });
    let id = JobId::new("20261028-120000-wake-haiku-0001");
    let haiku = probed(&ctx, waker::spec(&ctx, &id, 0).unwrap(), &what);
    let read = |r: &Value, p: &std::path::Path| r["read"][p.display().to_string()].as_str().unwrap().to_owned();
    for seen in
        [sessions.join("4242.json"), waker.join(".claude/.credentials.json"), waker.join(".claude/settings.json")]
    {
        assert!(read(&haiku, &seen).starts_with("READ"), "the haiku couldn't read {}: {haiku}", seen.display());
    }
    // Its home is its own: the shared home's config, a seat's to write, is not in its wall.
    for hidden in [
        shared.join(".claude/settings.json"),
        shared.join(".claude/.credentials.json"),
        home.join(".claude/sessions/4242.json"),
        home.join(".claude/.credentials.json"),
        home.join("dummy-secret"),
        runtime.join("other-file"),
    ] {
        assert!(!read(&haiku, &hidden).starts_with("READ"), "the haiku read {}: {haiku}", hidden.display());
    }
    let unix = |r: &Value, p: &str| r["unix"][p].as_str().unwrap().to_owned();
    assert!(unix(&haiku, &peer).starts_with("CONNECTED") && unix(&haiku, &peer).contains("hello"), "{haiku}");
    assert!(!unix(&haiku, &runtime.join("bus").display().to_string()).starts_with("CONNECTED"), "{haiku}");
    for ro in [sessions.clone(), runtime.join("cc-socks"), shared.clone(), shared.join(".claude")] {
        assert_ne!(haiku["writable"][ro.display().to_string()], "WROTE", "{haiku}");
    }

    seat_sees_none(&ctx, &rig, &what, &peer);
}

// A seat's wall, made as every turn's is: neither the folder nor the sockets, and not the waker's home.
fn seat_sees_none(ctx: &Context, rig: &Rig, what: &Value, peer: &str) {
    let (home, shared, waker) = (rig.t.home(), rig.agent_home(), rig.fhome().join("waker-home"));
    let id = JobId::new("20261028-120000-wake-haiku-0001");
    let owner = AgentKey::new("main/echo");
    let places = ctx.places(&shared, Vec::new(), Vec::new(), None, &[]).unwrap();
    let env = ctx.env(&id, &owner, None, &[], &rig.fhome().join("jobs").join(id.as_str()).join("io"));
    let cwd = shared.display().to_string();
    let seat = ctx.spec(&id, JobKind::Turn, &owner, Vec::new(), &cwd, &places, env, Some(ctx.ports(None)), 0);
    let seat = probed(ctx, seat, what);
    for hidden in [
        waker.join(".claude/sessions/4242.json"),
        waker.join(".claude/settings.json"),
        home.join(".claude/sessions/4242.json"),
    ] {
        let got = seat["read"][hidden.display().to_string()].as_str().unwrap().to_owned();
        assert!(!got.starts_with("READ"), "a seat read {}: {seat}", hidden.display());
    }
    assert_eq!(seat["exists"][waker.display().to_string()], false, "a seat sees the waker's home: {seat}");
    assert_ne!(seat["writable"][waker.display().to_string()], "WROTE", "a seat wrote the waker's home: {seat}");
    let got = seat["unix"][peer].as_str().unwrap().to_owned();
    assert!(!got.starts_with("CONNECTED"), "a seat reached the peer: {seat}");
}
