//! The merge queue (fac-375): each card is verified on its own candidate, and a verified card waits in its project's
//! queue in order of entry; one card at a time lands at its front, its candidate made again only when the base moved.

use mork_types::Sha;
use mork_types::card::{Candidate, Card, Landing};
use serde_json::{Value, json};

use super::candidates::{jobs_of, reset, start_on};
use super::order::{candidate, cards, in_queue, landing};
use crate::decide::{Ctx, Intent};
use crate::post::Poster;

/// The queue of a project kept true: a waiting card's verification started, and the first verified card landed once no
/// other card of the queue is landing or verified again.
pub fn maintain(p: &mut Poster<'_>, project: &str, ctx: Ctx<'_>) {
    if p.train {
        return;
    }
    p.train = true;
    // Each pass changes one thing that moves the order (a card told, or landing), then reads the
    // queue again; a pass changing nothing ends it.
    for _ in 0..64 {
        if !pass(p, project, ctx) {
            break;
        }
    }
    p.train = false;
}

fn pass(p: &mut Poster<'_>, project: &str, ctx: Ctx<'_>) -> bool {
    let all = cards(p, project);
    let busy = all.iter().any(active);
    for c in all {
        if c.paused && !landing(&c) {
            // Stopped, it is out of the queue until its `resume`, which verifies it again.
            if candidate(&c).is_some() || matches!(c.landing, Some(Landing::Fetch { .. })) {
                reset(p, c);
            }
            continue;
        }
        if step(p, c, busy, ctx) {
            return true;
        }
    }
    false
}

// The queue's front at work: its card landing, or its candidate made again on the moved base.
fn active(c: &Card) -> bool {
    in_queue(c) && !matches!(c.landing, Some(Landing::Passed(_)))
}

// One card, in order: whether it changed the order.
fn step(p: &mut Poster<'_>, c: Card, busy: bool, ctx: Ctx<'_>) -> bool {
    match c.landing.clone() {
        Some(Landing::Queued { at }) => {
            let (running, owed) = jobs_of(p, &c.id);
            // A run held for `/resume` holds the card, none of the others.
            let held = super::order::is_train(c.failed_job.as_ref());
            if !owed && running.is_empty() && !held {
                fetch(p, c, at);
            }
            false
        }
        Some(Landing::Passed(k)) if !busy => {
            super::landing::publish(p, c, k, ctx);
            true
        }
        _ => false,
    }
}

// Its verification starts on the base's tip, fetched first.
fn fetch(p: &mut Poster<'_>, mut c: Card, at: i64) {
    let project = c.project.clone();
    c.landing = Some(Landing::Fetch { at });
    p.put_card(c);
    p.fetch(project.as_str());
}

/// Its verification passed, its round and its suite (fac-375): our PR's candidate is published; any other card enters
/// its merge queue at the back, or keeps its place there when the queue's own run passed.
pub(crate) fn verified(p: &mut Poster<'_>, mut c: Card, k: Candidate, ctx: Ctx<'_>) {
    p.journal("merge.tested", Some(c.id.as_str()), json!({"head": k.commit, "tree": k.tree}));
    if let Some(head) = k.approved.clone() {
        return super::approval::reverified(p, c, k, head);
    }
    if crate::reviews::watch::prs(&c, ctx) {
        return super::landing::publish(p, c, k, ctx);
    }
    let (id, project, again) = (c.id.clone(), c.project.clone(), k.queue);
    let at = if again { k.at } else { p.now };
    // Its clone stays until its card's own clone goes, as it lands (`RemoveClone`).
    c.landing = Some(Landing::Passed(Candidate { at, queue: false, ..k }));
    p.put_card(c);
    if !again {
        let n = p.card(&id).map_or(0, |c| super::order::ahead(p, &c));
        let text =
            format!("{id}'s review round and suite passed: it entered the merge queue of {project}, {n} ahead of it.");
        super::members::say(p, &id, "verified: entered the merge queue", &text);
    }
}

/// A fetch of a project returned: a PR merged outside is read again, its merge commit in the mirror now (M5.3); each
/// card waiting for the tip is verified on it.
pub fn fetched(p: &mut Poster<'_>, project: &str, v: &Value, ctx: Ctx<'_>) {
    let hand: Vec<_> = p
        .state
        .cards
        .values()
        .filter(|c| c.project == project && c.landing == Some(Landing::HandMerged))
        .map(|c| c.id.clone())
        .collect();
    for card in hand {
        p.intent(Intent::ReadPr { card });
    }
    let Some(tip) = v.get("base_tip").and_then(Value::as_str).map(Sha::new) else { return };
    for c in cards(p, project) {
        if matches!(c.landing, Some(Landing::Fetch { .. }))
            && !c.paused
            && let Some(on) = super::entries::on(&c, &tip, v, ctx)
        {
            start_on(p, c, on, false);
        }
    }
    maintain(p, project, ctx);
}

/// The base moved under the queue's front: its new tip merged into its verified candidate, which runs its checks and
/// its suite again, never landed on a pass for other code (fac-10); no round runs in the queue.
pub fn again(p: &mut Poster<'_>, c: Card, tip: &Sha, why: &str, ctx: Ctx<'_>) {
    let text = format!("{why}: the merge queue merges {} in and runs its checks and its suite again.", tip.short());
    super::members::say(p, &c.id, "the base moved: run again", &text);
    let project = c.project.clone();
    start_on(p, c, tip.clone(), true);
    maintain(p, project.as_str(), ctx);
}
