use mork_core::decide::JobEnd;
use mork_types::JobId;
use mork_types::card::Mode;
use mork_types::review::{Review, Threads};
use mork_types::state::Job;
use mork_types::work::Purpose;
use serde_json::json;

use super::{Engine, Input};
use crate::forge;

impl Engine {
    pub(super) fn read_review_threads(&mut self, job: &Job, end: &JobEnd) -> bool {
        let Some(Purpose::Clone { card, .. }) = &job.purpose else { return false };
        let JobEnd::Finished { reply: Ok(_), .. } = end else { return false };
        let Some(c) = self.state.cards.get(card).filter(|c| c.mode == Mode::Review) else { return false };
        let Some(pr) = c.pr else { return false };
        if mork_core::queue::approval::owner(&self.state, c.project.as_str(), pr).is_some() {
            return false;
        }
        let ids: Vec<_> = c
            .review
            .as_deref()
            .into_iter()
            .flat_map(Review::open_findings)
            .filter_map(|f| Some((f.n, f.comment?)))
            .collect();
        if ids.is_empty() {
            return false;
        }
        let project = self.ctx.config.projects.get(&c.project).cloned();
        let readers = self.pipe.readers.clone();
        let (job, mut end, tx) = (job.id.clone(), end.clone(), self.tx.clone());
        self.pipe.review_threads.insert(job.clone());
        std::thread::spawn(move || {
            let threads = read(&readers, project.as_ref(), pr, &ids);
            if let JobEnd::Finished { reply: Ok(v), .. } = &mut end {
                v["threads"] = json!(threads);
            }
            let _ = tx.send(Input::ReviewThreads { job, end: Box::new(end) });
        });
        true
    }

    pub(super) fn review_threads_read(&mut self, id: &JobId, end: JobEnd) {
        self.pipe.review_threads.remove(id);
        let Some(job) = self.state.jobs.get(id).filter(|j| j.settled.is_none()).cloned() else { return };
        self.finish_work(&job, if job.stop_requested { JobEnd::Stopped } else { end });
    }
}

fn read(
    readers: &forge::Readers,
    project: Option<&mork_types::config::ProjectConfig>,
    pr: u64,
    ids: &[(u32, u64)],
) -> Threads {
    let repo = project
        .ok_or_else(|| "the project is gone".to_owned())
        .and_then(|p| readers.read(&p.forge).map_err(|e| format!("{e:?}")));
    match repo {
        Ok(repo) => repo.threads(pr, ids),
        Err(words) => ids.iter().map(|(n, _)| (*n, Err(words.clone()))).collect(),
    }
}
