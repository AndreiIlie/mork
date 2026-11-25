use std::collections::BTreeSet;
use std::path::Path;

use factory_core::decide::JobEnd;
use factory_types::state::Job;
use factory_types::work::{Made, Purpose};
use factory_types::{JobId, Sha};

use super::queue_work::read_json;
use super::work::{failed, finished};
use super::work_plan::Worktree;
use super::{Engine, Input};
use crate::git::mirror;

impl Engine {
    pub(super) fn read_candidate(&mut self, job: &Job, purpose: &Purpose, io: &Path) -> bool {
        if !matches!(purpose, Purpose::Candidate { .. }) {
            return false;
        }
        let work = match self.worktree(purpose) {
            Ok(work) => work,
            Err(why) => {
                self.finish_work(job, failed(why));
                return true;
            }
        };
        let paths = factory_core::reviews::gate::paths(&self.state, &work.card);
        let before = factory_core::reviews::gate::last_read(&self.state, &work.card);
        let lock = self.pipe.locks.of(&work.card.project);
        self.pipe.work.settling.insert(job.id.clone(), work.card.id.clone());
        let (job, purpose, io, tx) = (job.id.clone(), purpose.clone(), io.to_path_buf(), self.tx.clone());
        std::thread::spawn(move || {
            let _held = lock.lock();
            let end = work.made(&purpose, &io, &paths, before.as_ref()).unwrap_or_else(failed);
            let _ = tx.send(Input::CandidateRead { job, end: Box::new(end) });
        });
        true
    }

    pub(super) fn candidate_read(&mut self, id: &JobId, end: JobEnd) {
        self.pipe.work.settling.remove(id);
        let Some(job) = self.state.jobs.get(id).filter(|j| j.settled.is_none()).cloned() else { return };
        self.finish_work(&job, if job.stop_requested { JobEnd::Stopped } else { end });
    }
}

impl Worktree {
    // The commit made goes into the mirror as `refs/train/<card>`, with the project's copy files
    // into the clone; a conflict names its paths alone (4.12.1).
    pub(super) fn made(
        &self,
        purpose: &Purpose,
        io: &Path,
        paths: &BTreeSet<String>,
        before: Option<&(Sha, Sha)>,
    ) -> Result<JobEnd, String> {
        let Purpose::Candidate { card, clone, .. } = purpose else { return Err("not a candidate".into()) };
        let c = &self.card;
        let p = &self.project;
        let v = read_json(&io.join("made.json")).ok_or("it wrote no made.json")?;
        let mut m: Made = serde_json::from_value(v).map_err(|e| format!("made.json: {e}"))?;
        if !m.conflicts.is_empty() {
            return Ok(finished(serde_json::to_value(m).map_err(|e| e.to_string())?));
        }
        let (home, mir) = (&self.home, mirror::ensure(&self.home, &c.project)?);
        let bundle = io.join("out.bundle");
        if bundle.is_file() {
            let got = mirror::bundle_in(home, &mir, &bundle, &format!("refs/train/{card}"))?;
            if m.commit.as_ref().map(Sha::as_str) != Some(got.as_str()) {
                return Err(format!("the bundle holds {got}, not the candidate its result named"));
            }
        }
        self.read_contents(&mut m, &mir, purpose, paths, before)?;
        let dir = self.home.trains().join(format!("{card}-{clone}"));
        for f in &p.copy {
            super::work_end::copy_in(&Path::new(&p.repo).join(f), &dir, f)
                .map_err(|e| format!("the copy file {f}: {e}"))?;
        }
        Ok(finished(serde_json::to_value(m).map_err(|e| e.to_string())?))
    }

    fn read_contents(
        &self,
        m: &mut Made,
        mir: &Path,
        purpose: &Purpose,
        paths: &BTreeSet<String>,
        before: Option<&(Sha, Sha)>,
    ) -> Result<(), String> {
        let Purpose::Candidate { head, on, .. } = purpose else { return Err("not a candidate".into()) };
        let Some(commit) = &m.commit else { return Ok(()) };
        let home = &self.home;
        m.contents = mirror::contents(home, mir, on.as_str(), commit.as_str())?;
        let mut changed: BTreeSet<String> = m.contents.keys().cloned().collect();
        let rest: Vec<&str> = paths.iter().map(String::as_str).filter(|f| !m.contents.contains_key(*f)).collect();
        m.contents.extend(mirror::contents_at(home, mir, commit.as_str(), &rest)?);
        if factory_core::reviews::gate::entry(&self.card) {
            let last = self.card.review.as_deref().and_then(|r| r.last_read());
            let changes = match last {
                Some(r) if r.head.as_ref() == Some(head) => None,
                Some(r) => {
                    let from = r.head.as_ref().unwrap_or(&r.commit);
                    let paths = mirror::paths_changed(home, mir, from.as_str(), head.as_str())?;
                    // A change inherited from the destination leaves the candidate as the last round read it.
                    let merged = mirror::paths_changed(home, mir, r.commit.as_str(), commit.as_str()).ok();
                    let kept = |p: &&String| merged.as_ref().is_none_or(|m| m.contains(*p));
                    let paths: Vec<&str> = paths.iter().filter(kept).map(String::as_str).collect();
                    Some(mirror::contents_at(home, mir, commit.as_str(), &paths)?)
                }
                None => Some(m.contents.clone()),
            };
            m.contents.extend(changes.iter().flat_map(|c| c.clone()));
            m.review_changes = changes;
        }
        let Some((commit, base)) = before else { return Ok(()) };
        let Ok(previous) = mirror::paths_changed(home, mir, base.as_str(), commit.as_str()) else { return Ok(()) };
        // A revert still needs review even when the candidate now matches its base (fac-468).
        changed.extend(previous);
        m.contents.retain(|path, _| changed.contains(path));
        if let Some(changes) = &mut m.review_changes {
            changes.retain(|path, _| changed.contains(path));
        }
        Ok(())
    }
}
