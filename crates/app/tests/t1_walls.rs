// The wall (1.4.11): T1.33-T1.39, T1.53.
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

mod common;

use std::time::Duration;

use common::{Rig, wait_until};
use serde_json::{Value, json};

const LONG: Duration = Duration::from_secs(40);

// fac-657: the tests' seat on agy, walled as every agy seat is.
const AGY_ECHO: &str = "[seats.echo]\ncli = \"agy\"\nmodel = \"gemini-4-neon\"\neffort = \"high\"\n";

fn probe(rig: &mut Rig, spec: Value) -> Value {
    probe_on(rig, "claude", spec)
}

fn probe_on(rig: &mut Rig, fake: &str, spec: Value) -> Value {
    rig.scenario(json!([{"fake": fake, "steps": [
        {"do": "init"}, {"do": "probe", "spec": spec}, {"do": "echo", "text": "probed"}]}]));
    if rig.app.is_none() {
        rig.start();
    }
    let before = rig.jobs("main/echo").len();
    rig.echo("probe");
    wait_until("the probe's job", LONG, || rig.jobs("main/echo").len() > before);
    rig.wait_settled(LONG);
    probe_line(rig).unwrap_or_else(|| panic!("no probe line; app log: {}", rig.app_log()))
}

fn job_dir(rig: &Rig) -> std::path::PathBuf {
    let j = rig.jobs("main/echo").pop().unwrap();
    rig.fhome().join("jobs").join(j.id.as_str())
}

// The echo seat's `io/` folder, its `$MORK_IO` in every turn (fac-654).
fn seat_io(rig: &Rig) -> std::path::PathBuf {
    let j = rig.jobs("main/echo").pop().unwrap();
    j.spec.env.get("MORK_IO").map(std::path::PathBuf::from).unwrap()
}

fn probe_line(rig: &Rig) -> Option<Value> {
    let out = std::fs::read_to_string(job_dir(rig).join("out.jsonl")).ok()?;
    out.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["type"] == "probe")
}

#[cfg(feature = "faults")]
mod walled {
    use super::*;

    // 1.10.9, 1.10.10: with `MORK_UNWALLED=1` a wall test skips, saying why; without it, it never
    // does. In the train's walled phase its skip is recorded for the unwalled phase (fac-65).
    #[test]
    fn r1_10_10_wall_tests_skip_only_when_unwalled() {
        let run_one = |test: &str, unwalled: bool, train: &[(&str, &str)]| {
            let mut c = std::process::Command::new(std::env::current_exe().unwrap());
            c.args([test, "--exact", "--nocapture", "--test-threads=1"]);
            c.env_remove(common::TRAIN_PHASE).env_remove(common::WALL_TESTS).envs(train.iter().copied());
            if unwalled {
                c.env("MORK_UNWALLED", "1")
            } else {
                c.env_remove("MORK_UNWALLED")
            };
            let out = c.output().unwrap();
            (
                out.status.success(),
                format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
            )
        };
        let run = |unwalled: bool, train: &[(&str, &str)]| run_one("walled::t1_33_what_a_wall_sees", unwalled, train);
        let (ok, text) = run(true, &[]);
        assert!(ok && text.contains("skipped: T1.33") && text.contains("MORK_UNWALLED=1"), "{text}");
        let record = std::env::temp_dir().join(format!("wall-tests-record-{}", std::process::id()));
        let file = record.display().to_string();
        let (ok, text) = run(true, &[(common::TRAIN_PHASE, "walled"), (common::WALL_TESTS, &file)]);
        let recorded = std::fs::read_to_string(&record).unwrap_or_default();
        let _ = std::fs::remove_file(&record);
        assert!(ok && recorded == "walled::t1_33_what_a_wall_sees\n", "{recorded:?}: {text}");
        // Only the walled phase, given its record, records: unwalled, a leaked record stays unwritten.
        // Each skip still passes; failing one in the train is fac-123's, after fac-65's reload.
        for train in [
            &[(common::TRAIN_PHASE, "unwalled")][..],
            &[(common::TRAIN_PHASE, "unwalled"), (common::WALL_TESTS, &file)],
            &[(common::TRAIN_PHASE, "walled")],
        ] {
            let (ok, text) = run(true, train);
            assert!(ok && text.contains("skipped: T1.33"), "{train:?}: {text}");
            assert!(!record.exists(), "{train:?}: a skip was recorded: {text}");
        }
        // T1.34's own skip, below ABI 6, starts no wall: it says why, and the walled phase records it.
        let t1_34 = "walled::t1_34_sockets_and_ports";
        let (ok, text) = run_one(t1_34, false, &[("MORK_TEST_LANDLOCK_ABI", "5")]);
        assert!(ok && text.contains("skipped: T1.34: Landlock ABI 5 is below 6"), "{text}");
        let below = [("MORK_TEST_LANDLOCK_ABI", "5"), (common::TRAIN_PHASE, "walled"), (common::WALL_TESTS, &file)];
        let (ok, text) = run_one(t1_34, false, &below);
        let recorded = std::fs::read_to_string(&record).unwrap_or_default();
        let _ = std::fs::remove_file(&record);
        assert!(ok && recorded == format!("{t1_34}\n"), "{recorded:?}: {text}");
        if common::wall_test("the walled half of 1.10.10's check") {
            return;
        }
        let (ok, text) = run(false, &[]);
        assert!(ok && !text.contains("skipped:") && text.contains("1 passed"), "{text}");
    }

    #[test]
    fn configured_ticket_token_is_masked_in_the_wall() {
        if common::wall_test("configured tickets token") {
            return;
        }
        let mut rig = Rig::new();
        let token = rig.t.bin().join("ticket-token");
        let github = rig.t.bin().join("github-token");
        // An invalid credential keeps this wall test entirely offline.
        std::fs::write(&github, "fixture secret").unwrap();
        std::fs::write(&token, "sample-ticket-secret").unwrap();
        let cfg = rig.config(&format!(
            r#"[projects.mork.tickets]
kind = "azure-devops"
api = "https://example.invalid/api"
page = "https://example.invalid/tickets/"
token = "{}"
[projects.mork.forge]
kind = "github"
owner = "fixture"
repo = "fixture"
token = "{}"
"#,
            token.display(),
            github.display()
        ));
        rig.write_config(&cfg);
        let result = probe(&mut rig, json!({"read": [&token,&github]}));
        assert_eq!(result["read"][token.display().to_string()], "READ ", "{result}");
        assert_eq!(result["read"][github.display().to_string()], "READ ", "{result}");
    }

    const HIDDEN_NAMES: &[&str] = &[
        "ACCESS.md",
        "access-shape.txt",
        ".env",
        ".session-secret",
        "client-key.pem",
        "setup-credentials.ps1",
        "update-sudo-passwords.py",
    ];
    const PUBLIC_NAMES: &[&str] = &["visible", "access.md", "client-key.pem.bak", "secrets.ts", "NewPasswordCtrl.js"];

