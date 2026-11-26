//! What every fake CLI does: log its run, then play its scenario's steps.

use std::io::{Read, Write};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use crate::scenario::{self, Scenario, Step, Who};

/// What a run knows of itself.
#[derive(Debug, Clone, Default)]
pub struct RunCtx {
    pub fake: String,
    pub argv: Vec<String>,
    pub prompt: String,
    /// Claude's session id, Codex's thread or agy's conversation.
    pub session: String,
    /// Codex's `-o` file.
    pub last_message: Option<String>,
    /// A walled Codex turn's app-server (fac-389): its stream is the app-server's.
    pub app_server: bool,
}

fn arg_after(argv: &[String], flag: &str) -> Option<String> {
    argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1)).cloned()
}

impl RunCtx {
    fn new(fake: &str, argv: Vec<String>, prompt: String) -> Self {
        let session = match fake {
            "codex" => arg_after(&argv, "resume").unwrap_or_else(|| format!("thread-{}", std::process::id())),
            "agy" => arg_after(&argv, "--conversation").unwrap_or_else(|| format!("conv-{}", std::process::id())),
            _ => arg_after(&argv, "--session-id").or_else(|| arg_after(&argv, "--resume")).unwrap_or_default(),
        };
        let last_message = arg_after(&argv, "-o");
        Self { fake: fake.to_owned(), argv, prompt, session, last_message, app_server: false }
    }
}

/// Runs a fake named `name`: reads stdin, appends the run log, plays the steps.
pub fn main(name: &str) -> i32 {
    let argv: Vec<String> = std::env::args().collect();
    let app_server = name == "codex" && argv.iter().any(|a| a == "app-server");
    // Codex's limits reader (`codex app-server`, given no settings) is logged and matched as a fake of its own; a
    // walled turn's app-server is `codex`, its prompt in its turn's opening (fac-389).
    let turn_server = app_server && argv.iter().any(|a| a == "-c");
    let name = if app_server && !turn_server { "codex-app-server" } else { name };
    let (stdin, thread) = read_prompt(name, &argv, turn_server);
    let run = std::env::var_os("FAKE_LOG")
        .map(|p| crate::runlog::read(std::path::Path::new(&p)).iter().filter(|l| l["name"] == name).count())
        .unwrap_or(0) as u64
        + 1;
    crate::runlog::append(name, &argv, &stdin);
    report_paths();
    let prompt = String::from_utf8_lossy(&stdin).into_owned();
    let agent = std::env::var("MORK_AGENT").unwrap_or_default();
    let who = Who { fake: name, agent: &agent, run, prompt: &prompt };
    let steps = Scenario::load().steps_for(&who).unwrap_or_else(|| scenario::default_steps(name));
    let mut ctx = RunCtx::new(name, argv, prompt);
    if let Some(t) = thread {
        ctx.session = t;
        ctx.app_server = true;
    }
    play(&ctx, &steps)
}

// A prompt comes on stdin only to `claude -p`, `codex exec` and agy, or in a live turn's opening (fac-389); a login's
// terminal never ends, so it isn't read then. An app-server turn's thread comes with it.
fn read_prompt(name: &str, argv: &[String], turn_server: bool) -> (Vec<u8>, Option<String>) {
    if turn_server {
        let (thread, prompt) = crate::live::codex_open();
        return (prompt.into_bytes(), Some(thread));
    }
    if ["claude", "agy"].contains(&name) && argv.windows(2).any(|w| w[0] == "--input-format" && w[1] == "stream-json") {
        return (crate::live::stream_prompt().into_bytes(), None);
    }
    let mut stdin = Vec::new();
    let prompted =
        (name == "claude" && argv.iter().any(|a| a == "-p")) || (name == "codex" && argv.iter().any(|a| a == "exec"));
    if prompted {
        let _ = std::io::stdin().read_to_end(&mut stdin);
    }
    (stdin, None)
}

fn report_paths() {
    let Ok(list) = std::env::var("FAKE_CHECK_PATHS") else { return };
    let seen: serde_json::Map<String, Value> = list
        .split(':')
        .filter(|p| !p.is_empty())
        .map(|p| (p.to_owned(), std::path::Path::new(p).exists().into()))
        .collect();
    println!("{}", json!({"type": "fake-paths", "exists": seen}));
}

