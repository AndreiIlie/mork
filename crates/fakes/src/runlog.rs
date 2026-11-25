//! The run log (1.10.3): every fake appends one line per run.

use std::io::Write;

use sha2::{Digest, Sha256};

/// Appends this run's line to `$FAKE_LOG`: name, argv, folder, `FACTORY_JOB`,
/// `FACTORY_AGENT`, a hash of stdin, the time.
pub fn append(name: &str, argv: &[String], stdin: &[u8]) {
    let Some(path) = std::env::var_os("FAKE_LOG") else { return };
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let hash = format!("{:x}", Sha256::digest(stdin));
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let overlap = hold_run_lock(&path);
    let line = serde_json::json!({
        "name": name,
        "overlap": overlap,
        "argv": argv,
        "cwd": cwd,
        "job": std::env::var("FACTORY_JOB").unwrap_or_default(),
        "agent": std::env::var("FACTORY_AGENT").unwrap_or_default(),
        "stdin_sha256": hash,
        "time": u64::try_from(time).unwrap_or(0),
    });
    // One write of the whole line: runs at once append to one log, and a line written in
    // pieces could interleave with another's and be lost.
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

// Held for the rest of the run; `true` when another run held it (two runs overlapped). A file
// lock, so it works across walls.
fn hold_run_lock(log: &std::ffi::OsStr) -> bool {
    let name = std::env::args().next().unwrap_or_default();
    let base = std::path::Path::new(&name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let path = format!("{}.{base}.lock", std::path::Path::new(log).display());
    let Ok(f) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path) else { return false };
    let overlap = f.try_lock().is_err();
    std::mem::forget(f);
    overlap
}

pub fn read(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}
