//! The door's transport (fac-1, fac-86): the dispatcher is the engine's own child, whatever the target.
//! The request is one JSON line on stdin; the argv names no request value and no credential.

use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::bounded::{self, Bounds, Captured};

/// The dispatcher: this build's `door-dispatch`, allowed the route's kinds, keeping its receipts
/// and its calls' own folders under the home's `door/`.
pub fn local_argv(bin: &Path, allow: &[String], receipts: &Path, tunnels: &Path) -> Vec<String> {
    vec![
        bin.display().to_string(),
        "door-dispatch".into(),
        "--allow".into(),
        allow.join(","),
        "--receipts".into(),
        receipts.display().to_string(),
        "--tunnels".into(),
        tunnels.display().to_string(),
    ]
}

/// Sends one request line through `argv` and reads the answer, bounded: `max_bytes` of output (room for the
/// JSON's escapes), a deadline a little past the call's own; `stop`, once set, cuts it.
pub fn send(
    home_root: &Path,
    argv: &[String],
    line: &str,
    (max_bytes, timeout_ms): (u64, u64),
    stop: Option<&AtomicBool>,
) -> io::Result<Captured> {
    // The ssh agent's socket too, for a key of the operator's with a passphrase; never forwarded.
    let env = super::door_ssh::env(bounded::door_env());
    let stdin = format!("{line}\n");
    let max = usize::try_from(max_bytes).unwrap_or(usize::MAX).saturating_mul(8).saturating_add(65_536);
    let bounds =
        Bounds { max_out: max, max_err: 16 * 1024, timeout: Duration::from_millis(timeout_ms.saturating_add(15_000)) };
    let run = bounded::Run { argv, env: Some(&env), cwd: None, stdin: stdin.as_bytes() };
    bounded::run_watched(Some(home_root), "door", &run, bounds, &bounded::Watch { stop, ..bounded::Watch::default() })
}
