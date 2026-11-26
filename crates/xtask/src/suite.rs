//! fac-69: the suite's test binaries, run side by side rather than one after another as `cargo
//! test` runs them, each in its package's folder with the environment `cargo test` gives it.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

// How often a running binary's output is read for the results it printed (fac-697).
const POLL: Duration = Duration::from_millis(100);

/// A test binary to run: its program, arguments, folder and environment settings.
#[derive(Debug)]
pub struct Binary {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub dir: PathBuf,
    pub env: Vec<(String, String)>,
}

/// How many binaries run at once: a quarter of the cores, as each binary already runs its own
/// tests on all of them, and at least 2.
pub fn at_most() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get).div_ceil(4).max(2)
}

/// The whole workspace's tests, for `build`'s `select`.
pub const WORKSPACE: [&str; 1] = ["--workspace"];

/// The tests named `production`, all in these two targets of the app, for `build`'s `select`.
pub const PRODUCTION: [&str; 6] = ["-p", "mork-app", "--test", "m0_walled_turn", "--test", "t1_walls"];

/// Builds the tests `select` names with `cargo test` and `extra`, without running them,
/// and gives the test binaries it made, each to run with `args`.
pub fn build(root: &Path, select: &[&str], extra: &[&str], args: &[&str]) -> Result<Vec<Binary>, String> {
    let meta = crate::graph::metadata(root)?;
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["test", "--no-fail-fast", "--no-run", "--message-format=json-render-diagnostics"])
        .args(select)
        .args(extra)
        .current_dir(root)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo test --no-run: {e}"))?;
    if !out.status.success() {
        return Err(format!("the build failed: {}", out.status));
    }
    binaries(&String::from_utf8_lossy(&out.stdout), &meta, args)
}

/// The test binaries cargo's JSON `messages` report, each to run with `args`, the largest first:
/// the binaries that link the app and the window hold the slow tests, so none of them starts last.
pub fn binaries(messages: &str, meta: &Value, args: &[&str]) -> Result<Vec<Binary>, String> {
    let messages: Vec<Value> = messages.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let mut out = Vec::new();
    for m in messages.iter().filter(|m| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true) {
        let Some(program) = m["executable"].as_str() else { continue };
        let id = m["package_id"].as_str().unwrap_or_default();
        let package = meta["packages"].as_array().into_iter().flatten().find(|p| p["id"] == id);
        let package = package.ok_or_else(|| format!("{program}: no package {id} in cargo metadata"))?;
        let manifest = PathBuf::from(m["manifest_path"].as_str().unwrap_or_default());
        let dir = manifest.parent().map(Path::to_path_buf).ok_or_else(|| format!("{program}: no manifest path"))?;
        let src = PathBuf::from(m["target"]["src_path"].as_str().unwrap_or_default());
        let src = src.strip_prefix(&dir).unwrap_or(&src).display().to_string();
        let name = format!("{} {src}", package["name"].as_str().unwrap_or(id));
        let args = args.iter().map(|a| (*a).to_owned()).collect();
        out.push(Binary { name, program: program.into(), args, dir, env: Vec::new() });
    }
    if out.is_empty() {
        return Err("the build reported no test binaries".into());
    }
    out.sort_by_key(|b| std::cmp::Reverse(std::fs::metadata(&b.program).map_or(0, |m| m.len())));
    Ok(out)
}

/// Runs every binary, `at_most` at a time, never stopping at a failure. Each one's output is
/// printed whole when it ends; the failures come back, one line each.
pub fn run_all(binaries: &[Binary], at_most: usize) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..at_most.clamp(1, binaries.len().max(1)) {
            let (tx, next) = (tx.clone(), &next);
            scope.spawn(move || {
                while let Some(b) = binaries.get(next.fetch_add(1, Ordering::Relaxed)) {
                    eprintln!("xtask: started {}", b.name);
                    let start = Instant::now();
                    if tx.send((b, run_one(b), start.elapsed())).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx);
        rx.iter().filter_map(|(b, ran, took)| judge(b, ran, took)).collect()
    })
}

