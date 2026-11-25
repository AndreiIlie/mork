// What a job of no agent's turn found (section 4), read from its `io/` when it ended: a clone's copy files put in,
// a bundle's status, the checks' report with its findings located.

use std::path::{Path, PathBuf};

use factory_core::decide::{Event, JobEnd};
use factory_types::job::ExitRecord;
use factory_types::state::Job;
use factory_types::work::Purpose;
use factory_types::{CardId, Sha};
use serde_json::{Value, json};

use super::Engine;
use super::work::{done, failed, finished};
use crate::git::{self, mirror};

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

impl Engine {
    /// A work job ended: its result applied once, as `JobEnded`.
    pub(super) fn settle_work(&mut self, job: &Job, exit: Option<&ExitRecord>) {
        // Even a stopped candidate must keep its bundle and clone until its worker returns (fac-537).
        if self.pipe.work.settling.contains_key(&job.id) {
            return;
        }
        if self.pipe.review_threads.contains(&job.id) && !job.stop_requested {
            return;
        }
        let Some(purpose) = job.purpose.clone() else { return };
        let dir = self.job_dir(&job.id);
        let end = if job.stop_requested || exit.is_some_and(|e| e.stopped) {
            JobEnd::Stopped
        } else {
            match (exit, exit.and_then(|e| e.code)) {
                (_, Some(code)) => {
                    if code == 0 && self.read_candidate(job, &purpose, &dir.io()) {
                        return;
                    }
                    self.work_result(job, &purpose, &dir.io(), code, &self.failure_words(&purpose, &dir, code))
                }
                // No end at all: its supervisor died, and the job is cut (1.4.6, M3.10).
                (None, None) => JobEnd::Cut { session: factory_core::decide::SessionSeen::default() },
                (Some(e), None) => failed(e.error.clone().unwrap_or_else(|| self.words(&dir))),
            }
        };
        if self.read_review_threads(job, &end) {
            return;
        }
        self.finish_work(job, end);
    }

    pub(super) fn finish_work(&mut self, job: &Job, end: JobEnd) {
        self.pipe.review_threads.remove(&job.id);
        crate::faults::hit("job.before-apply");
        self.apply_with(&Event::JobEnded { id: job.id.clone(), end }, Some("job.mid-apply"));
        crate::faults::hit("job.after-apply");
        self.live.remove(&job.id);
    }

    fn words(&self, dir: &crate::jobs::folder::JobDir) -> String {
        let err = std::fs::read_to_string(dir.err()).unwrap_or_default();
        let tail: Vec<&str> = err.lines().rev().take(12).collect();
        let w = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        if w.trim().is_empty() { "it ended with no words".into() } else { w }
    }

    // An install's failure is in the last lines it printed, not the first of its warnings.
    fn failure_words(&self, purpose: &Purpose, dir: &crate::jobs::folder::JobDir, code: i32) -> String {
        if !matches!(purpose, Purpose::Install { .. }) || code == 0 {
            return self.words(dir);
        }
        let last = |p: PathBuf| {
            let text = std::fs::read_to_string(p).unwrap_or_default();
            let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            lines[lines.len().saturating_sub(3)..].join(" / ")
        };
        let tail = Some(last(dir.out())).filter(|t| !t.is_empty()).unwrap_or_else(|| last(dir.err()));
        let tail = if tail.is_empty() { self.words(dir) } else { tail };
        format!("{}{code}): {tail}", factory_core::decide::jobs::EXIT_MARK)
    }

    fn work_result(&mut self, job: &Job, purpose: &Purpose, io: &Path, code: i32, words: &str) -> JobEnd {
        let ok_exit = code == 0;
        let r = match purpose {
            Purpose::WallTests { .. } => return finished(json!({"exit": code})),
            Purpose::Check { card, head, .. } => {
                return self.check_result(card, head, io, code, job.checks_at.as_deref());
            }
            _ if !ok_exit => return failed(words.to_owned()),
            Purpose::Clone { card, at, .. } => self.cloned(card, at.as_str(), io),
            // Andrei may resolve a finding while the install runs (fac-298).
            Purpose::Install { card, cloned: Some(_) } => Ok(finished(json!({"resolved": self.andrei_resolved(card)}))),
            Purpose::Install { .. } => Ok(done()),
            Purpose::Bundle { .. } => {
                read_json(&io.join("status.json")).map(finished).ok_or_else(|| "it wrote no status.json".to_owned())
            }
            Purpose::Suite { .. } | Purpose::JoinTest { .. } | Purpose::Incorporate { .. } | Purpose::Final { .. } => {
                Ok(done())
            }
            Purpose::Candidate { .. } => Err("a candidate is read on its worker".into()),
            Purpose::TrainRun { .. } => self.train_result(job.id.as_str(), purpose, io, job.checks_at.as_deref()),
            Purpose::Build { .. } => Ok(done()),
        };
        r.unwrap_or_else(failed)
    }

