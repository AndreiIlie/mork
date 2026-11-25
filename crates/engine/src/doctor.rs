//! `factory doctor` (1.9): reports what is stale or orphaned, and changes nothing. One line
//! per finding, with the command that fixes it where one exists.

use std::collections::BTreeMap;
use std::path::Path;

use factory_core::state::State;

use crate::builds;
use crate::jobs::context::{bwrap_path, wall_mode, wall_unavailable};
use crate::jobs::procs;
use crate::paths::Home;
use crate::store::Store;

/// What doctor knows of the state's jobs: settled or not, and when.
pub type JobsSeen = BTreeMap<String, Option<i64>>;

const DAY: i64 = 86_400_000;

/// The state, read-only; `None` when it can't be read.
pub fn read_state(home: &Home) -> Option<State> {
    let store = Store::open_read_only(&home.state_db()).ok()?;
    store.load().ok().map(|s| with_clones(home, &store, s))
}

// The ended cards a clone is named for, which a start leaves in the store (fac-336): read, so a
// closed review's proofs are kept (fac-276).
fn with_clones(home: &Home, store: &Store, mut state: State) -> State {
    let names = std::fs::read_dir(home.clones()).into_iter().flatten().flatten();
    let ids = names.map(|e| factory_types::CardId::new(e.file_name().to_string_lossy()));
    let ended = ids.filter(|c| state.unloaded.cards.contains(c)).collect();
    for (id, r) in store.cards_records(&ended).unwrap_or_default() {
        state.absorb(&id, r);
    }
    state
}

/// Every job the store holds, ended cards' included (fac-336), and a settled one whose folder was
/// pruned and `status` leaves out, so a process carrying its `FACTORY_JOB` is still reported (M5.30).
pub fn every_job(store: &Store) -> Option<JobsSeen> {
    store.jobs_seen().ok().map(|jobs| jobs.into_iter().collect())
}

/// A state opened read-only (no app running), and its jobs, which an integrity failure leaves
/// unknown and makes a finding.
pub fn from_file(home: &Home, findings: &mut Vec<String>) -> (Option<JobsSeen>, Option<State>) {
    if !home.state_db().exists() {
        return (Some(JobsSeen::new()), None);
    }
    let store = match Store::open_read_only(&home.state_db()) {
        Ok(s) => s,
        Err(e) => {
            let backup = builds::latest_backup(home).unwrap_or_else(|| "none".into());
            findings.push(format!(
                "the state can't be opened: {e}. The latest backup is {backup}; with no factory running, copy it over {}",
                home.state_db().display()
            ));
            return (None, None);
        }
    };
    let state = store.load().ok().map(|s| with_clones(home, &store, s));
    let jobs = every_job(&store);
    if store.pragma("quick_check").ok().as_deref() != Some("ok") {
        let backup = builds::latest_backup(home).unwrap_or_else(|| "none".into());
        findings.push(format!(
            "the state failed SQLite's quick_check. The latest backup is {backup}; with no factory running, copy it over {}",
            home.state_db().display()
        ));
        return (None, state);
    }
    (jobs.filter(|_| state.is_some()), state)
}

/// Every finding, given the jobs the state knows and the state (each `None` when it couldn't be read).
pub fn findings(home: &Home, jobs: Option<&JobsSeen>, state: Option<&State>, now: i64, tmp: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(jobs) = jobs {
        job_folders(home, jobs, now, &mut out);
        job_processes(jobs, &mut out);
    }
    if let Some(p) = builds::pin(home)
        && !builds::binary(home, &p).exists()
    {
        out.push(format!("pin names {p}, which has no build: factory build {p}"));
    }
    if home.pin_next().exists() {
        out.push(format!("a pin.next is left from a reload that never finished: {}", home.pin_next().display()));
    }
    if let Some(why) = wall_unavailable(wall_mode(), &bwrap_path()) {
        out.push(format!("a wall can't start: {why}"));
    }
    test_homes(tmp, "factory-test-", &mut out);
    test_namespaces(&home.agent_home(), &mut out);
    orphan_clones(home, state, &mut out);
    out
}

fn proofs_only(dir: &std::path::Path) -> bool {
    let mut names = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name());
    names.all(|n| n == ".git")
}

fn orphan_clones(home: &Home, state: Option<&State>, out: &mut Vec<String>) {
    for e in std::fs::read_dir(home.clones()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let owned = state.is_some_and(|s| {
            s.cards.get(&factory_types::CardId::new(name.clone())).is_some_and(|c| {
                // A closed review keeps only its `.git/factory/`, for the head that reopens it (fac-276, fac-601).
                c.stage.is_live() || (c.mode == factory_types::card::Mode::Review && proofs_only(&e.path()))
            })
        });
        if !owned {
            out.push(format!("clone {name} belongs to no live card: rm -r {}", e.path().display()));
        }
    }
    let kept = state.map(factory_core::queue::candidates::clones).unwrap_or_default();
    for e in std::fs::read_dir(home.trains()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !kept.contains(&name) {
            out.push(format!("train clone {name} belongs to no candidate: rm -r {}", e.path().display()));
        }
    }
    for c in state.iter().flat_map(|s| s.cards.values()) {
        let made = c.stage.is_live() && c.stage != factory_types::card::Stage::Starting;
        if made && !home.clones().join(c.id.as_str()).is_dir() {
            out.push(format!("card {} is live, and its clone is gone: close it, or start it again", c.id));
        }
    }
}

fn job_folders(home: &Home, jobs: &JobsSeen, now: i64, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(home.jobs()) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        match jobs.get(&name) {
            None => out.push(format!("job folder {name} is unknown to the state: rm -r {}", e.path().display())),
            Some(Some(t)) if now - t > 14 * DAY => {
                out.push(format!("job folder {name} was settled over 14 days ago: rm -r {}", e.path().display()));
            }
            Some(_) => {}
        }
    }
}

fn job_processes(jobs: &JobsSeen, out: &mut Vec<String>) {
    for pid in procs::all_pids() {
        if let Some(job) = procs::environ_var(pid, "FACTORY_JOB")
            && jobs.get(&job).is_some_and(Option::is_some)
        {
            out.push(format!("process {pid} carries the FACTORY_JOB of settled job {job}: kill {pid}"));
        }
    }
}

fn test_namespaces(agent_home: &Path, out: &mut Vec<String>) {
    // fakes/src/home.rs::homes_dir names these folders; earlier workspaces must stay discoverable (fac-471).
    let Ok(entries) = std::fs::read_dir(agent_home) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let namespace = name.len() == 8 && name.starts_with(".t") && name[2..].bytes().all(|b| b.is_ascii_hexdigit());
        if namespace && entry.file_type().is_ok_and(|t| t.is_dir()) {
            test_homes(&entry.path(), "t-", out);
        }
    }
}

fn test_homes(tmp: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(tmp) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let modified = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        let old = modified
            .is_some_and(|m| crate::clock::real_now_ms() - i64::try_from(m.as_millis()).unwrap_or(0) > 3_600_000);
        if name.starts_with(prefix) && old {
            out.push(format!("test home older than an hour: rm -r {}", e.path().display()));
        }
    }
}
