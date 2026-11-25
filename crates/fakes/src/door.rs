//! Door fakes (fac-86); each run's argv and stdin go to `$FAKE_LOG.door.jsonl`. Scenario `$FAKE_DOOR`:
//! {"ssh": {"<alias>": {"after": disconnect|hang|tunnel-drop}}, "mysql": [<reply>…], "mongoexport": [<reply>…]}

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

pub(crate) fn scenario() -> Value {
    std::env::var_os("FAKE_DOOR")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// The door's own run log: one line per run, with its literal argv and stdin.
pub fn log_path() -> Option<std::path::PathBuf> {
    std::env::var_os("FAKE_LOG").map(|p| std::path::PathBuf::from(format!("{}.door.jsonl", p.to_string_lossy())))
}

pub(crate) fn log(name: &str, argv: &[String], stdin: &[u8]) {
    crate::runlog::append(name, argv, stdin);
    let Some(path) = log_path() else { return };
    let line = json!({
        "name": name,
        "argv": argv,
        "stdin": String::from_utf8_lossy(stdin),
        "pid": std::process::id(),
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

/// The door's run log's lines, of one fake.
pub fn runs(log: &std::path::Path, name: &str) -> Vec<Value> {
    let path = std::path::PathBuf::from(format!("{}.door.jsonl", log.display()));
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["name"] == name)
        .collect()
}

pub(crate) fn stdin_all() -> Vec<u8> {
    let mut b = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut b);
    b
}

pub(crate) fn after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1)).map(String::as_str)
}

pub(crate) fn option<'a>(argv: &'a [String], prefix: &str) -> Option<&'a str> {
    argv.iter().find_map(|a| a.strip_prefix(prefix))
}

pub(crate) fn fail(code: i32, words: &str) -> i32 {
    eprintln!("{words}");
    code
}

/// `ssh`: Andrei's own alias, with no key, login or configuration of the Factory's; a command is the one remote
/// string; a forward binds a socket here and holds the connection open until the scenario's `after` ends it.
pub fn ssh_main() -> i32 {
    let argv: Vec<String> = std::env::args().collect();
    let stdin = stdin_all();
    log("ssh", &argv, &stdin);
    // The alias carries its own host, login and keys: a user, key or configuration file named here
    // would be the Factory reaching past it, which it never does.
    for bad in ["-i", "-l", "-F"] {
        if argv.iter().any(|a| a == bad) {
            return fail(255, "fake ssh: a key, login or config file was named; the alias carries its own");
        }
    }
    if argv.windows(2).any(|w| w[0] == "-o" && (w[1].starts_with("User=") || w[1].starts_with("IdentityFile="))) {
        return fail(255, "fake ssh: a login or key was set; the alias carries its own");
    }
    let dd = argv.iter().position(|a| a == "--");
    let alias = dd.and_then(|i| argv.get(i + 1)).cloned().unwrap_or_default();
    let entry = scenario()["ssh"][&alias].clone();
    if entry.is_null() {
        return fail(255, &format!("ssh: Could not resolve hostname {alias}: no such alias"));
    }
    let next = entry["after"].as_str();
    if let Some(socket) = after(&argv, "-S") {
        return web_tunnel(socket, &entry);
    }
    if argv.iter().any(|a| a == "-N") {
        let Some(forward) = after(&argv, "-L") else { return fail(255, "fake ssh: -N with no -L") };
        return tunnel(forward, next);
    }
    let remote = dd.and_then(|i| argv.get(i + 2)).cloned().unwrap_or_default();
    command(&remote, next, &stdin)
}

fn web_tunnel(socket: &str, entry: &Value) -> i32 {
    if entry["after"] == "fail" {
        return fail(255, "bind: Address already in use");
    }
    let ready = entry["ready"].as_str().map(std::path::Path::new);
    while ready.is_some_and(|p| !p.exists()) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _listener = match UnixListener::bind(socket) {
        Ok(l) => l,
        Err(e) => return fail(255, &e.to_string()),
    };
    let drop = entry["drop"].as_str().map(std::path::Path::new);
    while !drop.is_some_and(std::path::Path::exists) {
        std::thread::sleep(Duration::from_millis(10));
    }
    fail(255, "client_loop: send disconnect: Broken pipe")
}

// A forward: this side's socket, held open; each client that reaches it gets one byte and is kept.
// `tunnel-drop` exits once one connects, so the client sees the line vanish as a dropped forward's would.
fn tunnel(forward: &str, after: Option<&str>) -> i32 {
    // `<socket>:<host>:<port>`; the socket has no `:` (the door keeps it so), so the first one ends it.
    let Some((socket, _)) = forward.split_once(':') else {
        return fail(255, "fake ssh: a forward is <socket>:<host>:<port>");
    };
    let _ = std::fs::remove_file(socket);
    let listener = match UnixListener::bind(socket) {
        Ok(l) => l,
        Err(e) => return fail(255, &format!("fake ssh: can't bind the forward: {e}")),
    };
    let mut held = Vec::new();
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        if after == Some("tunnel-drop") {
            // Exit with the stream still open: the client can only see its EOF once this process is
            // gone, so a client that failed can tell its forward was lost.
            std::process::exit(255);
        }
        let _ = s.write_all(b"\x01");
        held.push(s);
    }
    0
}

// A command: the one remote string, run as the login shell there would, with the box's own tools.
// `disconnect` runs it, then ends the connection with its answer never sent; `hang` stalls.
fn command(remote: &str, after: Option<&str>, stdin: &[u8]) -> i32 {
    let child = Command::new("sh")
        .arg("-c")
        .arg(remote)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else { return fail(255, "fake ssh: the remote shell didn't start") };
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(stdin);
    }
    let Ok(out) = child.wait_with_output() else { return fail(255, "fake ssh: the remote shell was lost") };
    match after {
        // It ran there; its answer never came back.
        Some("disconnect") => fail(255, "client_loop: send disconnect: Broken pipe"),
        Some("hang") => loop {
            std::thread::sleep(Duration::from_secs(3600));
        },
        _ => {
            let _ = std::io::stdout().write_all(&out.stdout);
            let _ = std::io::stderr().write_all(&out.stderr);
            out.status.code().unwrap_or(255)
        }
    }
}
