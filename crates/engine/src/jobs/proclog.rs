//! `logs/processes.log` (1.2.5): every process the engine starts, one JSON line each.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::clock;

pub fn record(home_root: &Path, kind: &str, argv: &[String], cwd: &str, walled: bool) {
    let line = serde_json::json!({
        "time": clock::now_ms(),
        "kind": kind,
        "argv": without_env_values(argv),
        "cwd": cwd,
        "walled": walled,
    });
    append(home_root, "processes.log", &line);
}

/// Appends one JSON line to a file of `logs/`, rotated at 10 MB with five kept, like `app.log` (1.3).
pub fn append(home_root: &Path, name: &str, line: &serde_json::Value) {
    let dir = home_root.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(name);
    crate::files::rotate(&path, 10 * 1024 * 1024, 5);
    // One write per whole line: the engine's threads start processes at once, and a line
    // written in pieces interleaves with another's.
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

/// The argv with each `--setenv <key> <value>`'s value dropped: a wall's environment can hold
/// secrets (the telemetry header, a seat's `env`), and the log is no place for them.
pub fn without_env_values(argv: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(argv.len());
    let mut i = 0;
    while let Some(a) = argv.get(i) {
        out.push(a.clone());
        if a == "--setenv" {
            if let Some(k) = argv.get(i + 1) {
                out.push(k.clone());
                out.push("‹value›".into());
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}
