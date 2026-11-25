// These work whether or not a factory is running.

use std::time::Duration;

use factory_engine::jobs::context::Context;
use factory_engine::jobs::{folder, launch};
use factory_engine::paths::Home;
use factory_engine::store::{self, Store};
use factory_engine::{builds, clock, config, doctor, settings};
use serde_json::{Value, json};

use crate::cli::{bin, commit, dirty};
use crate::ctl;

fn fail(e: &(impl std::fmt::Display + ?Sized)) -> i32 {
    eprintln!("{e}");
    1
}

/// `factory version [--json]`: its commit, and its manifest (1.8.2).
pub fn version(rest: &[String]) -> i32 {
    let v = json!({
        "commit": commit(),
        "dirty": dirty(),
        "state_oldest": store::schema::OLDEST,
        "state_newest": store::schema::NEWEST,
        "protocol": factory_types::protocol::PROTOCOL,
        "job_formats": factory_types::job::JOB_FORMATS,
        "reply_schemas": [factory_types::reply::SCHEMA_VERSION],
        "home": Home::from_env().map(|h| h.root().display().to_string()),
        "config": config::path().map(|p| p.display().to_string()),
    });
    if rest.iter().any(|a| a == "--json") {
        println!("{v}");
    } else {
        println!("factory {}{}", commit(), if dirty() { " (dirty)" } else { "" });
    }
    0
}

fn context(home: &Home) -> Result<Context, String> {
    let path = config::path().ok_or("no config path")?;
    let config = config::read(&path)?;
    let telemetry =
        std::env::var_os("HOME").map(|h| settings::read_telemetry(std::path::Path::new(&h))).unwrap_or_default();
    Ok(Context::new(home.clone(), config, bin(), telemetry))
}

/// `factory build <commit>` (1.8.3): through the engine when one runs, else here, recording
/// nothing in the state.
pub fn build(home: &Home, rest: &[String]) -> i32 {
    let Some(commit) = rest.first() else { return fail("factory build <commit>") };
    match ctl::request(home, &json!({"build": {"commit": commit}})) {
        Ok(Some(ok)) => return wait_for_build(home, &ok),
        Err(e) => return fail(&e),
        Ok(None) => {}
    }
    let ctx = match context(home) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    for d in [home.jobs(), home.builds(), home.agent_home(), home.run_dir()] {
        let _ = std::fs::create_dir_all(d);
    }
    let spec = match builds::prepare(&ctx, commit, clock::now_ms()) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let dir = match folder::write(&home.jobs(), &folder::NewFolder { spec: &spec, stdin: b"", io_files: &[] }) {
        Ok(d) => d,
        Err(e) => return fail(&e.to_string()),
    };
    if let Err(e) = launch::start_supervisor(&bin(), &dir.dir, home.root(), true) {
        return fail(&e.to_string());
    }
    while !dir.exit_path().exists() || dir.lock_held() {
        clock::sleep(Duration::from_millis(100));
    }
    let exit = dir.exit().ok().flatten();
    let err = std::fs::read_to_string(dir.err()).unwrap_or_default();
    let result = builds::finish(home, &spec, exit.is_some_and(|e| e.code == Some(0)), &err);
    let _ = std::fs::remove_dir_all(&dir.dir);
    match result {
        Ok(()) => 0,
        Err(e) => fail(&format!("the build failed: {e}")),
    }
}

fn wait_for_build(home: &Home, ok: &Value) -> i32 {
    let commit = ok["build"]["commit"].as_str().unwrap_or_default().to_owned();
    let job = ok["build"]["job"].as_str().unwrap_or_default().to_owned();
    loop {
        let Ok(Some(st)) = ctl::request(home, &json!({"status": {"jobs": true, "all": true}})) else {
            return fail("the factory went away");
        };
        let settled = st["status"]["jobs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|j| j["id"] == job.as_str() && j["settled"] == true);
        if settled {
            return if builds::binary(home, &commit).exists() { 0 } else { fail("the build failed; see main") };
        }
        clock::sleep(Duration::from_millis(200));
    }
}

/// `factory pin <commit>`: through the engine, or, with none running, after the checks a
/// reload makes, written here.
pub fn pin(home: &Home, rest: &[String]) -> i32 {
    let Some(commit) = rest.first() else { return fail("factory pin <commit>") };
    match ctl::request(home, &json!({"pin": {"commit": commit}})) {
        Ok(Some(ok)) => {
            println!("{ok}");
            return 0;
        }
        Err(e) => return fail(&e),
        Ok(None) => {}
    }
    let ctx = match context(home) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let (version, unsettled) = match state_for_checks(home) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let checked = builds::check_target(home, &ctx.config, commit, version, &unsettled, false);
    match checked.and_then(|full| {
        builds::write_pin(home, &full, "andrei")
            .map(|()| full)
            .map_err(|source| builds::BuildError::Io { what: "the pin".into(), source })
    }) {
        Ok(full) => {
            println!("{}", json!({"pin": {"commit": full}}));
            0
        }
        Err(e) => fail(&e),
    }
}

// Read-only; a home with no state yet is an empty one at the oldest version (M1 self-review, 2).
fn state_for_checks(home: &Home) -> Result<(u32, factory_core::maintenance::reload::Unsettled), String> {
    if !home.state_db().exists() {
        return Ok((store::schema::OLDEST, factory_core::maintenance::reload::Unsettled::default()));
    }
    let store = Store::open_read_only(&home.state_db()).map_err(|e| e.to_string())?;
    let state = store.load().map_err(|e| e.to_string())?;
    Ok((store.version, builds::unsettled(&state)))
}

/// `factory status`: one line per live card; through the socket when an app runs, else from
/// the state opened read-only.
pub fn status(home: &Home) -> i32 {
    match ctl::request(home, &json!({"status": {}})) {
        Ok(Some(ok)) => {
            for l in ok["status"]["lines"].as_array().into_iter().flatten() {
                println!("{}", l.as_str().unwrap_or_default());
            }
            0
        }
        Err(e) => fail(&e),
        Ok(None) if !home.state_db().exists() => 0,
        Ok(None) => match Store::open_read_only(&home.state_db()).and_then(|s| s.load()) {
            Ok(state) => {
                for l in factory_core::views::status::status_lines(&state) {
                    println!("{l}");
                }
                0
            }
            Err(e) => fail(&e.to_string()),
        },
    }
}

/// `factory doctor` (1.9): one line per finding; exit 1 with findings.
pub fn doctor(home: &Home) -> i32 {
    let mut found = Vec::new();
    let (jobs, state) = match ctl::request(home, &json!({"status": {"jobs": true, "all": true}})) {
        Ok(Some(ok)) => {
            let mut seen: doctor::JobsSeen = ok["status"]["jobs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|j| {
                    (
                        j["id"].as_str().unwrap_or_default().to_owned(),
                        j["settled_ms"].as_i64().filter(|_| j["settled"] == true),
                    )
                })
                .collect();
            // Online, `status` names only the jobs still of use; the state names every one (M5.30).
            let state = doctor::read_state(home);
            let store = Store::open_read_only(&home.state_db()).ok();
            for (id, settled) in store.as_ref().and_then(doctor::every_job).unwrap_or_default() {
                seen.entry(id).or_insert(settled);
            }
            (Some(seen), state)
        }
        _ => doctor::from_file(home, &mut found),
    };
    found.extend(doctor::findings(home, jobs.as_ref(), state.as_ref(), clock::now_ms(), &std::env::temp_dir()));
    for f in &found {
        println!("{f}");
    }
    i32::from(!found.is_empty())
}

