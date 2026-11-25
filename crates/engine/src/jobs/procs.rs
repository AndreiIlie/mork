//! A job's processes, seen from outside through `/proc` (1.4.4).

use std::fs;
use std::io;
use std::time::Duration;

use crate::clock;

/// A process's start time, in clock ticks since boot (`/proc/<pid>/stat`, field 22).
pub fn start_time(pid: i32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// A process's pid namespace (`pid:[…]`).
pub fn pidns(pid: i32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/ns/pid")).ok().map(|p| p.to_string_lossy().into_owned())
}

pub fn boot_id() -> String {
    fs::read_to_string("/proc/sys/kernel/random/boot_id").map(|s| s.trim().to_owned()).unwrap_or_default()
}

pub fn all_pids() -> Vec<i32> {
    let Ok(rd) = fs::read_dir("/proc") else { return Vec::new() };
    rd.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect()
}

/// The processes in a pid namespace; none for an empty one.
pub fn in_namespace(ns: &str) -> Vec<i32> {
    if ns.is_empty() {
        return Vec::new();
    }
    all_pids().into_iter().filter(|&p| pidns(p).as_deref() == Some(ns)).collect()
}

pub fn environ_var(pid: i32, key: &str) -> Option<String> {
    let raw = fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{key}=");
    raw.split(|&b| b == 0)
        .filter_map(|kv| std::str::from_utf8(kv).ok())
        .find_map(|kv| kv.strip_prefix(&prefix).map(str::to_owned))
}

fn ppid(pid: i32) -> Option<i32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// Whether `pid` is `ancestor` or one of its descendants, by its parents' chain.
pub fn descends_from(pid: i32, ancestor: i32) -> bool {
    let mut p = pid;
    // A chain this long is no job's: give up rather than walk a cycle.
    for _ in 0..256 {
        if p == ancestor {
            return true;
        }
        match ppid(p) {
            Some(parent) if parent > 0 && parent != p => p = parent,
            _ => return false,
        }
    }
    false
}

pub fn pgid(pid: i32) -> Option<i32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(2)?.parse().ok()
}

/// Whether a process is a zombie (exited, not yet reaped).
pub fn is_zombie(pid: i32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s.rsplit_once(')').map(|(_, r)| r.trim_start().starts_with('Z')))
        .unwrap_or(false)
}

/// Unwalled, a job's processes: its process group, every process whose environment carries its
/// `FACTORY_JOB` (1.4.4.1), and, asked by its supervisor (`sup`), every descendant it adopted.
pub fn unwalled(pgid_of_job: Option<i32>, job: &str, sup: Option<i32>) -> Vec<i32> {
    let me = std::process::id().cast_signed();
    all_pids()
        .into_iter()
        .filter(|&p| p != me && !is_zombie(p))
        .filter(|&p| {
            (pgid_of_job.is_some() && pgid(p) == pgid_of_job)
                || environ_var(p, "FACTORY_JOB").as_deref() == Some(job)
                || sup.is_some_and(|s| descends_from(p, s))
        })
        .collect()
}

/// Sends a signal to each pid, ignoring the ones already gone.
pub fn signal_all(pids: &[i32], sig: i32) {
    for &p in pids {
        let _ = factory_sys::kill(p, sig);
    }
}

/// Waits, in real time, until `alive()` is empty or `limit` passes; returns what is left.
pub fn wait_until_gone(limit: Duration, alive: impl Fn() -> Vec<i32>) -> Vec<i32> {
    let start = clock::instant();
    loop {
        let left = alive();
        if left.is_empty() || clock::elapsed(start) >= limit {
            return left;
        }
        clock::sleep(Duration::from_millis(50));
    }
}

/// SIGTERM to all of a wall's processes but its init, waiting for none.
pub fn term_walled(ns: &str, init: i32) {
    if ns.is_empty() || pidns(std::process::id().cast_signed()).as_deref() == Some(ns) {
        return;
    }
    let others: Vec<i32> = in_namespace(ns).into_iter().filter(|&p| p != init && !is_zombie(p)).collect();
    signal_all(&others, libc_sig::TERM);
}

/// Ends a walled job (1.4.4.2): SIGTERM to all but the init; after 5 s, SIGKILL to the init;
/// then up to 10 s for none to remain. Returns those that wouldn't go.
pub fn end_walled(ns: &str, init: i32) -> Vec<i32> {
    // Never the empty namespace, nor the caller's own: that is everyone's, not a wall's.
    if ns.is_empty() || pidns(std::process::id().cast_signed()).as_deref() == Some(ns) {
        return Vec::new();
    }
    let others = || -> Vec<i32> { in_namespace(ns).into_iter().filter(|&p| p != init && !is_zombie(p)).collect() };
    signal_all(&others(), libc_sig::TERM);
    let _ = wait_until_gone(Duration::from_secs(5), others);
    let _ = factory_sys::kill(init, libc_sig::KILL);
    wait_until_gone(Duration::from_secs(10), || in_namespace(ns).into_iter().filter(|&p| !is_zombie(p)).collect())
}

/// Ends an unwalled job: SIGTERM to its processes, after 5 s SIGKILL to what is left.
pub fn end_unwalled(pgid_of_job: Option<i32>, job: &str, sup: Option<i32>) -> Vec<i32> {
    signal_all(&unwalled(pgid_of_job, job, sup), libc_sig::TERM);
    let left = wait_until_gone(Duration::from_secs(5), || unwalled(pgid_of_job, job, sup));
    signal_all(&left, libc_sig::KILL);
    wait_until_gone(Duration::from_secs(10), || unwalled(pgid_of_job, job, sup))
}

/// Signal numbers, without a libc dependency in this crate.
pub mod libc_sig {
    pub const TERM: i32 = 15;
    pub const KILL: i32 = 9;
    pub const HUP: i32 = 1;
}

pub fn ctx(e: io::Error, what: &str) -> io::Error {
    io::Error::new(e.kind(), format!("{what}: {e}"))
}
