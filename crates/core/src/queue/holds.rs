//! What a card's landing holds (fac-10, fac-98): its agents' turns from its publication until it lands, and its landing
//! actions until nothing of the card may still change under them; the tick starts them once they may.

use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::card::{Card, Mode, Stage};
use mork_types::state::Agent;
use mork_types::work::Purpose;
use mork_types::{ActionId, AgentKey, CardId, ProjectId};

use super::order::{cards, landing};
use super::train::maintain;
use crate::decide::{Ctx, Intent};
use crate::post::Poster;
use crate::state::State;

// Whether a card holds its project's landing: at the train's front, publishing or merging, with
// no alarm raised. A company PR waiting for the operator's approval holds none (fac-98).
fn holds_slot(c: &Card) -> bool {
    c.stage == Stage::Merging && !c.alarm && landing(c) && !crate::queue::approval::waiting(c)
}

// Whether landing holds a card's turns; a landing held for the operator releases them until retry.
// A PR waiting for approval holds them only while its approved merge is pending or started (fac-98).
fn holds_turns(state: &State, c: &Card) -> bool {
    let mut mine = state.actions.values().filter(|a| a.what.card() == Some(&c.id) && landing_action(&a.what));
    if crate::queue::approval::waiting(c) {
        let underway = |a: &Action| matches!(a.status, ActionStatus::Pending | ActionStatus::Started) && !a.cancelled;
        return mine.any(|a| matches!(a.what, ActionWhat::Merge { .. }) && underway(a));
    }
    holds_slot(c) && c.failed_job.is_none() && !mine.any(|a| a.status == ActionStatus::Held)
}

/// An outward step of a card's landing: its base move, its push, its PR, its merge.
pub fn landing_action(w: &ActionWhat) -> bool {
    matches!(
        w,
        ActionWhat::MoveBase { .. } | ActionWhat::Push { .. } | ActionWhat::OpenPr { .. } | ActionWhat::Merge { .. }
    )
}

/// Whether an agent's turns wait for its card's landing, in its room or a DM.
pub fn held(state: &State, a: &Agent) -> bool {
    crate::messages::places::card_of(state, &a.place)
        .and_then(|c| state.cards.get(&c))
        .is_some_and(|c| holds_turns(state, c))
}

/// Each tick, every project's train kept true.
pub fn advance(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let mut projects: Vec<ProjectId> =
        p.state.cards.values().filter(|c| c.stage == Stage::Merging).map(|c| c.project.clone()).collect();
    projects.sort();
    projects.dedup();
    for project in projects {
        // A company PR waiting out of the train merges once its card's turns ended too (fac-98).
        let waiting: Vec<Card> = p
            .state
            .cards
            .keys()
            .filter_map(|id| p.card(id))
            .filter(|c| {
                c.project == project && c.stage == Stage::Merging && !c.alarm && crate::queue::approval::waiting(c)
            })
            .collect();
        for c in cards(p, &project).into_iter().chain(waiting) {
            if (holds_slot(&c) || crate::queue::approval::waiting(&c)) && !c.paused && !busy(p, &c.id) {
                landing_waits_over(p, &c.id);
            }
        }
        maintain(p, &project, ctx);
    }
}

/// Whether a card's landing action waits to start: its turns or jobs still running, or a head on its
/// way, could change it under its landing; the tick starts it once they ended (fac-10).
pub fn landing_waits(state: &State, w: &ActionWhat) -> bool {
    // A colleague's merge lands its PR's head, which its guard reads: no clone of ours (fac-95).
    // Our company card's waits for its turns, which could change its PR's head (fac-98).
    let review = |c: &CardId| state.cards.get(c).is_some_and(|x| x.mode == Mode::Review);
    let colleague = matches!(w, ActionWhat::Merge { card, dest: Some(_), .. } if review(card));
    landing_action(w) && !colleague && w.card().is_some_and(|c| busy_with(state, state.actions.values(), c))
}

// A landing action whose start waited for the card's turns and jobs to end starts now (`started`).
fn landing_waits_over(p: &mut Poster<'_>, id: &CardId) {
    let waiting: Vec<ActionId> = p
        .all_actions()
        .filter(|a| a.status == ActionStatus::Pending && !a.cancelled && a.what.card() == Some(id))
        .filter(|a| landing_action(&a.what))
        .map(|a| a.id.clone())
        .collect();
    for id in waiting {
        p.intent(Intent::RunAction { id });
    }
}

// A turn of one of the card's agents, a job of the card's, or a head on its way into the mirror.
fn busy(p: &Poster<'_>, id: &CardId) -> bool {
    busy_with(p.state, p.all_actions(), id)
}

// Whether a card may still change, `actions` the actions as they stand: a turn of one of its
// agents, a job of its own, or a head on its way into the mirror.
fn busy_with<'a>(state: &State, actions: impl IntoIterator<Item = &'a Action>, id: &CardId) -> bool {
    let jobs = state.jobs.values().filter(|j| j.settled.is_none());
    let mine = |owner: &AgentKey| {
        state.agents.get(owner).and_then(|a| crate::messages::places::card_of(state, &a.place)).as_ref() == Some(id)
    };
    let job = jobs.into_iter().any(|j| j.purpose.as_ref().and_then(Purpose::card) == Some(id) || mine(&j.owner));
    let owed = state.owed_work.values().any(|w| w.card() == Some(id));
    let taking = actions.into_iter().any(|a| {
        matches!(&a.what, ActionWhat::Take { card, .. } if card == id)
            && matches!(a.status, ActionStatus::Pending | ActionStatus::Started)
    });
    job || owed || taking
}
