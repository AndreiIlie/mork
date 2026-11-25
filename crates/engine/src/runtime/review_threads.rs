use factory_core::decide::JobEnd;
use factory_types::JobId;
use factory_types::card::Mode;
use factory_types::review::{Review, Threads};
use factory_types::state::Job;
use factory_types::work::Purpose;
use serde_json::json;

use super::{Engine, Input};
use crate::bitbucket::{calls, parse};

impl Engine {
    pub(super) fn read_review_threads(&mut self, job: &Job, end: &JobEnd) -> bool {
        let Some(Purpose::Clone { card, .. }) = &job.purpose else { return false };
        let JobEnd::Finished { reply: Ok(_), .. } = end else { return false };
        let Some(c) = self.state.cards.get(card).filter(|c| c.mode == Mode::Review) else { return false };
        let Some(pr) = c.pr else { return false };
        if factory_core::queue::approval::owner(&self.state, c.project.as_str(), pr).is_some() {
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
        let (job, mut end, tx) = (job.id.clone(), end.clone(), self.tx.clone());
        self.pipe.review_threads.insert(job.clone());
        std::thread::spawn(move || {
            let threads = read(project.as_ref(), pr, &ids);
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

fn read(project: Option<&factory_types::config::ProjectConfig>, pr: u64, ids: &[(u32, u64)]) -> Threads {
    let repo = project
        .ok_or_else(|| "the project is gone".to_owned())
        .and_then(|p| super::act_bitbucket::repo(p).map_err(|e| format!("{e:?}")));
    let (repo, all) =
        match repo.and_then(|r| calls::thread_comments(&r, pr).map(|all| (r, all)).map_err(|e| format!("{e:?}"))) {
            Ok(read) => read,
            Err(words) => return ids.iter().map(|(n, _)| (*n, Err(words.clone()))).collect(),
        };
    ids.iter()
        .map(|(n, id)| {
            let thread = calls::thread_root(&repo, pr, *id)
                .map_err(|e| format!("{e:?}"))
                .and_then(|root| parse::thread(&root, &all));
            (*n, thread)
        })
        .collect()
}