    fn listed_folder(rig: &mut Rig) -> std::path::PathBuf {
        let work = rig.t.home().join("Work");
        let data = work.join("data");
        std::fs::create_dir_all(data.join("ACCESS-private")).unwrap();
        for name in HIDDEN_NAMES {
            std::fs::write(data.join(name), "a dummy secret").unwrap();
        }
        for name in PUBLIC_NAMES {
            std::fs::write(data.join(name), "whole").unwrap();
        }
        std::fs::write(data.join("ACCESS-private/hidden"), "a dummy secret").unwrap();
        let cfg = rig.config(&format!("data = \"{}\"", data.display())).replacen(
            "toolchains = [",
            &format!(r#"toolchains = [{{ path = "{}", hide = ["ACCESS*", "access-shape.txt", ".env", ".session-secret", "*-key.pem", "setup-credentials.*", "*sudo-password*"] }}, "#, work.display()),
            1,
        );
        rig.write_config(&cfg);
        work
    }

    fn plant_wall(rig: &Rig) -> [std::path::PathBuf; 3] {
        let secrets =
            [rig.t.home().join("dummy-secret"), rig.t.tmp().join("dummy-secret"), rig.t.xdg().join("dummy-secret")];
        for secret in &secrets {
            std::fs::write(secret, "a dummy secret").unwrap();
        }
        let f = rig.fhome();
        let other = ["jobs/20200101-000000-other-0000/io", "io/20200101-000000-other-0000"];
        for dir in ["mirrors/proj", "clones/other-card", "login-home/.claude/accounts"].iter().chain(&other) {
            std::fs::create_dir_all(f.join(dir)).unwrap();
        }
        std::fs::write(f.join("clones/other-card/visible"), "whole").unwrap();
        std::fs::write(f.join("login-home/.claude/accounts/saved"), "a dummy login").unwrap();
        std::fs::write(rig.t.root.join("public"), "whole").unwrap();
        secrets
    }

    fn assert_listed(r: &Value, root: &std::path::Path) {
        for name in HIDDEN_NAMES {
            assert_eq!(r["read"][root.join(name).display().to_string()], "READ ", "{name}: {r}");
        }
        for name in PUBLIC_NAMES {
            assert_eq!(r["read"][root.join(name).display().to_string()], "READ whole", "{name}: {r}");
        }
        assert_eq!(r["exists"][root.join("ACCESS-private/hidden").display().to_string()], false, "{r}");
    }

    fn assert_hidden(r: &Value, secrets: &[std::path::PathBuf], hidden: &[std::path::PathBuf]) {
        for secret in secrets {
            assert!(!r["read"][secret.display().to_string()].as_str().unwrap().starts_with("READ"), "{r}");
        }
        for path in hidden {
            assert_eq!(r["exists"][path.display().to_string()], false, "{}: {r}", path.display());
        }
    }

    struct CrashFixture(std::path::PathBuf);

    impl Drop for CrashFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn crash_fixture(rig: &Rig) -> Option<CrashFixture> {
        let name = rig.t.root.file_name()?.to_str()?;
        // No `.crash` suffix: the desktop's crash notifier would report it to the operator.
        let path = std::path::Path::new("/var/crash").join(format!("mork-test-{name}"));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).ok()?;
        let fixture = CrashFixture(path);
        std::io::Write::write_all(&mut file, b"a dummy crash secret").ok()?;
        Some(fixture)
    }

    #[test]
    fn t1_33_what_a_wall_sees() {
        if common::wall_test("T1.33") {
            return;
        }
        let mut rig = Rig::new();
        let work = listed_folder(&mut rig);
        let mut secrets = plant_wall(&rig).to_vec();
        let crash = crash_fixture(&rig);
        if let Some(crash) = &crash {
            secrets.push(crash.0.clone());
        }
        let f = rig.fhome();
        let bus = rig.t.xdg().join("bus");
        let _bus = std::os::unix::net::UnixListener::bind(&bus).unwrap();
        let hidden = [
            f.join("state.db"),
            f.join("jobs/20200101-000000-other-0000"),
            f.join("io/20200101-000000-other-0000"),
            f.join("mirrors"),
            f.join("mork.sock"),
            "/run/dbus/system_bus_socket".into(),
            bus.clone(),
            f.join("login-home"),
            f.join("agent-home"),
            f.join("login-home/.claude/accounts/saved"),
        ];
        let mut read = secrets.to_vec();
        read.extend(HIDDEN_NAMES.iter().chain(PUBLIC_NAMES).map(|n| work.join("data").join(n)));
        let public = [rig.t.root.join("public"), f.join("clones/other-card/visible")];
        read.extend(public.clone());
        let mut exists = hidden.to_vec();
        exists.push(work.join("data/ACCESS-private/hidden"));
        let mut readonly = vec![rig.t.root.clone(), work.clone(), f.join("clones/other-card")];
        if crash.is_some() {
            readonly.push("/var/crash".into());
        }
        let mut writable = readonly.to_vec();
        writable.extend(["$HOME".into(), f.join("logins")]);
        let r = probe(
            &mut rig,
            json!({
                "read": read, "exists": exists,
                "unix": [f.join("run/agent.sock"), bus.clone()], "writable": writable,
            }),
        );
        assert_hidden(&r, &secrets, &hidden);
        for path in &public {
            assert_eq!(r["read"][path.display().to_string()], "READ whole", "{r}");
        }
        for path in &readonly {
            assert_eq!(r["writable"][path.display().to_string()], "EROFS", "{r}");
        }
        assert_listed(&r, &work.join("data"));
        let sock = r["unix"][f.join("run/agent.sock").display().to_string()].as_str().unwrap();
        assert!(sock.starts_with("CONNECTED") && sock.contains("hello"), "{r}");
        assert!(!r["unix"][bus.display().to_string()].as_str().unwrap().starts_with("CONNECTED"), "{r}");
        assert_eq!(r["writable"]["$HOME"], "WROTE");
        assert_eq!(r["writable"][f.join("logins").display().to_string()], "WROTE");
        let r = probe(&mut rig, json!({"writable": ["$IO"]}));
        assert_eq!(r["writable"]["$IO"], "WROTE", "{r}");
        assert!(seat_io(&rig).starts_with(f.join("io")), "its $MORK_IO is its seat's folder");
    }

    #[test]
    fn seat_homes_isolate_foreman_and_reviewer() {
        if common::wall_test("seat homes") {
            return;
        }
        use mork_types::card::{Mode, Stage};
        let mut rig = Rig::new();
        let cfg = rig.config("").replace("[mork]\n", "[mork]\ncaches = ['shared']\ncache_files = ['cache.lock']\n");
        rig.write_config(&cfg);
        let mut card = rig.card("ex-1", Mode::Review);
        card.stage = Stage::Review;
        rig.put_cards(vec![card]);
        let work = rig.fhome().join("work/ex-1");
        let lock = r#"stat -c '%d:%i' "$HOME/cache.lock" > "$MORK_IO/lock-id""#;
        let first = r#"echo private > "$HOME/marker" && echo "$HOME" > "$HOME/shared/foreman""#;
        let hidden = r#"test ! -e "$(cat "$HOME/shared/foreman")/marker""#;
        let spec = json!({"read":["$HOME/shared/foreman"],"exec":[["sh","-c",hidden]],
            "exists":[work,rig.fhome().join("agent-home"),rig.fhome().join("login-home")],
            "writable":["$HOME",rig.fhome().join("logins")]});
        rig.scenario(json!([
            {"agent":"ex-1/foreman","steps":[{"do":"sh","script":first},{"do":"sh","script":lock},
             {"do":"probe","spec":{"writable":[work]}},
             {"do":"ctl","args":["spawn","ex-1","reviewer","--topic","Homes","Inspect the home."]},{"do":"hang"}]},
            {"agent":"ex-1/reviewer-1","steps":[{"do":"sh","script":lock},{"do":"probe","spec":spec},{"do":"hang"}]}
        ]));
        rig.start();
        rig.say("ex-1", "Go.");
        let seen = |key| {
            let job = rig.jobs(key).into_iter().next()?;
            let probe = rig.out_lines(&job).into_iter().find(|v| v["type"] == "probe")?;
            Some((job, probe))
        };
        wait_until("reviewer home probe", LONG, || seen("ex-1/reviewer-1").is_some());
        let (foreman, a) = seen("ex-1/foreman").unwrap();
        let (reviewer, b) = seen("ex-1/reviewer-1").unwrap();
        assert_eq!(a["writable"][work.display().to_string()], "WROTE");
        assert!(b["exists"].as_object().unwrap().values().all(|v| v == false), "{b}");
        assert!(b["writable"].as_object().unwrap().values().all(|v| v == "WROTE"), "{b}");
        let first = &foreman.spec.env["HOME"];
        assert_ne!(first, &reviewer.spec.env["HOME"]);
        assert!(std::path::Path::new(first).join("marker").is_file());
        assert_eq!(std::fs::read_to_string(rig.fhome().join("caches/shared/foreman")).unwrap().trim(), first);
        assert!(b["read"]["$HOME/shared/foreman"].as_str().unwrap().starts_with("READ /"), "{b}");
        assert_eq!(b["exec"][0]["code"], 0, "{b}");
        let lock_id = |job: &mork_types::state::Job| {
            std::fs::read_to_string(std::path::Path::new(&job.spec.env["MORK_IO"]).join("lock-id")).unwrap()
        };
        let id = lock_id(&foreman);
        assert!(id.contains(':'));
        assert_eq!(id, lock_id(&reviewer));
        rig.send(json!({"stop":{"card":"ex-1"}}));
        rig.wait_settled(LONG);
    }

