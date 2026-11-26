// The pipeline's own events (section 4): the boards read, the engine started, and time passing, with the fetches it
// makes due.

use std::collections::{BTreeMap, BTreeSet};

use mork_types::ProjectId;
use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::card::BoardCard;

use super::{Ctx, Decision, Intent};
use crate::post::Poster;
use crate::state::{Change, State};

pub(super) fn boards_read(
    state: &State,
    cards: &[BoardCard],
    prefixes: &BTreeMap<ProjectId, String>,
    ctx: Ctx<'_>,
) -> Decision {
    let mut d = Decision {
        changes: vec![Change::SetBoard { cards: cards.to_vec(), prefixes: prefixes.clone() }],
        ..Decision::default()
    };
    let mut p = Poster::for_ctx(state, ctx);
    crate::messages::announce::boards_read(&mut p, cards);
    crate::lifecycle::carried::released(&mut p, cards);
    p.finish(&mut d);
    d
}

pub(super) fn started(state: &State, ctx: Ctx<'_>) -> Decision {
    let mut d = crate::queue::leases::started(state, ctx);
    let mut p = Poster::for_ctx(state, ctx);
    crate::lifecycle::brief::recover_flow(&mut p, ctx.config);
    crate::queue::recovery::recover(&mut p);
    crate::lifecycle::branches::recover(&mut p, ctx);
    crate::reviews::verdict::recover(&mut p);
    p.finish(&mut d);
    d
}

pub(super) fn tick(state: &State, ctx: Ctx<'_>) -> Decision {
    let mut p = Poster::for_ctx(state, ctx);
    crate::queue::holds::advance(&mut p, ctx);
    retry_fetches(&mut p, ctx);
    timed_fetches(&mut p, ctx);
    crate::queue::leases::expire(&mut p, ctx);
    crate::agents::closing::expire(&mut p, ctx);
    crate::agents::stalled::wake(&mut p);
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

const FETCH_EVERY_MS: i64 = 5 * 60 * 1000;

// The fetch cadence outlives a restart. Idle Bitbucket projects need a current base for look-back counts.
fn timed_fetches(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let mut projects: BTreeSet<ProjectId> =
        p.state.cards.values().filter(|c| c.stage.is_live()).map(|c| c.project.clone()).collect();
    let idle = p.state.lookbacks.iter().filter(|(project, reads)| {
        ctx.config.projects.get(*project).is_some_and(|pc| pc.forge.hosted())
            && reads.values().any(|c| c.commit.is_some())
    });
    projects.extend(idle.map(|(project, _)| project.clone()));
    for project in projects {
        let on_hosted_forge = ctx.config.projects.get(&project).is_some_and(|pc| pc.forge.hosted());
        if on_hosted_forge && p.state.world.token_refused.contains_key(&project) {
            continue;
        }
        let fetches: Vec<&Action> = p
            .all_actions()
            .filter(|a| matches!(&a.what, ActionWhat::Fetch { project: x, .. } if *x == project))
            .collect();
        // One under way, or held for the operator (2.5.7), and no other is made.
        let under_way = fetches
            .iter()
            .any(|a| matches!(a.status, ActionStatus::Pending | ActionStatus::Started | ActionStatus::Held));
        let last = p.last_fetch(project.as_str());
        if !under_way && last.is_none_or(|t| p.now >= t + FETCH_EVERY_MS) {
            p.fetch(project.as_str());
        }
    }
}

// A fetch that failed runs again at the next poll: one poll interval after each failure.
fn retry_fetches(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let every = i64::try_from(ctx.config.mork.poll_seconds).unwrap_or(60).saturating_mul(1000);
    let now = p.now;
    let again: Vec<Action> = p
        .all_actions()
        .filter(|a| a.status == ActionStatus::Pending && a.attempts > 0 && !a.cancelled)
        .filter(|a| matches!(a.what, ActionWhat::Fetch { .. }) && a.done_ms.is_none_or(|t| now >= t + every))
        .cloned()
        .collect();
    for mut a in again {
        a.done_ms = None;
        let id = a.id.clone();
        p.put_action(a);
        p.intent(Intent::RunAction { id });
    }
}
