// Recovery at start (1.4.6): each job the state doesn't record as settled is looked at,
// and the table says what to do.

use mork_core::agents::recovery::{Observed, Recovery, recover};
use mork_core::decide::{Event, JobEnd};
use mork_types::JobId;

use super::Engine;

impl Engine {
    /// The launches recovery decided, now that it has ended (2.7, step 9; T2.14).
    pub(super) fn launch_recovered(&mut self) {
        for (id, write) in std::mem::take(&mut self.pipe.recovered) {
            if write { self.launch(&id) } else { self.start_supervisor(&id) }
        }
    }

    pub(super) fn recover_jobs(&mut self) {
        let ids: Vec<JobId> = self.state.jobs.values().filter(|j| j.settled.is_none()).map(|j| j.id.clone()).collect();
        for id in ids {
            self.recover_one(&id);
        }
    }

    fn recover_one(&mut self, id: &JobId) {
        let dir = self.job_dir(id);
        let o = Observed {
            folder: dir.exists(),
            claim: dir.claim_path().exists(),
            lock_held: dir.lock_held(),
            exit: dir.exit_path().exists(),
            run: dir.run_path().exists(),
        };
        let job = self.state.jobs.get(id);
        let walls = job.is_some_and(|j| matches!(j.purpose, Some(mork_types::work::Purpose::WallTests { .. })));
        let stop = job.is_some_and(|j| j.stop_requested) || walls;
        match recover(o, stop) {
            Recovery::SettleStopped => {
                self.apply(&Event::JobEnded { id: id.clone(), end: JobEnd::Stopped });
            }
            // Decided now, started once recovery has ended (2.7, step 9).
            Recovery::WriteAndLaunch => self.pipe.recovered.push((id.clone(), true)),
            Recovery::StartSupervisor => self.pipe.recovered.push((id.clone(), false)),
            Recovery::Wait | Recovery::Reattach => {
                // A recorded stop wins: a job still running is stopped again (1.4.6.2).
                if stop {
                    self.stop_job(id);
                }
            }
            Recovery::TakeResult | Recovery::EndThenResultOrCut => self.settle(id),
            Recovery::Cut => {
                // Any other combination is recorded cut, never applied (1.4.6.1).
                self.log(&format!("{id}: found {o:?}; recorded cut"));
                self.settle_cut(id);
            }
        }
    }
}

#[cfg(test)]
mod tests;