    #[test]
    fn checks_wall_hides_cli_logins() {
        if common::wall_test("check job logins") {
            return;
        }
        use mork_types::card::{Mode, Stage};
        let mut rig = Rig::new();
        let head = mork_types::Sha::new(common::builds::mork_repo(&rig, 1).remove(0));
        let repo = rig.t.root.join("mork-repo");
        // An owed check has already had its submitted commit taken into the project's mirror.
        common::builds::git(&rig.fhome(), &["clone", "--bare", repo.to_str().unwrap(), "mirrors/mork"]);
        rig.write_config(
            &rig.config(&format!("checks = {{ command = 'fake-checks', folder = {:?} }}", repo.display().to_string())),
        );
        let mut card = rig.card("ex-1", Mode::Full);
        card.stage = Stage::Checks;
        card.head = Some(head.clone());
        let purpose = mork_types::work::Purpose::Check { card: card.id.clone(), head: head.clone(), fork: head };
        rig.put_cards(vec![card]);
        rig.put(vec![mork_core::state::Change::PutOwedWork(purpose)]);
        let logins = rig.fhome().join("logins");
        std::fs::create_dir_all(&logins).unwrap();
        rig.scenario(json!([{"fake":"fake-checks","steps":[{"do":"probe","spec":{"exists":[logins,rig.fhome().join("login-home")],"writable":["$HOME"]}},{"do":"exit","code":1}]}]));
        rig.start();
        let seen = || {
            let job = rig.jobs("ex-1/check").into_iter().next()?;
            rig.out_lines(&job).into_iter().find(|v| v["type"] == "probe")
        };
        wait_until("the check job's wall probe", LONG, || seen().is_some());
        let report = seen().unwrap();
        assert!(report["exists"].as_object().unwrap().values().all(|v| v == false), "{report}");
        assert_eq!(report["writable"]["$HOME"], "WROTE", "{report}");
    }

    // The train's HOME is the shared agent home, so its advertised browser path takes precedence.
    fn installed_browsers() -> std::path::PathBuf {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let mut at: Vec<_> =
            std::env::var_os(mork_engine::jobs::wall::BROWSERS_ENV).map(std::path::PathBuf::from).into_iter().collect();
        at.extend(mork_engine::paths::Home::from_env().iter().map(|h| h.browsers()));
        at.extend([home.join(".cache/ms-playwright"), home.join(".local/share/mork/agent-home/.cache/ms-playwright")]);
        at.iter().find(|p| p.is_dir()).cloned().unwrap_or_else(|| panic!("no installed browsers, looked in {at:?}"))
    }

