// The live cards' suite runs read as their output grows, for the Overview's Suite row (fac-697). Their
// counts are kept here alone, never in the state: after a restart each run is read again from its start.

use std::collections::BTreeMap;

use factory_types::JobId;
use factory_types::view::SuiteProgress;
use factory_types::work::Purpose;

use super::Engine;
use crate::jobs::suite_progress::Output;

/// A suite run's output as read so far.
#[derive(Debug, Default)]
pub struct SuiteRead {
    output: Output,
    // Its job ended and its output was read to its end: nothing more will come.
    done: bool,
}

impl Engine {
    /// Reads what each live card's suite runs printed since the last tick; a run that moved the
    /// counts shows on the views' next send.
    pub(super) fn watch_suites(&mut self) {
        let live = |c: &factory_types::CardId| self.state.cards.get(c).is_some_and(|c| c.stage.is_live());
        let runs: BTreeMap<JobId, bool> = self
            .state
            .jobs
            .values()
            .filter(|j| matches!(&j.purpose, Some(Purpose::TrainRun { card, test: Some(_), .. }) if live(card)))
            .map(|j| (j.id.clone(), j.settled.is_some()))
            .collect();
        self.suites.retain(|id, _| runs.contains_key(id));
        for (id, ended) in runs {
            let file = self.job_dir(&id).io().join("suite.out");
            let read = self.suites.entry(id).or_default();
            if read.done {
                continue;
            }
            let before = read.output.progress();
            read.output.more(&file);
            if ended {
                read.output.close();
                read.done = true;
            }
            if read.output.progress() != before {
                self.views_cache.stale = true;
            }
        }
    }

    /// Each suite run's counts so far, by its job.
    pub(super) fn suite_counts(&self) -> BTreeMap<JobId, SuiteProgress> {
        self.suites.iter().filter_map(|(id, r)| Some((id.clone(), r.output.progress()?))).collect()
    }
}