/// `factory login claude|codex|agy` (1.7.2): an interactive wall on the agent home.
pub fn login(home: &Home, rest: &[String]) -> i32 {
    let argv: Vec<String> = match rest.first().map(String::as_str) {
        Some("claude") => vec!["claude".into()],
        Some("codex") => vec!["codex".into(), "login".into()],
        // It prints a link and takes the pasted code: no callback port, no session bus (fac-657).
        Some("agy") => vec!["agy".into(), "acp".into(), "--login".into()],
        _ => return fail("factory login claude|codex|agy"),
    };
    let ctx = match context(home) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    for d in [home.agent_home(), home.run_dir(), home.builds()] {
        let _ = std::fs::create_dir_all(d);
    }
    let owner = factory_types::AgentKey::new("login");
    let id = factory_types::JobId::new("login");
    let ah = home.agent_home();
    let places = match ctx.places(&ah, Vec::new(), Vec::new(), None, &[]) {
        Ok(places) => places,
        Err(e) => return fail(&e),
    };
    let mut env = ctx.env(&id, &owner, None, &[], &home.job(id.as_str()).join("io"));
    env.insert("TERM".into(), std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into()));
    // Its binary is read-only in the wall, as at a turn.
    if argv[0] == "agy" {
        env.insert("AGY_CLI_DISABLE_AUTO_UPDATE".into(), "true".into());
    }
    let spec = ctx.spec(
        &id,
        factory_types::job::JobKind::Turn,
        &owner,
        argv,
        &ah.display().to_string(),
        &places,
        env,
        Some(ctx.config.factory.ports.clone()),
        clock::now_ms(),
    );
    launch::interactive_wall(&spec, home.root()).unwrap_or_else(|e| fail(&e.to_string()))
}

/// `factory install [--path]`: the desktop entry; with `--path`, `~/.local/bin/factory` linked to
/// this binary, Andrei's switch from v1 (1.7.3, 6.5.3; M5.13).
pub fn install(rest: &[String]) -> i32 {
    let Some(home) = std::env::var_os("HOME") else { return fail("no HOME") };
    if rest.iter().any(|a| a == "--path") {
        return match builds::link_path(std::path::Path::new(&home), &bin()) {
            Ok(link) => {
                println!("{} → {}", link.display(), bin().display());
                0
            }
            Err(why) => fail(&why),
        };
    }
    let dir = std::path::Path::new(&home).join(".local/share/applications");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Factory\nExec={}\nTerminal=false\nCategories=Development;\n",
        bin().display()
    );
    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join("factory.desktop"), entry)) {
        Ok(()) => 0,
        Err(e) => fail(&e.to_string()),
    }
}

/// `factory web` (fac-671): prints the local web bridge's keyed link; refused inside walls.
pub fn web(home: &Home) -> i32 {
    let walled =
        ["FACTORY_JOB", "FACTORY_AGENT_SOCK"].iter().any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()));
    if walled {
        return fail("factory web is refused inside a wall");
    }
    let path = match config::path() {
        Some(p) => p,
        None => return fail("factory web: no config path: set FACTORY_CONFIG or HOME"),
    };
    let cfg = match config::read(&path) {
        Ok(c) => c,
        Err(e) => return fail(&format!("factory web: {e}")),
    };
    let port = match cfg.factory.web_port {
        None => return fail("factory web: [factory] web_port is not set in factory.toml"),
        Some(0) => return fail("factory web: web_port = 0 uses an ephemeral port; set a fixed port"),
        Some(p) => p,
    };
    let key_path = home.web_key();
    let key = match factory_engine::web::key::read(&key_path) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return fail("factory web: web.key does not exist yet (the bridge has not started)");
        }
        Err(e) => return fail(&format!("factory web: {}: {e}", key_path.display())),
    };
    println!("{}", factory_engine::web::key::link(port, &key));
    0
}
