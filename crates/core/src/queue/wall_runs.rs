//! Connection-bound wall-test jobs (fac-480): permissions, cancellation and settlement stay separate from gates.

use factory_types::card::Mode;
use factory_types::reply::Role;
use factory_types::state::{Job, Settlement};
use factory_types::work::Purpose;
use factory_types::{AgentKey, CardId};
use serde_json::json;

use crate::decide::{Ctx, Decision, JobEnd};
use crate::post::Poster;
use crate::state::{Change, State};

pub(crate) fn command(
    state: &State,
    key: &AgentKey,
    card: &CardId,
    words: &[String],
    ctx: Ctx<'_>,
) -> Result<Vec<String>, String> {
    let c = state.cards.get(card).ok_or("no card to run wall tests in")?;
    if c.project != ctx.config.factory.project {
        return Err("`factory test --walls` is only for the factory's own project".into());
    }
    if c.mode != Mode::Full {
        return Err("`factory test --walls` is only for a full card".into());
    }
    if !state.agents.get(key).is_some_and(|a| a.role == Role::Foreman) {
        return Err("`factory test --walls` is only for a full card's foreman".into());
    }
    let project = ctx.config.projects.get(&c.project).ok_or("the card's project is not configured")?;
    super::own_runs::walls_argv(project, words)
}

pub(crate) fn stop(p: &mut Poster<'_>, lease: &str, ctx: Ctx<'_>) {
    let holds = |purpose: &Purpose| matches!(purpose, Purpose::WallTests { lease: id, .. } if id.as_str() == lease);
    for purpose in p.state.owed_work.values().filter(|w| holds(w)) {
        p.other.push(Change::RemoveOwedWork { key: purpose.key() });
    }
    let jobs: Vec<Job> = p
        .state
        .jobs
        .values()
        .filter(|j| j.settled.is_none() && j.purpose.as_ref().is_some_and(holds))
        .cloned()
        .collect();
    if jobs.is_empty() {
        super::leases::free(p, lease, ctx);
    }
    for job in jobs {
        crate::lifecycle::stops::stop_job(p, job);
    }
}

pub(crate) fn ended(state: &State, job: &Job, lease: &str, end: &JobEnd, ctx: Ctx<'_>) -> Decision {
    let mut p = Poster::for_ctx(state, ctx);
    p.ending = Some(job.id.clone());
    if super::leases::live(&p).contains_key(lease) {
        super::leases::free(&mut p, lease, ctx);
    }
    let how = match end {
        JobEnd::Finished { .. } if !job.stop_requested => Settlement::Applied,
        JobEnd::Failed { words } => Settlement::Failed { words: words.clone() },
        JobEnd::Finished { .. } | JobEnd::Cut { .. } | JobEnd::Stopped => Settlement::Stopped,
    };
    p.journal(
        "job.settled",
        job.purpose.as_ref().and_then(Purpose::card).map(CardId::as_str),
        json!({"job": job.id, "how": how}),
    );
    let mut d = Decision { changes: vec![crate::agents::ends::settle(job, how, ctx.now)], ..Decision::default() };
    p.finish(&mut d);
    d
}
