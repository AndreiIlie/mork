use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use factory_core::state::Change;
use factory_types::action::{ActionStatus, ActionWhat};
use factory_types::work::Purpose;
use factory_types::{ActionId, CardId, JobId};

use super::work::Plan;
use super::{Engine, Input};

#[derive(Debug, Default)]
pub(super) struct Workers {
    preparing: BTreeMap<String, Preparation>,
    pub settling: BTreeMap<JobId, CardId>,
    waiting: Vec<Purpose>,
    removals: BTreeSet<ActionId>,
}

#[derive(Debug)]
struct Preparation {
    purpose: Purpose,
    run: u32,
    cancelled: bool,
}

pub struct Prepared {
    purpose: Purpose,
    result: Result<Plan, String>,
    exports: Vec<PathBuf>,
}

pub(super) fn prepares(purpose: &Purpose) -> bool {
    match purpose {
        Purpose::Clone { .. } | Purpose::Candidate { .. } => true,
        Purpose::Bundle { base_in, recover, refresh, .. } => {
            base_in.is_some() || recover.is_some() || refresh.is_some()
        }
        _ => false,
    }
}

impl Workers {
    fn uses(&self, card: &CardId) -> bool {
        self.preparing.values().any(|p| p.purpose.card() == Some(card)) || self.settling.values().any(|c| c == card)
    }
}

impl Engine {
    pub(super) fn wait_for_work(&mut self, purpose: &Purpose) -> bool {
        let workers = &mut self.pipe.work;
        if workers.preparing.get(&purpose.key()).is_some_and(|p| !p.cancelled) {
            return true;
        }
        let Some(card) = purpose.card() else { return false };
        let preparing = workers.preparing.values().any(|p| p.purpose.card() == Some(card));
        let removing = self.state.actions.values().any(|a| {
            a.status == ActionStatus::Started
                && matches!(&a.what, ActionWhat::RemoveClone { card: c, .. } | ActionWhat::RemoveTrain { card: c, .. } if c == card)
        });
        if !preparing && !removing {
            return false;
        }
        if !workers.waiting.contains(purpose) {
            workers.waiting.push(purpose.clone());
        }
        true
    }

    pub(super) fn prepare_work(&mut self, purpose: &Purpose) {
        let work = match self.worktree(purpose) {
            Ok(work) => work,
            Err(why) => return self.work_refused(purpose, &why, crate::clock::now_ms()),
        };
        let pending = Preparation { purpose: purpose.clone(), run: work.card.run, cancelled: false };
        self.pipe.work.preparing.insert(purpose.key(), pending);
        let lock = self.pipe.locks.of(&work.card.project);
        let (purpose, tx) = (purpose.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let _held = lock.lock();
            let mut exports = Vec::new();
            let result = work.prepare(&purpose, &mut exports);
            let _ = tx.send(Input::WorkPrepared(Box::new(Prepared { purpose, result, exports })));
        });
    }

    pub(super) fn work_prepared(&mut self, prepared: Prepared) {
        let Prepared { purpose, result, exports } = prepared;
        let pending = self.pipe.work.preparing.remove(&purpose.key());
        // A closed card's recovery bundle is still owed: only a stop or a new run cancels (fac-537).
        let current = pending.is_some_and(|p| {
            !p.cancelled
                && purpose.card().and_then(|c| self.state.cards.get(c)).is_some_and(|c| c.run == p.run)
                && self.state.owed_work.contains_key(&purpose.key())
        });
        if !current || result.is_err() {
            self.drop_bundles(&exports);
        }
        if !current {
            return;
        }
        let now = crate::clock::now_ms();
        match result {
            Ok(plan) => {
                if !self.launch_work(&purpose, plan, now) {
                    self.drop_bundles(&exports);
                }
            }
            Err(why) => self.work_refused(&purpose, &why, now),
        }
    }

    pub(super) fn work_changed(&mut self, changes: &[Change]) {
        // A preparation has no recorded job for a stop to mark, and a resume may owe the same purpose again.
        for change in changes {
            if let Change::RemoveOwedWork { key } = change {
                if let Some(p) = self.pipe.work.preparing.get_mut(key) {
                    p.cancelled = true;
                }
                self.pipe.work.waiting.retain(|w| w.key() != *key);
            }
        }
    }

    pub(super) fn wait_to_remove(&mut self, id: &str) -> bool {
        let busy = self.state.actions.get(id).is_some_and(|a| match &a.what {
            ActionWhat::RemoveClone { card, .. } => self.pipe.work.uses(card) || self.runs_in_clone(card),
            ActionWhat::RemoveTrain { card, .. } => self.pipe.work.uses(card),
            _ => false,
        });
        if !busy {
            return false;
        }
        self.pipe.work.removals.insert(ActionId::new(id));
        true
    }

    // A turn still in the clone, a closing turn its card's end doesn't stop among them, keeps it (fac-506).
    fn runs_in_clone(&self, card: &CardId) -> bool {
        let dir = self.opts.home.clones().join(card.as_str());
        self.state.jobs.values().any(|j| j.settled.is_none() && Path::new(&j.spec.cwd).starts_with(&dir))
    }

    pub(super) fn work_tick(&mut self) {
        for purpose in std::mem::take(&mut self.pipe.work.waiting) {
            self.start_work(&purpose);
        }
        for id in std::mem::take(&mut self.pipe.work.removals) {
            if self.state.actions.get(&id).is_some_and(|a| a.status == ActionStatus::Pending && !a.cancelled) {
                self.run_action(id.as_str());
            }
        }
    }
}