fn judge(b: &Binary, ran: std::io::Result<(ExitStatus, Vec<u8>)>, took: Duration) -> Option<String> {
    let (verdict, output) = match ran {
        Ok((status, output)) if status.success() => (None, output),
        Ok((status, output)) => (Some(status.to_string()), output),
        Err(e) => (Some(format!("couldn't run {}: {e}", b.program.display())), Vec::new()),
    };
    let mut err = std::io::stderr().lock();
    let said = verdict.as_deref().map_or_else(|| "ok".to_owned(), |v| format!("FAILED, {v}"));
    let _ = writeln!(err, "xtask: {}: {said}, in {:.1}s", b.name, took.as_secs_f64());
    let _ = err.write_all(&output);
    verdict.map(|v| format!("{}: {v}", b.name))
}

// The output goes to an unlinked file rather than a pipe: a process the binary leaves behind
// can't hold its run open, and the hygiene step still finds it.
fn run_one(b: &Binary) -> std::io::Result<(ExitStatus, Vec<u8>)> {
    let mut log = scratch()?;
    let mut cmd = Command::new(&b.program);
    cmd.args(&b.args).current_dir(&b.dir).env_remove(crate::walls::RECORD);
    let mut child = cmd
        .envs(b.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .spawn()?;
    let mut told = 0;
    let status = loop {
        let ended = child.try_wait()?;
        told = tell(b, &log, told)?;
        if let Some(status) = ended {
            break status;
        }
        std::thread::sleep(POLL);
    };
    let mut output = Vec::new();
    log.seek(SeekFrom::Start(0))?;
    log.read_to_end(&mut output)?;
    Ok((status, output))
}

// Each test result the binary printed after `from` is told at once, for the suite's live counts
// (fac-697); its whole output is still printed when it ends. Gives where the complete lines end.
fn tell(b: &Binary, log: &File, from: u64) -> std::io::Result<u64> {
    let len = log.metadata()?.len();
    let mut buf = vec![0; usize::try_from(len.saturating_sub(from)).unwrap_or(0)];
    log.read_exact_at(&mut buf, from)?;
    let complete = buf.iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
    let text = String::from_utf8_lossy(&buf[..complete]);
    for line in text.lines().filter(|l| is_result(l)) {
        eprintln!("xtask: {}: {line}", b.name);
    }
    Ok(from + complete as u64)
}

// libtest's line as a test ends: `test <name> ... ok`, `FAILED`, or `ignored[, <reason>]`.
fn is_result(line: &str) -> bool {
    let Some((_, result)) = line.strip_prefix("test ").and_then(|t| t.rsplit_once(" ... ")) else { return false };
    result == "ok" || result == "FAILED" || result == "ignored" || result.starts_with("ignored, ")
}

/// The tests `b` runs with its arguments, as its harness lists them: `<name>: test`, by the names
/// `--exact` matches.
pub fn listed(b: &Binary) -> Result<BTreeSet<String>, String> {
    let mut cmd = Command::new(&b.program);
    cmd.args(&b.args).arg("--list").current_dir(&b.dir);
    let out = cmd
        .envs(b.env.iter().map(|(k, v)| (k, v)))
        .env_remove(crate::walls::RECORD)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{} --list: {e}", b.name))?;
    if !out.status.success() {
        return Err(format!("{} --list: {}", b.name, out.status));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().filter_map(|l| l.strip_suffix(": test")).map(str::to_owned).collect())
}

/// How many tests the binaries run, none when one can't list them.
pub fn count(binaries: &[Binary]) -> Option<usize> {
    binaries.iter().map(|b| listed(b).ok().map(|l| l.len())).sum()
}

fn scratch() -> std::io::Result<File> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("xtask-output-{}-{n}", std::process::id()));
    let file = OpenOptions::new().read(true).append(true).create_new(true).open(&path)?;
    std::fs::remove_file(&path)?;
    Ok(file)
}