fn line(v: &Value) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn result(ctx: &RunCtx, output: Value) -> Value {
    if ctx.fake == "agy" {
        let usage = json!({"input_tokens": 1, "output_tokens": 1, "cache_read_tokens": 0, "total_tokens": 2});
        let r = json!({"conversation_id": ctx.session, "status": "SUCCESS", "response": "", "usage": usage,
            "structured_output": output});
        return json!({"event": "result", "result": r});
    }
    json!({
        "type": "result", "subtype": "success", "is_error": false, "result": "",
        "session_id": ctx.session, "total_cost_usd": 0.01, "structured_output": output,
    })
}

/// Plays the steps; returns the exit code.
pub fn play(ctx: &RunCtx, steps: &[Step]) -> i32 {
    for step in steps {
        if let Some(code) = play_one(ctx, step) {
            return code;
        }
    }
    0
}

fn codex_completed(ctx: &RunCtx, text: &str) {
    if ctx.app_server {
        let item = json!({"type": "agentMessage", "id": "msg-1", "text": text});
        crate::live::notify(
            "item/completed",
            &json!({"threadId": ctx.session, "turnId": crate::live::TURN, "item": item}),
        );
        let turn = json!({"id": crate::live::TURN, "status": "completed", "items": []});
        crate::live::notify("turn/completed", &json!({"threadId": ctx.session, "turn": turn}));
        return;
    }
    if let Some(p) = &ctx.last_message {
        let _ = std::fs::write(p, text);
    }
    line(&json!({"type": "turn.completed", "usage": {"input_tokens": 1, "output_tokens": 1}}));
}

fn play_one(ctx: &RunCtx, step: &Step) -> Option<i32> {
    match step {
        Step::Init if ctx.fake == "agy" => line(&json!({"event": "init", "conversation_id": ctx.session})),
        Step::Init => line(&json!({"type": "system", "subtype": "init", "session_id": ctx.session})),
        Step::Reply { value, cost } => {
            let value = serde_json::from_str(&with_io(&value.to_string())).unwrap_or_else(|_| value.clone());
            let mut r = result(ctx, value);
            r["total_cost_usd"] = json!(cost.unwrap_or(0.01));
            line(&r);
        }
        Step::Echo { text } => line(&result(ctx, json!({"message": text}))),
        Step::ThreadStarted { thread } if ctx.app_server => {
            let thread = thread.clone().unwrap_or(ctx.session.clone());
            crate::live::notify("thread/started", &json!({"thread": {"id": thread}}));
            crate::live::notify("turn/started", &json!({"threadId": thread, "turn": {"id": crate::live::TURN}}));
        }
        Step::ThreadStarted { thread } => {
            line(&json!({"type": "thread.started", "thread_id": thread.clone().unwrap_or(ctx.session.clone())}));
        }
        Step::CodexReply { value } => codex_completed(ctx, &with_io(&value.to_string())),
        Step::Sleep { ms } => std::thread::sleep(Duration::from_millis(*ms)),
        Step::Hang => loop {
            std::thread::sleep(Duration::from_secs(3600));
        },
        Step::Exit { code } => return Some(*code),
        Step::WriteFile { path, text } => {
            let _ = std::fs::write(with_io(path), text);
        }
        Step::Sh { script } => sh(&with_io(script)),
        Step::Symlink { link, target } => {
            let _ = std::os::unix::fs::symlink(target, with_io(link));
        }
        Step::Probe { spec } => probe(spec),
        Step::Ctl { args } => ctl(&args.iter().map(|a| with_io(a)).collect::<Vec<_>>()),
        Step::ShowEnv => show_env(),
        Step::CargoBuild { commit, link } => return Some(cargo_build(&ctx.argv, commit.as_deref(), link.as_deref())),
        Step::Checks { exit, report } => return Some(checks(&ctx.argv, *exit, report.as_ref())),
    }
    let _ = std::io::stdout().flush();
    None
}

