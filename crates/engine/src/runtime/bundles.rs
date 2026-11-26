// Bundles (fac-175): a mirror's copy goes once its job's folder holds it, a job's own once it settles or the
// action taking it is done; what is left of the mirror's goes at the engine's start.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mork_core::state::Change;
use mork_types::JobId;
use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::state::Job;

use super::Engine;

impl Engine {
    // A bundle written beside a mirror for a job to copy in (`<project>.bundles/`): never an
    // image, nor a retried turn's copy from an older job's `io/`, which must stay.
    fn mirrors_bundle(&self, src: &str) -> bool {
        Path::new(src).parent().is_some_and(|d| {
            d.parent() == Some(self.opts.home.mirrors().as_path()) && d.extension().is_some_and(|e| e == "bundles")
        })
    }

    /// The job's folder is written: the mirror's bundles it copied go. Until then recovery
    /// writes the folder from them (1.4.6).
    pub(super) fn drop_sources(&self, job: &Job) {
        for src in job.io_copies.values().filter(|s| self.mirrors_bundle(s)) {
            self.remove_bundle(Path::new(src));
        }
    }

    /// At start: a mirror's bundle no unsettled job copies in is a crash's leftover, or a planned
    /// job's that was never recorded.
    pub(super) fn sweep_bundles(&self) {
        let named: BTreeSet<PathBuf> = self
            .state
            .jobs
            .values()
            .filter(|j| j.settled.is_none())
            .flat_map(|j| j.io_copies.values().map(PathBuf::from))
            .collect();
        let stores = std::fs::read_dir(self.opts.home.mirrors()).into_iter().flatten().flatten().map(|e| e.path());
        for store in stores.filter(|d| d.is_dir() && d.extension().is_some_and(|e| e == "bundles")) {
            for f in std::fs::read_dir(&store).into_iter().flatten().flatten().map(|e| e.path()) {
                if !named.contains(&f) {
                    self.remove_bundle(&f);
                }
            }
        }
    }

    /// The bundles a commit's changes used up, read before the state takes them: a settled work
    /// job's in its `io/`, but the one a take naming it still reads; a take done, that one.
    pub(super) fn used_bundles(&self, changes: &[Change]) -> Vec<PathBuf> {
        let mut used = Vec::new();
        for c in changes {
            match c {
                Change::PutJob(j) if j.purpose.is_some() && j.settled.is_some() => {
                    if self.state.jobs.get(&j.id).is_some_and(|old| old.settled.is_some()) {
                        continue;
                    }
                    let taken = self.taken_later(changes, &j.id);
                    let io = self.opts.home.job(j.id.as_str()).join("io");
                    let all = std::fs::read_dir(io).into_iter().flatten().flatten().map(|e| e.path());
                    let bundle = |f: &PathBuf| f.extension().is_some_and(|e| e == "bundle");
                    used.extend(all.filter(|f| bundle(f) && taken.is_none_or(|t| !f.ends_with(t))));
                }
                Change::PutAction(a) if a.status == ActionStatus::Done => {
                    if self.state.actions.get(&a.id).is_some_and(|old| old.status == ActionStatus::Done) {
                        continue;
                    }
                    let (job, name) = match &a.what {
                        ActionWhat::Take { job, .. } => (job, "out.bundle"),
                        _ => continue,
                    };
                    used.push(self.opts.home.job(job.as_str()).join("io").join(name));
                }
                _ => {}
            }
        }
        used
    }

    // The bundle a take naming the job will still read: one the settle's decision makes, or one
    // already there. A take of the same head that ran, or another job's, leaves none (gates).
    fn taken_later(&self, changes: &[Change], job: &JobId) -> Option<&'static str> {
        let made: Vec<&Action> = changes
            .iter()
            .filter_map(|c| match c {
                Change::PutAction(a) => Some(&**a),
                _ => None,
            })
            .collect();
        let had = self.state.actions.values().filter(|a| !made.iter().any(|m| m.id == a.id));
        made.iter()
            .copied()
            .chain(had)
            .filter(|a| matches!(a.status, ActionStatus::Pending | ActionStatus::Started | ActionStatus::Held))
            .find_map(|a| match &a.what {
                ActionWhat::Take { job: j, .. } if *j == *job => Some("out.bundle"),
                _ => None,
            })
    }

    pub(super) fn drop_bundles(&self, used: &[PathBuf]) {
        for b in used {
            self.remove_bundle(b);
        }
    }

    fn remove_bundle(&self, path: &Path) {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                self.log(&format!("the bundle {} couldn't be removed: {e}", path.display()));
            }
            _ => {}
        }
    }
}