    // The project's `copy` files go in as plain files only (4.2.2).
    fn cloned(&self, card: &CardId, at: &str, io: &Path) -> Result<JobEnd, String> {
        let c = self.card(card)?;
        let p = self.project(&c.project)?;
        let dir = self.clone_dir(card.as_str());
        for f in &p.copy {
            copy_in(&Path::new(&p.repo).join(f), &dir, f).map_err(|e| format!("the copy file {f}: {e}"))?;
        }
        // A review's branch, or a later run's, forks from the base before its tip (fac-111).
        let fork = match c.base_tip.as_ref().filter(|t| c.mode == factory_types::card::Mode::Review || t.as_str() != at)
        {
            Some(tip) => {
                let m = mirror::ensure(&self.opts.home, &c.project)?;
                mirror::merge_base(&self.opts.home, &m, at, tip.as_str()).unwrap_or_else(|_| at.to_owned())
            }
            None => at.to_owned(),
        };
        let resolved = self.andrei_resolved(card);
        let mut result = json!({"fork": fork, "resolved": resolved});
        if let Some(rebased) = read_json(&io.join("rebased.json")) {
            result["rebased"] = rebased;
        }
        // An earlier run's mock goes on in this clone (fac-591).
        if let Some(files) = super::mock_kept::put_into(&self.opts.home, card, c.run, &dir)? {
            result["mock"] = json!({"files": files});
        }
        if let Some(run) = &c.lookback {
            let from = self.write_lookback(&c, run, at, &dir)?;
            result["lookback"] = json!({"from": from});
        }
        Ok(finished(result))
    }

    // Andrei's resolutions, which a round's request tells its reviewers not to raise again (fac-298).
    fn andrei_resolved(&self, card: &CardId) -> Vec<u64> {
        let events = self.store.card_events(card.as_str()).unwrap_or_default();
        let by_andrei = events.iter().filter(|e| e.2 == "review.resolved" && e.3["from"] == "andrei");
        by_andrei.filter_map(|e| e.3["n"].as_u64()).collect()
    }

    fn check_result(&self, card: &CardId, head: &Sha, io: &Path, code: i32, started: Option<&str>) -> JobEnd {
        let report = read_json(&io.join("checks.json"));
        let run = self.final_checks(card, head, &self.clone_dir(card.as_str()), code, report, started);
        finished(serde_json::to_value(run).unwrap_or_default())
    }

    /// The checks folder's commit, and whether it has uncommitted changes (4.7.2).
    pub(super) fn checks_folder(&self, card: &CardId) -> (String, bool) {
        let Some(folder) =
            self.card(card).ok().and_then(|c| self.project(&c.project).ok()).and_then(|p| p.checks).map(|k| k.folder)
        else {
            return (String::new(), false);
        };
        let t = std::time::Duration::from_secs(30);
        let dir = Path::new(&folder);
        let commit = git::git(&self.opts.home, dir, &["rev-parse", "HEAD"], t).unwrap_or_default();
        let dirty = git::git(&self.opts.home, dir, &["status", "--porcelain"], t).map_or(true, |s| !s.is_empty());
        (commit, dirty)
    }

    /// What a wall mounts, read-only, for a checks folder: its git checkout's root, for the checks to
    /// read their own commit (fac-165); the folder itself when it is in none.
    pub(super) fn checks_mount(&self, folder: &str) -> String {
        let t = std::time::Duration::from_secs(30);
        git::git(&self.opts.home, Path::new(folder), &["rev-parse", "--show-toplevel"], t)
            .ok()
            .filter(|root| !root.is_empty())
            .unwrap_or_else(|| folder.to_owned())
    }
}

// A plain file, read and written without following a symlink, under folders that are folders, never
// links the clone's commits planted (R9).
pub(super) fn copy_in(src: &Path, clone: &Path, rel: &str) -> std::io::Result<()> {
    let bad = |why: &str| std::io::Error::other(why.to_owned());
    let mut from = factory_sys::open_no_follow(src)?;
    if !from.metadata()?.is_file() {
        return Err(bad("it isn't a plain file"));
    }
    let rel = Path::new(rel);
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(bad("its path leaves the clone"));
    }
    let mut at = clone.to_path_buf();
    for part in rel.parent().into_iter().flat_map(Path::components) {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(bad("a folder on its path is a link or a file in the clone")),
            Err(_) => std::fs::create_dir(&at)?,
        }
    }
    let mut to = factory_sys::create_no_follow(&clone.join(rel))?;
    std::io::copy(&mut from, &mut to)?;
    Ok(())
}