fn sibling(name: &str) -> std::path::PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name))).unwrap_or_else(|| name.into())
}

fn probe(spec: &Value) {
    let out = Command::new(sibling("probe")).arg(spec.to_string()).output();
    match out {
        Ok(o) => print!("{}", String::from_utf8_lossy(&o.stdout)),
        Err(e) => line(&json!({"type": "probe", "error": e.to_string()})),
    }
}

fn sh(script: &str) {
    let _ = Command::new("sh").args(["-c", script]).output();
}

// `{io}` is the turn's `MORK_IO`, its seat's folder (fac-654); `{job}` its job's folder, `{jobs}` all jobs',
// `{seats}` all seats' folders.
fn with_io(s: &str) -> String {
    let socket = std::env::var_os("MORK_AGENT_SOCK").map(std::path::PathBuf::from).unwrap_or_default();
    let id = std::env::var("MORK_JOB").unwrap_or_default();
    let root = socket.parent().and_then(std::path::Path::parent).unwrap_or(std::path::Path::new(""));
    let jobs = root.join("jobs");
    let job = jobs.join(id);
    let io = std::env::var("MORK_IO")
        .ok()
        .filter(|io| !io.is_empty())
        .unwrap_or_else(|| job.join("io").display().to_string());
    let s = s.replace("{io}", &io).replace("{jobs}", &jobs.display().to_string());
    let s = s.replace("{seats}", &root.join("io").display().to_string());
    s.replace("{job}", &job.display().to_string())
}

fn ctl(args: &[String]) {
    mork_cmd("fake-ctl", "ctl", args, None);
}

fn mork_cmd(kind: &str, sub: &str, args: &[String], dir: Option<&str>) {
    let mut cmd = Command::new("mork");
    if let Some(d) = dir {
        let _ = std::fs::create_dir_all(d);
        cmd.current_dir(d);
    }
    let out = cmd.arg(sub).args(args).output();
    let v = match out {
        Ok(o) => json!({"type": kind, "code": o.status.code(),
            "out": String::from_utf8_lossy(&o.stdout), "err": String::from_utf8_lossy(&o.stderr)}),
        Err(e) => json!({"type": kind, "code": null, "err": e.to_string()}),
    };
    line(&v);
}

fn checks(argv: &[String], exit: i32, report: Option<&Value>) -> i32 {
    if let (Some(path), Some(r)) = (arg_after(argv, "--json"), report) {
        let mut r = r.clone();
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
        if r.get("head").is_none_or(Value::is_null) {
            r["head"] = json!(head);
        }
        if r.get("base").is_none_or(Value::is_null) {
            r["base"] = json!(arg_after(argv, "--base"));
        }
        let _ = std::fs::write(path, r.to_string());
    }
    exit
}

fn show_env() {
    let env: serde_json::Map<String, Value> = std::env::vars().map(|(k, v)| (k, Value::String(v))).collect();
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let stdin = std::fs::read_link("/proc/self/fd/0").map(|p| p.display().to_string()).unwrap_or_default();
    line(&json!({"type": "fake-env", "env": env, "cwd": cwd, "stdin": stdin}));
}

fn cargo_build(argv: &[String], as_commit: Option<&str>, link: Option<&str>) -> i32 {
    let commit = as_commit.map_or_else(|| std::env::var("MORK_BUILD_COMMIT").unwrap_or_default(), str::to_owned);
    let target = arg_after(argv, "--target-dir")
        .or_else(|| std::env::var("CARGO_TARGET_DIR").ok())
        .unwrap_or_else(|| "target".into());
    let release = std::path::Path::new(&target).join("release");
    if std::fs::create_dir_all(&release).is_err() {
        return 101;
    }
    let real = sibling("mork");
    let script = format!("#!/bin/sh\nMORK_TEST_COMMIT={commit} exec {} \"$@\"\n", real.display());
    let bin = release.join("mork");
    if let Some(to) = link {
        let _ = std::fs::remove_file(&bin);
        return if std::os::unix::fs::symlink(to, &bin).is_ok() { 0 } else { 101 };
    }
    if std::fs::write(&bin, script).is_err() {
        return 101;
    }
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
    0
}
