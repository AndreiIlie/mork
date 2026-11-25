//! Who a peer of `agent.sock` is (1.5.4): the job whose wall its pid lives in, read from the
//! job folders on the connection's own thread, so the engine never waits on it.

use std::path::Path;
use std::time::Duration;

use factory_types::job::WallMode;
use factory_types::{AgentKey, JobId};

use super::folder::JobDir;
use super::procs;
use crate::clock;

/// The owner of the running job whose wall holds `pid`, and that job: by pid namespace, or unwalled by descent
/// from the job's supervisor (fac-25). One still launching (no `run.json`) is waited for, 5 s; `None` if in no job.
pub fn of_peer(jobs_dir: &Path, pid: i32) -> Option<(AgentKey, JobId)> {
    let ns = procs::pidns(pid);
    let start = clock::instant();
    loop {
        let (found, pending) = scan(jobs_dir, pid, ns.as_deref(), true);
        if found.is_some() || !pending || clock::elapsed(start) > Duration::from_secs(5) {
            return found;
        }
        clock::sleep(Duration::from_millis(50));
    }
}

/// The unwalled job a peer of `factory.sock` runs in, if any (fac-25): that agent's, never
/// Andrei's, whichever socket it reaches.
pub fn of_unwalled_peer(jobs_dir: &Path, pid: i32) -> Option<(AgentKey, JobId)> {
    scan(jobs_dir, pid, procs::pidns(pid).as_deref(), false).0
}

fn scan(jobs_dir: &Path, pid: i32, ns: Option<&str>, walled: bool) -> (Option<(AgentKey, JobId)>, bool) {
    let Ok(rd) = std::fs::read_dir(jobs_dir) else { return (None, false) };
    // An unwalled job's processes share the engine's namespace: a walled peer is never one of them,
    // whatever `FACTORY_JOB` it sets.
    let host = ns.is_some() && procs::pidns(std::process::id().cast_signed()).as_deref() == ns;
    let boot = procs::boot_id();
    let mut pending = false;
    for e in rd.flatten() {
        let dir = JobDir::new(e.path());
        if !dir.exists() || dir.exit_path().exists() {
            continue;
        }
        let Ok(Some(spec)) = dir.spec() else { continue };
        if spec.wall == WallMode::None {
            if host && supervised_by(&dir, pid, &boot) {
                return (Some((spec.owner, spec.id)), false);
            }
            continue;
        }
        if !walled {
            continue;
        }
        match dir.run() {
            Ok(Some(run)) if !run.pidns.is_empty() && Some(run.pidns.as_str()) == ns => {
                return (Some((spec.owner, spec.id)), false);
            }
            Ok(Some(_)) => {}
            _ => pending = true,
        }
    }
    (None, pending)
}

// The supervisor claims before it starts the job's process, as its parent: the claim names an unwalled
// job's processes from their first instant. Its pid counts only while its start time and boot match.
fn supervised_by(dir: &JobDir, pid: i32, boot: &str) -> bool {
    let Ok(Some(claim)) = dir.claim() else { return false };
    claim.boot_id == boot
        && procs::start_time(claim.pid) == Some(claim.start_time)
        && procs::descends_from(pid, claim.pid)
}
