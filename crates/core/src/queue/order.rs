//! Where a card stands in its merge queue (fac-68, fac-375): its place, its candidate, what is before it, and the
//! commit it lands. A published PR waits for the operator outside the queue (fac-98).

use mork_types::Sha;
use mork_types::card::{Candidate, Card, Landing, Stage};
use mork_types::work::Purpose;

use crate::post::Poster;

/// A card's candidate, at any step that has one.
pub fn candidate(c: &Card) -> Option<&Candidate> {
    match &c.landing {
        Some(Landing::Prepare(k) | Landing::Test(k) | Landing::Review(k) | Landing::Passed(k)) => Some(k),
        _ => None,
    }
}

/// Whether a card is past its candidate: landing at the train's front (its base move, its push,
/// its PR or its merge under way), or its company PR waiting.
pub fn landing(c: &Card) -> bool {
    matches!(
        c.landing,
        Some(Landing::HandMerged | Landing::Published { .. } | Landing::Approval { .. } | Landing::Merge { .. })
    )
}

/// A card's place in its train: a landing card is before every other, the rest by the time each
/// was admitted.
pub fn place(c: &Card) -> i64 {
    match &c.landing {
        Some(Landing::Queued { at } | Landing::Fetch { at }) => *at,
        Some(Landing::Prepare(k) | Landing::Test(k) | Landing::Review(k) | Landing::Passed(k)) => k.at,
        _ => i64::MIN,
    }
}

/// A project's train, in its order: its live cards merging, an alarmed one and a company PR
/// waiting for the operator aside (fac-98).
pub fn cards(p: &Poster<'_>, project: &str) -> Vec<Card> {
    let mut all: Vec<Card> = p.state.cards.keys().filter_map(|id| p.card(id)).chain(p.all_new_cards()).collect();
    all.retain(|c| c.project == project && member(c));
    all.sort_by(|a, b| (place(a), &a.id).cmp(&(place(b), &b.id)));
    all
}

/// Whether a card is in its project's train: merging with a landing, not alarmed, and not a company PR waiting out of
/// it for approval (fac-98).
pub fn member(c: &Card) -> bool {
    c.stage == Stage::Merging && c.landing.is_some() && !c.alarm && !crate::queue::approval::waiting(c)
}

/// The base's tip a project's train builds on (fac-144): what its front's candidate, the first made
/// on the base, or a landing card's, was made on.
pub fn base_on(p: &Poster<'_>, project: &str) -> Option<Sha> {
    cards(p, project).into_iter().find_map(|c| match &c.landing {
        Some(Landing::Merge { base, .. } | Landing::Published { base, .. }) => Some(base.clone()),
        _ => candidate(&c).map(|k| k.on.clone()),
    })
}

/// Whether a card is in its merge queue (fac-375): verified and waiting, landing, or made again at its front.
pub fn in_queue(c: &Card) -> bool {
    landing(c) || matches!(c.landing, Some(Landing::Passed(_))) || candidate(c).is_some_and(|k| k.queue)
}

/// How many cards of its merge queue are before this one; all of them before one still verified, and none before a
/// published PR waiting for the operator outside it (fac-98).
pub fn ahead(p: &Poster<'_>, c: &Card) -> usize {
    if crate::queue::approval::waiting(c) {
        return 0;
    }
    let all = cards(p, c.project.as_str());
    let queued = all.iter().filter(|x| x.id != c.id && in_queue(x));
    if in_queue(c) { queued.take_while(|x| place(x) <= place(c)).count() } else { queued.count() }
}

/// The commit a card in the train lands: its candidate's, or its head.
pub fn commit_of(c: &Card) -> Option<Sha> {
    match &c.landing {
        Some(Landing::Published { commit, .. } | Landing::Approval { commit, .. } | Landing::Merge { commit, .. }) => {
            commit.clone().or_else(|| c.head.clone())
        }
        _ => candidate(c).and_then(|k| k.commit.clone()).or_else(|| c.head.clone()),
    }
}

/// The train's own job of a card's: a candidate made, or run.
pub fn is_train(purpose: Option<&Purpose>) -> bool {
    matches!(purpose, Some(Purpose::Candidate { .. } | Purpose::TrainRun { .. }))
}

/// The clone a train job works in.
pub fn clone_of(purpose: &Purpose) -> Option<i64> {
    if let Purpose::Candidate { clone, .. } | Purpose::TrainRun { clone, .. } = purpose { Some(*clone) } else { None }
}
