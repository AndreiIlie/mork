//! `mork doctor` (1.9): reports what is stale or orphaned, and changes nothing. One line
//! per finding, with the command that fixes it where one exists.

use std::collections::BTreeMap;
use std::path::Path;

use mork_core::state::State;

use crate::builds;
use crate::jobs::context::{bwrap_path, wall_mode, wall_unavailable};
use crate::jobs::procs;
use crate::paths::Home;
use crate::store::{Store, StoreError};

/// What doctor knows of the state's jobs: settled or not, and when.
pub type JobsSeen = BTreeMap<String, Option<i64>>;

const DAY: i64 = 86_400_000;

fn load_state(store: &Store, findings: &mut Vec<String>) -> Option<State> {
    store.load().inspect_err(|e| findings.push(e.to_string())).ok()
}

/// A state with complete clone ownership, or a finding explaining why it couldn't be read.
pub fn read_state(home: &Home, store: &Store, findings: &mut Vec<String>) -> Option<State> {
    let state = load_state(store, findings)?;
    with_clones(home, store, state, findings)
}

// The ended cards a clone is named for, which a start leaves in the store (fac-336): read, so a
// closed review's proofs are kept (fac-276).
fn with_clones(home: &Home, store: &Store, mut state: State, findings: &mut Vec<String>) -> Option<State> {
    let names = std::fs::read_dir(home.clones()).into_iter().flatten().flatten();
    let ids = names.map(|e| mork_types::CardId::new(e.file_name().to_string_lossy()));
    let ended = ids.filter(|c| state.unloaded.cards.contains(c)).collect();
    let records =
        store.cards_records(&ended).inspect_err(|e| findings.push(format!("cannot read ended cards: {e}"))).ok()?;
    for (id, r) in records {
        state.absorb(&id, r);
    }
    Some(state)
}

/// Every job the store holds, ended cards' included (fac-336), and a settled one whose folder was
/// pruned and `status` leaves out, so a process carrying its `MORK_JOB` is still reported (M5.30).
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
        Err(e @ StoreError::Version { .. }) => {
            findings.push(e.to_string());
            return (None, None);
        }
        Err(e) => {
            let backup = builds::latest_backup(home).unwrap_or_else(|| "none".into());
            findings.push(format!(
                "the state can't be opened: {e}. The latest backup is {backup}; with Mork stopped, copy it over {}",
                home.state_db().display()
            ));
            return (None, None);
        }
    };
    let state = load_state(&store, findings);
    let jobs = state.as_ref().and_then(|_| every_job(&store));
    let state = state.and_then(|s| with_clones(home, &store, s, findings));
    if store.pragma("quick_check").ok().as_deref() != Some("ok") {
        let backup = builds::latest_backup(home).unwrap_or_else(|| "none".into());
        findings.push(format!(
            "the state failed SQLite's quick_check. The latest backup is {backup}; with Mork stopped, copy it over {}",
            home.state_db().display()
        ));
        return (None, state);
    }
    (jobs, state)
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
        out.push(format!("pin names {p}, which has no build: mork build {p}"));
    }
    if home.pin_next().exists() {
        out.push(format!("a pin.next is left from a reload that never finished: {}", home.pin_next().display()));
    }
    if let Some(why) = wall_unavailable(wall_mode(), &bwrap_path()) {
        out.push(format!("a wall can't start: {why}"));
    }
    test_homes(tmp, "mork-test-", &mut out);
    test_namespaces(&home.agent_home(), &mut out);
    // Without a readable state we cannot know which clones are live, so give no deletion advice (fac-725).
    if let Some(state) = state {
        orphan_clones(home, state, &mut out);
    }
    out
}

fn proofs_only(dir: &std::path::Path) -> bool {
    let mut names = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name());
    names.all(|n| n == ".git")
}

fn orphan_clones(home: &Home, state: &State, out: &mut Vec<String>) {
    for e in std::fs::read_dir(home.clones()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let owned = state.cards.get(&mork_types::CardId::new(name.clone())).is_some_and(|c| {
            // A closed review keeps only its `.git/mork/`, for the head that reopens it (fac-276, fac-601).
            c.stage.is_live() || (c.mode == mork_types::card::Mode::Review && proofs_only(&e.path()))
        });
        if !owned {
            out.push(format!("clone {name} belongs to no live card: rm -r {}", e.path().display()));
        }
    }
    let kept = mork_core::queue::candidates::clones(state);
    for e in std::fs::read_dir(home.trains()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !kept.contains(&name) {
            out.push(format!("train clone {name} belongs to no candidate: rm -r {}", e.path().display()));
        }
    }
    for c in state.cards.values() {
        let made = c.stage.is_live() && c.stage != mork_types::card::Stage::Starting;
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
        if let Some(job) = procs::environ_var(pid, "MORK_JOB")
            && jobs.get(&job).is_some_and(Option::is_some)
        {
            out.push(format!("process {pid} carries the MORK_JOB of settled job {job}: kill {pid}"));
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
        let old =
            modified.is_some_and(|m| crate::clock::now_ms() - i64::try_from(m.as_millis()).unwrap_or(0) > 3_600_000);
        if name.starts_with(prefix) && old {
            out.push(format!("test home older than an hour: rm -r {}", e.path().display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // why: a fixture that cannot be created or read fails the test.
    #[allow(clippy::unwrap_used)]
    fn unreadable_ended_card_prevents_clone_advice() {
        let home = Home::new(std::env::temp_dir().join(format!("mork-doctor-{}", crate::random::uuid())));
        std::fs::create_dir_all(home.clones().join("ex-1")).unwrap();
        Store::open(&home.state_db(), "test").unwrap();
        let row = r#"INSERT INTO entities VALUES ('card', 'ex-1', '{"stage":"done"}')"#;
        rusqlite::Connection::open(home.state_db()).unwrap().execute(row, []).unwrap();

        let mut offline = Vec::new();
        let (jobs, state) = from_file(&home, &mut offline);
        assert!(state.is_none());
        assert!(jobs.is_some());
        assert_eq!(offline.len(), 1);
        assert!(offline[0].contains("cannot read ended cards"));

        let store = Store::open_read_only(&home.state_db()).unwrap();
        let mut online = Vec::new();
        assert!(read_state(&home, &store, &mut online).is_none());
        assert_eq!(online, offline);
        drop(store);
        std::fs::remove_dir_all(home.root()).unwrap();
    }
}