    // fac-114: the installed browsers, outside the writable agent home, are read-only with the
    // folders above them, and the real browser there renders a page into a writable output.
    #[test]
    fn t1_57_the_installed_browsers_are_read_only() {
        if common::wall_test("T1.57") {
            return;
        }
        let mut rig = Rig::new();
        let home = mork_engine::paths::Home::new(rig.fhome());
        let installed = home.browsers();
        assert!(!installed.starts_with(home.agent_home()), "the browsers are in the writable agent home");
        let source = installed_browsers();
        let shell = std::fs::read_dir(&source)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().starts_with("chromium_headless_shell-"))
            .expect("no headless shell in the installed browsers")
            .file_name();
        std::fs::create_dir_all(&installed).unwrap();
        let copied =
            std::process::Command::new("cp").arg("-a").arg(source.join(&shell)).arg(&installed).status().unwrap();
        assert!(copied.success(), "the browser wasn't copied into the test's home");
        // The source may be read-only by mode; the wall assertions must test the mount, not the modes.
        let shell_dir = installed.join(&shell);
        let opened = std::process::Command::new("chmod").arg("-R").arg("u+w").arg(&shell_dir).status().unwrap();
        assert!(opened.success(), "the copied browser couldn't be made writable");
        for dir in [&installed, &shell_dir] {
            let probe = dir.join("precondition");
            std::fs::write(&probe, b"x").expect("the fixture isn't writable outside the wall");
            std::fs::remove_file(&probe).unwrap();
        }
        let binary = installed.join(&shell).join("chrome-headless-shell-linux64/chrome-headless-shell");
        let shot = "$IO/shot.png";
        rig.start();
        let path = |p: &std::path::Path| p.display().to_string();
        let page = "data:text/html,<h1 style='color:red'>fac-114</h1>";
        let root = home.root().to_path_buf();
        let r = probe(
            &mut rig,
            json!({
                "writable": [path(&installed), path(&shell_dir), path(&root), path(&home.agent_home()), "/tmp"],
                "exec": [[path(&binary), "--no-sandbox", "--disable-gpu", "--user-data-dir=/tmp/profile",
                    &format!("--screenshot={shot}"), "--window-size=400,300", page]],
            }),
        );
        for ro in [&installed, &shell_dir, &root] {
            assert_ne!(r["writable"][path(ro)], "WROTE", "{} is writable: {r}", ro.display());
        }
        assert_ne!(r["writable"][path(&home.agent_home())], "WROTE", "{r}");
        assert_eq!(r["writable"]["/tmp"], "WROTE", "{r}");
        let keys = r["env_keys"].as_array().unwrap();
        assert!(keys.iter().any(|k| k == mork_engine::jobs::wall::BROWSERS_ENV), "no browsers variable: {r}");
        assert_eq!(r["exec"][0]["code"], 0, "the browser didn't render: {r}");
        let png = std::fs::read(seat_io(&rig).join("shot.png")).expect("no screenshot in the seat's io/");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
        assert_eq!((size(16), size(20)), (400, 300), "the screenshot isn't the window's size");
    }

    fn remove_peer_at_launch(rig: &mut Rig, peer: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir(peer).unwrap();
        let wrapper = rig.t.root.join("bwrap-finish-peer");
        std::fs::write(
            &wrapper,
            r#"#!/usr/bin/python3
import os, sys
args = sys.argv[1:]
if "ex-1/foreman" in args:
    peer = os.environ["PEER_CLONE"]
    assert peer in args, "peer clone missing from planned mounts"
    os.rmdir(peer)
os.execv("/usr/bin/bwrap", ["bwrap", *args])
"#,
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        rig.set_env("PEER_CLONE", peer.to_str().unwrap());
        rig.set_env("MORK_TEST_BWRAP", wrapper.to_str().unwrap());
    }

    // T1.33, M1.28: its own clone stays writable while the shared clones and data remain read-only.
    #[test]
    fn m1_28_a_card_turn_writes_its_clone() {
        if common::wall_test("M1.28") {
            return;
        }
        let mut rig = Rig::new();
        let work = listed_folder(&mut rig);
        let f = rig.fhome();
        std::fs::create_dir_all(f.join("clones/other-card")).unwrap();
        let finished = f.join("clones/finished-card");
        remove_peer_at_launch(&mut rig, &finished);
        rig.put_cards(vec![rig.card("ex-1", mork_types::card::Mode::Full)]);
        let clone = f.join("clones/ex-1");
        let roots = [work.join("data"), std::path::PathBuf::from("/data")];
        let read: Vec<_> =
            roots.iter().flat_map(|p| HIDDEN_NAMES.iter().chain(PUBLIC_NAMES).map(|n| p.join(n))).collect();
        let spec = json!({
            "read": read,
            "exists": [finished, f.join("clones/other-card"), roots[0].join("ACCESS-private/hidden"), roots[1].join("ACCESS-private/hidden")],
            "writable": [clone.clone(), f.join("agent-home"), f.join("clones/other-card"), roots[1].clone()],
        });
        rig.scenario(json!([{"agent": "ex-1/foreman", "steps": [{"do": "probe", "spec": spec}, {"do": "hang"}]}]));
        rig.start();
        rig.say("ex-1", "Go.");
        let line = || {
            let job = rig.jobs("ex-1/foreman").into_iter().next()?;
            rig.out_lines(&job).into_iter().find(|v| v["type"] == "probe")
        };
        wait_until("the probe", LONG, || line().is_some());
        let r = line().unwrap();
        assert_eq!(r["writable"][clone.display().to_string()], "WROTE", "{r}");
        assert!(!finished.exists(), "the peer must be removed at launch");
        assert_eq!(r["exists"][finished.display().to_string()], false, "{r}");
        assert_eq!(r["exists"][f.join("clones/other-card").display().to_string()], true, "{r}");
        assert_eq!(r["writable"][f.join("clones/other-card").display().to_string()], "EROFS", "{r}");
        assert_eq!(r["writable"]["/data"], "EROFS", "{r}");
        for root in roots {
            assert_listed(&r, &root);
        }
        rig.send(json!({"stop": {"card": "ex-1"}}));
        rig.wait_settled(LONG);
    }

    // T1.33, fac-323: a report card's foreman writes its handoff folder, nothing else of the clone.
    #[test]
    fn t1_33_a_report_foreman_writes_only_its_handoff() {
        if common::wall_test("T1.33") {
            return;
        }
        handoff_wall(mork_types::card::Mode::Report, "ex-1/foreman");
    }

    #[test]
    fn t1_33_a_planner_writes_only_its_handoff() {
        if common::wall_test("T1.33 planner") {
            return;
        }
        handoff_wall(mork_types::card::Mode::Full, "ex-1/planner");
    }

    fn handoff_wall(mode: mork_types::card::Mode, agent: &str) {
        let mut rig = Rig::new();
        let clone = rig.fhome().join("clones/ex-1");
        std::fs::create_dir_all(clone.join(".git")).unwrap();
        std::fs::write(clone.join("source"), "the source").unwrap();
        let mut card = rig.card("ex-1", mode);
        if mode == mork_types::card::Mode::Full {
            card.stage = mork_types::card::Stage::Brief;
            card.approval = None;
        }
        rig.put_cards(vec![card]);
        let at = |rel: &str| clone.join(rel).display().to_string();
        let spec = json!({
            "read": [at("source")],
            "writable": [at(".git/mork/handoff"), at(""), at(".git"), at(".git/mork")],
        });
        rig.scenario(json!([{"agent": agent, "steps": [{"do": "probe", "spec": spec}, {"do": "hang"}]}]));
        rig.start();
        // A full card's planner hears the operator in its spec room, a report card's foreman in its room (fac-633).
        rig.say(if agent.ends_with("/planner") { "ex-1/spec" } else { "ex-1" }, "Go.");
        let line = || {
            let job = rig.jobs(agent).into_iter().next()?;
            rig.out_lines(&job).into_iter().find(|v| v["type"] == "probe")
        };
        wait_until("the probe", LONG, || line().is_some());
        let r = line().unwrap();
        assert_eq!(r["read"][at("source")], "READ the source", "{r}");
        assert_eq!(r["writable"][at(".git/mork/handoff")], "WROTE", "{r}");
        for rel in ["", ".git", ".git/mork"] {
            assert_ne!(r["writable"][at(rel)], "WROTE", "{rel:?} is writable: {r}");
        }
        rig.send(json!({"stop": {"card": "ex-1"}}));
        rig.wait_settled(LONG);
    }

    // fac-591, fac-636: the planner, its card's mock open at brief or reopened at build, writes its handoff and the
    // mock, nothing else of the clone.
    #[test]
    fn t1_33_a_planner_writes_only_its_handoff_and_open_mock() {
        if common::wall_test("T1.33 mock") {
            return;
        }
        for stage in [mork_types::card::Stage::Brief, mork_types::card::Stage::Build] {
            planner_mock_wall(stage);
        }
    }

    fn planner_mock_wall(stage: mork_types::card::Stage) {
        let mut rig = Rig::new();
        let clone = rig.fhome().join("clones/ex-1");
        std::fs::create_dir_all(clone.join(".git/mork-mock-host")).unwrap();
        let mut card = rig.card("ex-1", mork_types::card::Mode::Full);
        card.visual = true;
        if stage == mork_types::card::Stage::Brief {
            card.stage = stage;
            card.approval = None;
        }
        card.mock = Some(mork_types::card::Mock {
            step: mork_types::card::MockStep::Open,
            ..mork_types::card::Mock::default()
        });
        rig.put_cards(vec![card]);
        std::fs::write(clone.join("source"), "the source").unwrap();
        let at = |rel: &str| clone.join(rel).display().to_string();
        let spec = json!({
            "read": [at("source")],
            "writable": [
                at(".git/mork/handoff"),
                at(".git/mork-mock"),
                at(""),
                at(".git"),
                at(".git/mork-mock-host"),
            ],
        });
        rig.scenario(json!([{"agent": "ex-1/planner", "steps": [{"do": "probe", "spec": spec}, {"do": "hang"}]}]));
        rig.start();
        // Unnamed words in the spec room reach the planner (fac-633).
        rig.say("ex-1/spec", "Go.");
        let line = || {
            let job = rig.jobs("ex-1/planner").into_iter().next()?;
            rig.out_lines(&job).into_iter().find(|v| v["type"] == "probe")
        };
        wait_until("the planner's wall probe", LONG, || line().is_some());
        let r = line().unwrap();
        assert_eq!(r["read"][at("source")], "READ the source", "{stage:?}: {r}");
        for rel in [".git/mork/handoff", ".git/mork-mock"] {
            assert_eq!(r["writable"][at(rel)], "WROTE", "{stage:?}, {rel:?}: {r}");
        }
        for rel in ["", ".git", ".git/mork-mock-host"] {
            assert_eq!(r["writable"][at(rel)], "EROFS", "{stage:?}, {rel:?}: {r}");
        }
        rig.send(json!({"stop": {"card": "ex-1"}}));
        rig.wait_settled(LONG);
    }

    // fac-636: a visual card's baseline seat, its foreman's child at the baseline step, writes the mock and nothing
    // else of the clone, its planner's handoff included.
    #[test]
    fn t1_33_a_baseline_seat_writes_only_the_mock() {
        if common::wall_test("T1.33 baseline") {
            return;
        }
        let mut rig = Rig::new();
        let clone = rig.fhome().join("clones/ex-1");
        std::fs::create_dir_all(clone.join(".git/mork/handoff")).unwrap();
        let mut card = rig.card("ex-1", mork_types::card::Mode::Full);
        card.visual = true;
        card.stage = mork_types::card::Stage::Brief;
        card.approval = None;
        card.mock = Some(mork_types::card::Mock {
            step: mork_types::card::MockStep::Baseline,
            ..mork_types::card::Mock::default()
        });
        rig.put_cards(vec![card]);
        std::fs::write(clone.join("source"), "the source").unwrap();
        let at = |rel: &str| clone.join(rel).display().to_string();
        let spec = json!({
            "read": [at("source")],
            "writable": [at(".git/mork-mock"), at(""), at(".git"), at(".git/mork/handoff")],
        });
        let spawn = json!({"do": "ctl", "args": ["spawn", "ex-1", "baseline", "Reproduce today's pages."]});
        rig.scenario(json!([
            {"agent": "ex-1/foreman", "steps": [spawn, {"do": "hang"}]},
            {"agent": "ex-1/baseline-1", "steps": [{"do": "probe", "spec": spec}, {"do": "hang"}]},
        ]));
        rig.start();
        rig.say("ex-1", "Go.");
        let line = || {
            let job = rig.jobs("ex-1/baseline-1").into_iter().next()?;
            rig.out_lines(&job).into_iter().find(|v| v["type"] == "probe")
        };
        wait_until("the baseline seat's wall probe", LONG, || line().is_some());
        let r = line().unwrap();
        assert_eq!(r["read"][at("source")], "READ the source", "{r}");
        assert_eq!(r["writable"][at(".git/mork-mock")], "WROTE", "{r}");
        for rel in ["", ".git", ".git/mork/handoff"] {
            assert_eq!(r["writable"][at(rel)], "EROFS", "{rel:?}: {r}");
        }
        rig.send(json!({"stop": {"card": "ex-1"}}));
        rig.wait_settled(LONG);
    }

    #[test]
    fn t1_33_a_mock_host_writes_only_its_host_folder() {
        if common::wall_test("T1.33") {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let mut rig = Rig::new();
        let clone = rig.fhome().join("clones/ex-1");
        std::fs::create_dir_all(clone.join(".git/mork-mock")).unwrap();
        let mut card = rig.card("ex-1", mork_types::card::Mode::Full);
        card.mock = Some(mork_types::card::Mock {
            step: mork_types::card::MockStep::Approved,
            // The fake host probes its wall without binding a port.
            port: Some(1),
            ..mork_types::card::Mock::default()
        });
        rig.put_cards(vec![card]);
        std::fs::write(clone.join("source"), "the source").unwrap();
        let at = |rel: &str| clone.join(rel).display().to_string();
        let spec = json!({
            "read": [at("source")],
            "writable": [at(".git/mork-mock-host"), at(""), at(".git"), at(".git/mork-mock")],
        });
        let vite = rig.t.bin().join("vite");
        let bin = vite.join("node_modules/.bin/vite");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        let script = format!("#!/bin/sh\nexec probe {}\n", mork_engine::jobs::door_ssh::quoted(&spec.to_string()));
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cfg = rig.config("").replace("[mork]\n", &format!("[mork]\nvite = {:?}\n", vite.display().to_string()));
        rig.write_config(&cfg);
        rig.start();
        let line = || {
            let log = std::fs::read_to_string(clone.join(mork_core::lifecycle::mock::LOG)).ok()?;
            log.lines().filter_map(|s| serde_json::from_str::<Value>(s).ok()).find(|v| v["type"] == "probe")
        };
        wait_until("the mock host's wall probe", LONG, || line().is_some());
        let r = line().unwrap();
        assert_eq!(r["read"][at("source")], "READ the source", "{r}");
        assert_eq!(r["writable"][at(".git/mork-mock-host")], "WROTE", "{r}");
        for rel in ["", ".git", ".git/mork-mock"] {
            assert_eq!(r["writable"][at(rel)], "EROFS", "{rel:?}: {r}");
        }
    }

    fn outside_listeners() -> (String, String, std::net::TcpListener, impl Sized) {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::{SocketAddr, UnixDatagram, UnixListener};
        // T1.34 and T1.38 share the process: a per-call number keeps their abstract names apart (fac-627).
        static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tag = format!("mork-test-{}-{n}", std::process::id());
        let stream_name = format!("{tag}-stream");
        let dgram_name = format!("{tag}-dgram");
        let s = UnixListener::bind_addr(&SocketAddr::from_abstract_name(stream_name.as_bytes()).unwrap()).unwrap();
        let d = UnixDatagram::bind_addr(&SocketAddr::from_abstract_name(dgram_name.as_bytes()).unwrap()).unwrap();
        let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        (stream_name, dgram_name, tcp, (s, d))
    }

    // These peers share the wall's network namespace but predate wall-exec's Landlock domain.
    fn abstract_peers(rig: &mut Rig) {
        use std::os::unix::fs::PermissionsExt;
        let wrapper = rig.t.bin().join("bwrap-with-peers");
        std::fs::write(
            &wrapper,
            r#"#!/usr/bin/python3
import os, socket, subprocess, sys
args = sys.argv[1:]
if args[0] != "peers":
    cut = args.index("--") + 1
    os.execv("/usr/bin/bwrap", ["bwrap", *args[:cut], __file__, "peers", *args[cut:]])
stream = socket.socket(socket.AF_UNIX)
stream.bind(bytes([0]) + b"wall-stream")
stream.listen()
dgram = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
dgram.bind(bytes([0]) + b"wall-dgram")
sys.exit(subprocess.run(args[1:], pass_fds=(4,)).returncode)
"#,
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        rig.set_env("MORK_TEST_BWRAP", wrapper.to_str().unwrap());
    }

    fn sockets_and_ports(abi_override: Option<&str>) -> Result<(), String> {
        let abi = abi_override.and_then(|a| a.parse::<i64>().ok()).unwrap_or(mork_sys::landlock_abi().0);
        if abi < 6 {
            return Err(format!("T1.34: Landlock ABI {abi} is below 6; it runs at the live check on Ubuntu"));
        }
        let (stream, dgram, tcp, _keep) = outside_listeners();
        let mut rig = Rig::new();
        abstract_peers(&mut rig);
        let off = format!("127.0.0.1:{}", tcp.local_addr().unwrap().port());
        let external = "192.0.2.1:443";
        let loopback = "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); c=socket.create_connection(s.getsockname()); p,_=s.accept(); c.sendall(b'x'); assert p.recv(1)==b'x'";
        let r = probe(
            &mut rig,
            json!({"abstract_stream": [stream,"wall-stream"], "abstract_dgram": [dgram,"wall-dgram"],
            "tcp": [off,external], "exec": [["python3","-c",loopback]]}),
        );
        assert_eq!(r["abstract_stream"]["wall-stream"], "EPERM", "{r}");
        assert_eq!(r["abstract_dgram"]["wall-dgram"], "EPERM", "{r}");
        assert_ne!(r["abstract_stream"][&stream], "CONNECTED", "{r}");
        assert_ne!(r["abstract_dgram"][&dgram], "CONNECTED", "{r}");
        // The private proxy can have the same port number. Only a connection queued here reached the machine.
        assert!(matches!(r["tcp"][&off].as_str(), Some("ECONNREFUSED" | "CONNECTED")), "{r}");
        tcp.set_nonblocking(true).unwrap();
        assert_eq!(tcp.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(r["tcp"][external], "ENETUNREACH", "{r}");
        assert_eq!(r["exec"][0]["code"], 0, "{r}");
        Ok(())
    }

    #[test]
    fn t1_34_sockets_and_ports() {
        if common::wall_test("T1.34") {
            return;
        }
        // The ABI as the engine reads it, its test override included (1.10.6): its skip is checked with no wall.
        let abi = std::env::var("MORK_TEST_LANDLOCK_ABI").ok();
        if let Err(why) = sockets_and_ports(abi.as_deref()) {
            common::say_skipped(&why);
        }
    }

    #[test]
    fn t1_35_descriptors_and_privileges() {
        if common::wall_test("T1.35") {
            return;
        }
        let mut rig = Rig::new();
        let leak = rig.t.root.join("leak-me");
        std::fs::write(&leak, "a dummy secret").unwrap();
        rig.set_env("MORK_TEST_LEAK_FD", &leak.display().to_string());
        rig.scenario(
            json!([{"fake": "claude", "steps": [{"do": "init"}, {"do": "probe", "spec": {}}, {"do": "hang"}]}]),
        );
        rig.start();
        rig.echo("probe");
        wait_until("the probe", LONG, || !rig.jobs("main/echo").is_empty() && probe_line(&rig).is_some());
        let r = probe_line(&rig).unwrap();
        // The leak is real: the supervisor holds the file open while the job runs.
        let dir = mork_engine::jobs::folder::JobDir::new(job_dir(&rig));
        let sup = dir.claim().unwrap().unwrap().pid;
        let held: Vec<String> = std::fs::read_dir(format!("/proc/{sup}/fd"))
            .unwrap()
            .flatten()
            .filter_map(|e| std::fs::read_link(e.path()).ok().map(|p| p.display().to_string()))
            .collect();
        // The fd links come back resolved, the temporary folder's alias among them.
        let leak = std::fs::canonicalize(&leak).unwrap();
        assert!(
            held.iter().any(|p| p == &leak.display().to_string()),
            "the supervisor doesn't hold the leak: {held:?}"
        );
        rig.send(json!({"stop": {"agent": "main/echo"}}));
        rig.wait_settled(LONG);
        let fds: Vec<&str> = r["fds"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(fds, ["0", "1", "2"], "{r}");
        let pids = r["pids"].as_array().unwrap();
        assert!(pids.iter().all(|p| p.as_u64().unwrap() < 50), "the host's /proc: {pids:?}");
        let status = r["status"].to_string();
        assert!(status.contains("NoNewPrivs:\\t1"), "{status}");
        assert!(status.contains("CapEff:\\t0000000000000000"), "{status}");
        assert_ne!(r["unshare_user"]["code"], 0, "{r}");
        assert_ne!(r["mount"]["code"], 0, "{r}");
    }

    #[test]
    fn wall_syscall_filter() {
        if common::wall_test("seccomp") {
            return;
        }
        let mut rig = Rig::new();
        let source = rig.t.bin().join("wall-canary.c");
        std::fs::write(
            &source,
            r#"
#include <errno.h>
#include <stdio.h>
#include <sys/syscall.h>
#include <unistd.h>

int main(void) {
    long pid = syscall(SYS_getpid);
    if (pid <= 0 || pid != getpid()) return 1;
    const long denied[] = {
        SYS_io_uring_setup, SYS_userfaultfd, SYS_ptrace,
#ifdef __x86_64__
        __X32_SYSCALL_BIT | SYS_getpid,
#endif
    };
    for (unsigned i = 0; i < sizeof(denied) / sizeof(denied[0]); ++i) {
        errno = 0;
        long rc = syscall(denied[i], -1L, 0L, 0L, 0L);
        if (rc != -1 || errno != EPERM) {
            fprintf(stderr, "syscall %ld returned %ld, errno %d\n", denied[i], rc, errno);
            return 1;
        }
    }
    return 0;
}
"#,
        )
        .unwrap();
        let mut commands = vec![json!(["cc", source, "-o", "/tmp/wall-canary"])];
        #[cfg(target_arch = "x86_64")]
        if let Some(binary) = i386_canary(&rig) {
            commands.push(json!([binary]));
        }
        commands.push(json!(["/tmp/wall-canary"]));
        let result = probe(&mut rig, json!({"exec": commands}));
        for (i, command) in commands.iter().enumerate() {
            assert_eq!(result["exec"][i]["code"], 0, "{command}: {}", result["exec"][i]);
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn i386_canary(rig: &Rig) -> Option<std::path::PathBuf> {
        // A static fixture needs no i386 libc or loader, even on hosts with only native toolchains.
        let binary = rig.t.bin().join("i386-canary");
        let source = binary.with_extension("s");
        std::fs::write(
            &source,
            r#"
.global _start
_start:
    cmpl $1, (%esp)
    jne success
    mov $20, %eax        # i386 getpid
    int $0x80
    test %eax, %eax
    jle failed
    mov $-1, %ebx        # invalid entries, flags or ptrace request
    xor %ecx, %ecx
    xor %edx, %edx
    xor %esi, %esi
.macro denied nr
    mov $\nr, %eax
    int $0x80
    cmp $-1, %eax        # raw -EPERM
    jne failed
.endm
    denied 425           # io_uring_setup
    denied 374           # userfaultfd
    denied 26            # ptrace
success:
    xor %ebx, %ebx
    jmp exit
failed:
    mov $1, %ebx
exit:
    mov $1, %eax         # i386 exit
    int $0x80
"#,
        )
        .unwrap();
        let built = std::process::Command::new("cc")
            .args(["-m32", "-nostdlib", "-static", "-o"])
            .arg(&binary)
            .arg(&source)
            .status()
            .unwrap();
        assert!(built.success(), "could not build the static i386 canary");
        // Only an unsupported executable format outside the wall excuses the i386 assertions inside.
        match std::process::Command::new(&binary).arg("check-format").status() {
            Err(e) if e.raw_os_error() == Some(8) => None,
            status => {
                assert!(status.unwrap().success(), "i386 format check failed");
                Some(binary)
            }
        }
    }

    #[test]
    fn t1_38_fail_closed() {
        if common::wall_test("T1.38") {
            return;
        }
        for (k, v) in [("MORK_TEST_BWRAP", "/nonexistent/bwrap"), ("MORK_TEST_LANDLOCK_ABI", "5")] {
            let mut rig = Rig::new();
            rig.set_env(k, v);
            rig.start();
            rig.echo("go");
            wait_until("the agent to show why", LONG, || {
                rig.store().load().unwrap().agents.values().any(|a| {
                    matches!(&a.status,
                    mork_types::state::AgentStatus::Error { words } if words.contains("no wall"))
                })
            });
            assert!(rig.runs("claude").is_empty(), "{k}: a turn ran");
        }
        // Partial, at ABI 5: the wall starts with its mounts, with no scoping (an abstract
        // socket in the same namespace answers), and T1.34 skips.
        assert!(sockets_and_ports(Some("5")).is_err_and(|w| w.contains("below 6")));
        let (stream, _dgram, _tcp, _keep) = outside_listeners();
        let mut rig = Rig::new();
        rig.set_env("MORK_TEST_WALL", "partial");
        rig.set_env("MORK_TEST_LANDLOCK_ABI", "5");
        abstract_peers(&mut rig);
        let state = rig.fhome().join("state.db");
        let r = probe(
            &mut rig,
            json!({"exists": [state.display().to_string()], "abstract_stream": [stream,"wall-stream"]}),
        );
        assert_eq!(r["exists"][state.display().to_string()], false, "{r}");
        assert_eq!(r["abstract_stream"]["wall-stream"], "CONNECTED", "{r}");
        assert_ne!(r["abstract_stream"][&stream], "CONNECTED", "{r}");
    }

    // fac-654: a turn sees and writes its seat's own `io/` alone, the one every turn of the seat has: no other
    // seat's, no job's records and no job's `io/`, its own included.
    #[test]
    fn t1_39_only_io() {
        if common::wall_test("T1.39") {
            return;
        }
        let mut rig = Rig::new();
        rig.start();
        // The job's own folder: its files are predictable once the first job ran.
        rig.echo("first");
        rig.wait_settled(LONG);
        let other = rig.fhome().join("io/20200101-000000-other-0000");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("x.png"), "OTHER").unwrap();
        let jobs = rig.fhome().join("jobs");
        // Every job folder there is, this probe's own included once it runs: none of their
        // records is visible, and none of the folders is writable.
        let mut exists = vec![other.join("x.png").display().to_string()];
        let mut writable = vec![other.display().to_string()];
        for e in std::fs::read_dir(&jobs).unwrap().flatten() {
            for f in ["lock", "claim.json", "run.json", "exit.json", "job.json", "input", "empty", "io"] {
                exists.push(e.path().join(f).display().to_string());
            }
            writable.extend([e.path(), e.path().join("io")].map(|p| p.display().to_string()));
        }
        let r = probe(&mut rig, json!({"exists": exists, "writable": writable}));
        for p in r["exists"].as_object().unwrap() {
            assert_eq!(p.1, false, "{} is visible inside", p.0);
        }
        for w in r["writable"].as_object().unwrap() {
            assert_ne!(w.1, "WROTE", "{} is writable inside", w.0);
        }
        own_folders(&mut rig, &jobs);
    }

    // This job's own folder, seen from inside, shows nothing; its seat's, `$IO`, is there, writable, and the one
    // its first turn named.
    fn own_folders(rig: &mut Rig, jobs: &std::path::Path) {
        let own = format!("{}/$JOB", jobs.display());
        let mut files: Vec<String> =
            ["lock", "claim.json", "run.json", "exit.json", "job.json", "stdin", "out.jsonl", "input", "empty", "io"]
                .iter()
                .map(|f| format!("{own}/{f}"))
                .collect();
        files.push("$IO".into());
        let r = probe(rig, json!({"exists": files, "writable": [own.clone(), format!("{own}/io"), "$IO".to_owned()]}));
        let (io, records) = files.split_last().unwrap();
        for f in records {
            assert_eq!(r["exists"][f], false, "{f} is visible inside: {r}");
        }
        assert_eq!(r["exists"][io], true, "{r}");
        assert_ne!(r["writable"][&own], "WROTE", "{r}");
        assert_ne!(r["writable"][format!("{own}/io")], "WROTE", "{r}");
        assert_eq!(r["writable"]["$IO"], "WROTE", "{r}");
        let turns = rig.jobs("main/echo");
        let seat = rig.fhome().join("io").join(turns[0].id.as_str()).display().to_string();
        for j in &turns {
            assert_eq!(j.spec.env.get("MORK_IO"), Some(&seat), "{} had another folder", j.id);
        }
    }

    // M0.1: a "wall" whose init shares the supervisor's pid namespace is refused, and
    // nothing else is signalled: a bystander in that namespace lives on.
    #[test]
    fn m0_1_a_wall_that_didnt_form() {
        if common::wall_test("M0.1") {
            return;
        }
        let mut rig = Rig::new();
        let fake = rig.t.root.join("fake-bwrap");
        std::fs::write(&fake, "#!/bin/sh\necho \"{\\\"child-pid\\\": $$}\" >&3\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        rig.set_env("MORK_TEST_BWRAP", &fake.display().to_string());
        let mut bystander = std::process::Command::new("sleep").arg("300").spawn().unwrap();
        rig.start();
        rig.echo("go");
        wait_until("the turn to fail", LONG, || rig.jobs("main/echo").first().is_some_and(|j| j.settled.is_some()));
        let dir = mork_engine::jobs::folder::JobDir::new(job_dir(&rig));
        let exit = dir.exit().unwrap().unwrap();
        assert!(exit.error.as_deref().is_some_and(|e| e.contains("didn't form")), "{exit:?}");
        assert!(exit.left.is_empty(), "{exit:?}");
        assert!(bystander.try_wait().unwrap().is_none(), "a bystander was killed");
        let _ = bystander.kill();
        let _ = bystander.wait();
    }

    #[test]
    fn t1_55_no_host_socket_outside_the_allowlist() {
        if common::wall_test("T1.55") {
            return;
        }
        let mut rig = Rig::new();
        let tools = rig.t.home().join("tools");
        std::fs::create_dir_all(tools.join("a")).unwrap();
        let sock = tools.join("b.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o777)).unwrap();
        let cfg = rig.config("").replacen(
            "toolchains = [",
            &format!("toolchains = [{{ path = \"{}\" }}, ", tools.join("a").display()),
            1,
        );
        rig.write_config(&cfg);
        let a = tools.join("a").display().to_string();
        let b = sock.display().to_string();
        let r = probe(&mut rig, json!({"exists": [a.clone(), b.clone()], "unix": [b.clone()]}));
        assert_eq!(r["exists"][&a], true, "the toolchain isn't there: {r}");
        assert_eq!(r["exists"][&b], false, "the socket beside it is visible: {r}");
        assert!(!r["unix"][&b].as_str().unwrap().starts_with("CONNECTED"), "{r}");
    }

    #[test]
    fn t1_56_the_sockets_a_wall_is_given() {
        if common::wall_test("T1.56") {
            return;
        }
        let mut rig = Rig::new();
        let f = rig.fhome();
        let roots = [
            rig.t.bin().display().to_string(),
            f.join("builds").display().to_string(),
            f.join("run").display().to_string(),
            rig.t.home().display().to_string(),
            rig.t.tmp().display().to_string(),
            rig.t.xdg().display().to_string(),
        ];
        // A socket planted in a bind the test owns proves the scan reaches inside the binds.
        let planted = rig.t.bin().join("planted.sock");
        let _planted = std::os::unix::net::UnixListener::bind(&planted).unwrap();
        // The resolver's file is there when the host has it; its folder's sockets never are.
        let resolver = mork_engine::jobs::wall::RESOLVER;
        let resolver_sockets =
            ["/run/systemd/resolve/io.systemd.Resolve", "/run/systemd/resolve/io.systemd.Resolve.Monitor"];
        let mut exists = vec![resolver.to_owned()];
        exists.extend(resolver_sockets.iter().map(|s| (*s).to_owned()));
        let r = probe(&mut rig, json!({"sockets": roots, "exists": exists}));
        assert_eq!(r["exists"][resolver], std::path::Path::new(resolver).exists(), "{r}");
        for s in resolver_sockets {
            assert_eq!(r["exists"][s], false, "{s} is visible in the wall: {r}");
        }
        let found: Vec<&str> = r["sockets"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        let mut want = vec![f.join("run/agent.sock").display().to_string(), planted.display().to_string()];
        want.sort();
        assert_eq!(found, want, "{r}");
        for path in &roots {
            assert_eq!(r["walked"][path], true, "{path} wasn't scanned: {r}");
        }
        let _ = std::fs::remove_file(&planted);
    }

    #[test]
    fn t1_53_mork_login() {
        if common::wall_test("T1.53") {
            return;
        }
        let rig = Rig::new();
        rig.scenario(json!([{"fake": "claude", "steps": [
            {"do": "show-env"}, {"do": "probe", "spec":{"exists":["$HOME/.claude/accounts/saved"],"writable":["$HOME/.claude/accounts"]}}, {"do": "write-file", "path": ".claude-login.json", "text": "{\"logged\":true}"}]}]));
        let saved = rig.fhome().join("login-home/.claude/accounts");
        std::fs::create_dir_all(&saved).unwrap();
        std::fs::write(saved.join("saved"), "dummy account").unwrap();
        // `script` gives the login a terminal, as the operator's would be.
        let cmdline = format!("{} login claude", common::exe().display());
        let status = std::process::Command::new("script")
            .args(["-qec", &cmdline, "/dev/null"])
            .stdin(std::process::Stdio::null())
            .envs(rig.t.env())
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
        let shown = String::from_utf8_lossy(&status.stdout);
        let env: serde_json::Value = shown
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
            .find(|v| v["type"] == "fake-env")
            .unwrap_or_else(|| panic!("no fake-env line: {shown}"));
        assert_eq!(env["env"]["HOME"], rig.fhome().join("login-home").display().to_string(), "{env}");
        let mask = shown
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
            .find(|v| v["type"] == "probe")
            .unwrap();
        assert_eq!(mask["exists"]["$HOME/.claude/accounts/saved"], false);
        assert_eq!(mask["writable"]["$HOME/.claude/accounts"], "EROFS");
        assert!(env["stdin"].as_str().unwrap().starts_with("/dev/pts/"), "no terminal attached: {env}");
        let runs = rig.runs("claude");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["cwd"], rig.fhome().join("login-home").display().to_string());
        let login: Vec<_> = rig.processes().into_iter().filter(|p| p["kind"] == "login").collect();
        assert!(!login.is_empty() && login.iter().all(|p| p["walled"] == true), "{login:?}");
        assert!(rig.fhome().join("login-home/.claude-login.json").exists(), "the login is missing from the login home");
        assert!(!rig.t.home().join(".claude-login.json").exists());
        let found = std::process::Command::new("find")
            .args([&rig.t.root.display().to_string(), "-name", ".claude-login.json"])
            .output()
            .unwrap();
        let found = String::from_utf8_lossy(&found.stdout);
        assert_eq!(found.lines().count(), 1, "the login is elsewhere too: {found}");
    }

    // fac-657: agy's sign-in runs as the others do, with no session bus by address or socket, so agy keeps its token
    // as a file there.
    #[test]
    fn t1_53_mork_login_agy() {
        if common::wall_test("T1.53, agy") {
            return;
        }
        let mut rig = Rig::new();
        let bus = rig.t.xdg().join("bus");
        let _bus = std::os::unix::net::UnixListener::bind(&bus).unwrap();
        rig.set_env("DBUS_SESSION_BUS_ADDRESS", &format!("unix:path={}", bus.display()));
        let token = ".gemini/antigravity-cli/antigravity-oauth-token";
        rig.scenario(json!([{"fake": "agy", "steps": [
            {"do": "show-env"}, {"do": "probe", "spec": {"unix": [bus]}},
            {"do": "sh", "script": format!("mkdir -p $(dirname {token}) && echo signed > {token}")}]}]));
        let cmdline = format!("{} login agy", common::exe().display());
        let mut script = rig.command_at("script", &["-qec", &cmdline, "/dev/null"]);
        let status = script.stdin(std::process::Stdio::null()).output().unwrap();
        assert!(status.status.success(), "{status:?}");
        let shown = String::from_utf8_lossy(&status.stdout);
        let line = |kind: &str| {
            shown
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
                .find(|v| v["type"] == kind)
                .unwrap_or_else(|| panic!("no {kind} line: {shown}"))
        };
        let env = line("fake-env");
        let probe = line("probe");
        let reached = probe["unix"][bus.display().to_string()].as_str().unwrap_or_else(|| panic!("{probe}"));
        assert!(!reached.starts_with("CONNECTED"), "the login's wall reached the host bus: {probe}");
        assert_eq!(env["env"]["HOME"], rig.fhome().join("login-home").display().to_string(), "{env}");
        assert!(env["env"].get("DBUS_SESSION_BUS_ADDRESS").is_none(), "{env}");
        assert_eq!(env["env"]["AGY_CLI_DISABLE_AUTO_UPDATE"], "true", "{env}");
        assert!(env["stdin"].as_str().unwrap().starts_with("/dev/pts/"), "no terminal attached: {env}");
        let runs = rig.runs("agy");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["argv"], json!(["agy", "acp", "--login"]), "{runs:?}");
        let login: Vec<_> = rig.processes().into_iter().filter(|p| p["kind"] == "login").collect();
        assert!(!login.is_empty() && login.iter().all(|p| p["walled"] == true), "{login:?}");
        assert!(rig.fhome().join("logins/agy/antigravity-oauth-token").exists(), "the shared token is missing");
        assert!(!rig.t.home().join(token).exists());
    }

    // fac-657: an agy seat's wall: Mork's skills read-only where agy reads them, its state folders writable, and
    // no session bus, by address or socket.
    #[test]
    fn t1_33_an_agy_seats_wall() {
        if common::wall_test("T1.33, agy") {
            return;
        }
        let mut rig = Rig::new();
        rig.write_config(&rig.config(AGY_ECHO));
        let bus = rig.t.xdg().join("bus");
        let _bus = std::os::unix::net::UnixListener::bind(&bus).unwrap();
        rig.set_env("DBUS_SESSION_BUS_ADDRESS", &format!("unix:path={}", bus.display()));
        let gemini = std::path::PathBuf::from("$HOME/.gemini");
        let state = gemini.join("antigravity-cli");
        let skills = std::path::PathBuf::from("$HOME/.agents/skills");
        let skill = skills.join("testing-in-a-slot/SKILL.md");
        let old_skill = gemini.join("config/skills/testing-in-a-slot/SKILL.md");
        let spec = json!({"exists": [skill, old_skill], "writable": [skills, state], "unix": [bus],
            "exec": [["mkdir", "-p", gemini.join("config/write-probe")]]});
        let r = probe_on(&mut rig, "agy", spec);
        assert!(!rig.runs("agy").is_empty(), "no agy turn ran: {r}");
        assert_eq!(r["exists"][skill.display().to_string()], true, "agy finds no skills: {r}");
        assert_eq!(r["exists"][old_skill.display().to_string()], false, "{r}");
        assert_eq!(r["writable"][skills.display().to_string()], "EROFS", "{r}");
        assert_eq!(r["writable"][state.display().to_string()], "WROTE", "{r}");
        assert_eq!(r["exec"][0]["code"], 0, "agy cannot write its config: {r}");
        assert!(!r["unix"][bus.display().to_string()].as_str().unwrap().starts_with("CONNECTED"), "{r}");
        assert!(!r["env_keys"].as_array().unwrap().iter().any(|k| k == "DBUS_SESSION_BUS_ADDRESS"), "{r}");
    }

    // fac-657: a turn whose wall is off never runs agy, whatever chose its seat: it fails naming the rule. No wall
    // starts, so it runs in either phase.
    #[test]
    fn an_unwalled_agy_turn_is_refused_at_launch() {
        let mut rig = Rig::new();
        rig.write_config(&rig.config(AGY_ECHO));
        rig.set_env("MORK_UNWALLED", "1");
        rig.start();
        rig.echo("go");
        let mut words = String::new();
        wait_until("the agent to show why", LONG, || {
            let state = rig.store().load().unwrap();
            let status = state.agents.get(&mork_types::AgentKey::new("main/echo")).map(|a| a.status.clone());
            let Some(mork_types::state::AgentStatus::Error { words: w }) = status else { return false };
            words = w;
            true
        });
        assert!(words.contains("agy") && words.contains("walled alone"), "{words}");
        assert!(rig.runs("agy").is_empty(), "an unwalled agy turn ran");
        assert!(rig.jobs("main/echo").is_empty(), "a job was made: {words}");
    }

    fn card_probe(rig: &Rig, spec: Value) -> Value {
        rig.scenario(json!([{"agent":"ex-1/foreman","steps":[{"do":"probe","spec":spec},{"do":"hang"}]}]));
        rig.say("ex-1", "Probe.");
        let line = || {
            let job = rig.jobs("ex-1/foreman").into_iter().next()?;
            rig.out_lines(&job).into_iter().find(|v| v["type"] == "probe")
        };
        wait_until("the card's wall probe", LONG, || line().is_some());
        let result = line().unwrap();
        rig.send(json!({"stop":{"card":"ex-1"}}));
        rig.wait_settled(LONG);
        result
    }

    #[test]
    fn wall_proxy_and_local_ports() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        if common::wall_test("wall proxy") {
            return;
        }
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let blocked = other.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut s, _) = server.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&s).read_line(&mut line).unwrap();
                writeln!(s, "received {}", line.trim()).unwrap();
            }
        });
        let mut rig = Rig::new();
        rig.write_config(
            &rig.config(&format!("host_ports = [{port}]\ndomains = [\"localhost:{port}\",\"localhost:{blocked}\"]")),
        );
        rig.put_cards(vec![rig.card("ex-1", mork_types::card::Mode::Full)]);
        rig.start();
        let proxy = "$HTTPS_PROXY";
        let result = card_probe(
            &rig,
            json!({"tcp_send":[
                {"addr":proxy,"tunnel":"denied.invalid:443","send":""},
                {"addr":proxy,"tunnel":format!("localhost:{blocked}"),"send":""},
                {"addr":proxy,"tunnel":format!("localhost:{port}"),"send":"tunnel\n"},
                {"addr":format!("127.0.0.1:{port}"),"send":"direct\n"}
            ]}),
        );
        assert_eq!(
            result["tcp_send"],
            json!(["HTTP/1.1 403 Forbidden", "HTTP/1.1 403 Forbidden", "received tunnel", "received direct"])
        );
        worker.join().unwrap();
    }

    #[test]
    fn fac671_web_bridge_key_and_port_are_hidden() {
        if common::wall_test("fac-671 web bridge") {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let mut rig = Rig::new();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let cfg =
            rig.config(&format!("host_ports = [{port}]")).replace("[mork]\n", &format!("[mork]\nweb_port = {port}\n"));
        rig.put_cards(vec![rig.card("ex-1", mork_types::card::Mode::Full)]);
        rig.write_config(&cfg);
        rig.start();

        let key_path = rig.fhome().join("web.key");
        let log_path = rig.fhome().join("logs/app.log");
        let bound = || {
            let log = std::fs::read_to_string(&log_path).ok()?;
            log.lines().find_map(|l| l.split("web bridge listening on http://").nth(1).map(str::to_owned))
        };
        wait_until("web bridge bound address in app.log", LONG, || bound().is_some());
        let addr = bound().unwrap();
        let key = mork_engine::web::key::read(&key_path).unwrap();
        assert_eq!(std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(!std::fs::read_to_string(&log_path).unwrap().contains(&key), "web.key leaked to app.log");

        let key_hex_bytes: String = key.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let spec = json!({
            "exists": [key_path.display().to_string()],
            "read": [key_path.display().to_string()],
            "search_hex": key_hex_bytes,
            "search_roots": ["$HOME", rig.fhome().join("logins").display().to_string(), rig.fhome().join("run").display().to_string()],
            "tcp_send": [{"addr": &addr, "send": "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n"}],
            "exec": [[common::exe().display().to_string(), "web"]],
        });
        let r = card_probe(&rig, spec);
        assert_eq!(r["exists"][key_path.display().to_string()], false, "{r}");
        assert!(!r["read"][key_path.display().to_string()].as_str().unwrap().starts_with("READ"), "{r}");
        assert_eq!(r["found"], json!([]), "key bytes found inside wall: {r}");
        // A same-number wall proxy returns 403 for GET. The fixture's web bridge has no assets and returns 404.
        assert!(matches!(r["tcp_send"][0].as_str(), Some("ECONNREFUSED" | "HTTP/1.1 403 Forbidden")), "{r}");
        assert_ne!(r["exec"][0]["code"], 0, "mork web succeeded inside wall: {r}");
        assert!(r["exec"][0]["err"].as_str().unwrap().contains("refused inside a wall"), "{r}");
    }
}

// Production-only (1.10.8): `MORK_UNWALLED` and `MORK_TEST_WALL` are ignored. A build is
// a walled job of a production build: it still runs fully walled.
#[cfg(not(feature = "faults"))]
#[test]
fn t1_38_fail_closed_production_ignores_test_settings() {
    if common::wall_test("T1.38, production-only") {
        return;
    }
    let mut rig = Rig::new();
    let repo = rig.t.root.join("mork-repo");
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git").args(args).current_dir(&repo).output().unwrap();
        assert!(o.status.success(), "{o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_owned()
    };
    git(&["init", "-q", "-b", "master"]);
    std::fs::write(repo.join("f"), "x").unwrap();
    git(&["add", "."]);
    git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "c"]);
    let commit = git(&["rev-parse", "HEAD"]);
    rig.set_env("MORK_UNWALLED", "1");
    rig.set_env("MORK_TEST_WALL", "partial");
    let out = rig.command(&["build", &commit]).output().unwrap();
    // A production binary reports its own commit, whatever the fake cargo's script sets, so
    // the build is refused at install (M1.2); what this test checks is that it ran walled.
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() || err.contains("the binary built reports commit"), "{out:?}");
    let procs = rig.processes();
    let build: Vec<_> = procs.iter().filter(|p| p["argv"].to_string().contains("cargo build")).collect();
    assert!(!build.is_empty());
    for p in build {
        assert_eq!(p["walled"], true, "{p}");
        assert!(!p["argv"].to_string().contains("--partial"), "{p}");
    }
}
