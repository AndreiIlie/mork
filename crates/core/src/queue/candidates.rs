//! A card's candidates (fac-68, fac-375): each made of its card's head on the base's tip, in a clone of its own; given
//! up, its jobs stop before its clone goes. A result for a given-up candidate only removes its clone.

use mork_types::action::ActionWhat;
use mork_types::ask::AskKind;
use mork_types::card::{Candidate, Card, Landing};
use mork_types::state::Job;
use mork_types::work::{Made, Purpose};
use mork_types::{CardId, Sha};
use serde_json::json;

use super::members::{hold, say};
use super::order::{candidate, clone_of, is_train, place};
use super::train::maintain;
use crate::decide::{Ctx, Intent};
use crate::post::Poster;
use crate::state::Change;

// A candidate asked of the card's head on `on`, in a clone of its own; `queue`, the merge queue's own, made again
// on a moved base.
pub(crate) fn start(p: &mut Poster<'_>, c: Card, at: i64, on: Sha, queue: bool) {
    begin(p, c, at, on, queue, None);
}

/// Our approved PR's head, `approved`, merged with its destination's new tip and verified again (fac-375).
pub(crate) fn start_approved(p: &mut Poster<'_>, c: Card, at: i64, on: Sha, approved: Sha) {
    begin(p, c, at, on, true, Some(approved));
}

fn begin(p: &mut Poster<'_>, mut c: Card, at: i64, on: Sha, queue: bool, approved: Option<Sha>) {
    let Some(head) = approved.clone().or_else(|| c.head.clone()) else { return };
    let id = c.id.clone();
    let clone = p.now;
    let k = Candidate {
        at,
        on: on.clone(),
        clone,
        commit: None,
        tree: None,
        failure: None,
        contents: Default::default(),
        review_changes: None,
        checked: false,
        failing: Vec::new(),
        round: None,
        queue,
        approved: approved.clone(),
    };
    c.landing = Some(Landing::Prepare(k));
    // An approved head is the commit pushed last itself.
    let pushed = if approved.is_some() { None } else { c.pushed.clone() };
    p.put_card(c);
    p.journal("merge.slot", Some(id.as_str()), json!({"on": on, "queue": queue}));
    p.intent(Intent::StartJob { purpose: Purpose::Candidate { card: id, head, on, pushed, clone } });
}

/// The base's tip fetched for a card's verification, or found moved at its landing (`queue`): its candidate is made
/// on it.
pub fn start_on(p: &mut Poster<'_>, c: Card, tip: Sha, queue: bool) {
    let at = place(&c);
    start(p, c, at, tip, queue);
}

// The card's train jobs not yet ended, and whether one is owed or asked for in this decision.
pub(crate) fn jobs_of(p: &Poster<'_>, card: &CardId) -> (Vec<Job>, bool) {
    let mine = |x: &Purpose| is_train(Some(x)) && x.card() == Some(card);
    let running: Vec<Job> = p
        .state
        .jobs
        .values()
        .filter(|j| j.settled.is_none() && p.ending.as_ref() != Some(&j.id) && j.purpose.as_ref().is_some_and(mine))
        .cloned()
        .collect();
    // Owed work this decision gave up waits no longer: what was behind it starts in the same one.
    let dropped = |key: &String| p.other.iter().any(|x| matches!(x, Change::RemoveOwedWork { key: k } if k == key));
    let owed = p.state.owed_work.iter().any(|(k, w)| mine(w) && !dropped(k))
        || p.intents.iter().any(|i| matches!(i, Intent::StartJob { purpose } if mine(purpose)));
    (running, owed)
}

/// Its clone is removed, its jobs having ended (fac-68).
pub fn release(p: &mut Poster<'_>, card: &CardId, clone: i64) {
    p.act(ActionWhat::RemoveTrain { card: card.clone(), clone });
}

/// Drops the candidate and clears train holds and slot waits, including with no candidate (fac-98).
/// Its jobs stop before clone removal; the caller sets the landing and preserves any queued hold.
pub fn drop_candidate(p: &mut Poster<'_>, c: &mut Card) {
    if is_train(c.failed_job.as_ref()) {
        c.failed_job = None;
        crate::messages::asks::close(p, c.id.as_str(), &[AskKind::Error, AskKind::Cut, AskKind::Checks]);
    }
    if is_train(c.gates.slot_wait.as_ref()) {
        c.gates.slot_wait = None;
    }
    let Some(k) = candidate(c).cloned() else { return };
    // A job asked for and not yet started never starts: its clone would outlive the release.
    let mine = |x: &Purpose| is_train(Some(x)) && x.card() == Some(&c.id);
    p.intents.retain(|i| !matches!(i, Intent::StartJob { purpose } if mine(purpose)));
    let owed: Vec<String> = p.state.owed_work.iter().filter(|(_, w)| mine(w)).map(|(k, _)| k.clone()).collect();
    for key in owed {
        p.other.push(Change::RemoveOwedWork { key });
    }
    let (running, _) = jobs_of(p, &c.id);
    if running.is_empty() {
        release(p, &c.id, k.clone);
    }
    for j in running {
        crate::lifecycle::stops::stop_job(p, j);
    }
}

// Back to its place's wait, its candidate given up; a run held for `/resume` keeps its hold, and its
// red ask, in the queue, nothing made for it till its `resume` makes it again (decision 14).
pub(crate) fn reset(p: &mut Poster<'_>, mut c: Card) -> Card {
    if super::order::in_queue(&c) {
        p.journal("merge.reverify", Some(c.id.as_str()), json!({}));
    }
    let at = place(&c);
    let held = c.failed_job.take();
    drop_candidate(p, &mut c);
    c.failed_job = held;
    c.landing = Some(Landing::Queued { at });
    p.put_card(c.clone());
    c
}

/// Whether a train job is the card's current candidate's: its clone is that candidate's alone. The
/// commit it is made on may have landed meanwhile.
pub fn current(c: &Card, purpose: &Purpose) -> bool {
    match (purpose, &c.landing) {
        (Purpose::Candidate { head, clone, .. }, Some(Landing::Prepare(k))) => {
            k.clone == *clone && k.approved.as_ref().or(c.head.as_ref()) == Some(head)
        }
        (Purpose::TrainRun { commit, clone, .. }, Some(Landing::Test(k))) => {
            k.clone == *clone && k.commit.as_ref() == Some(commit)
        }
        _ => false,
    }
}

/// A train job that ended without its result for a candidate given up: its clone goes.
pub fn stale(p: &mut Poster<'_>, c: &Card, purpose: &Purpose) -> bool {
    if current(c, purpose) {
        return false;
    }
    if let Some(clone) = clone_of(purpose) {
        release(p, &c.id, clone);
    }
    true
}

fn list(paths: &[String]) -> String {
    let shown: Vec<&str> = paths.iter().take(10).map(String::as_str).collect();
    let more = paths.len().saturating_sub(shown.len());
    if more > 0 { format!("{} and {more} more", shown.join(", ")) } else { shown.join(", ") }
}

/// A candidate's conflict, naming the commits of the merge that conflicted (fac-155): the base's tip
/// into its head, or its head into the commit pushed last.
pub(crate) fn conflict(purpose: &Purpose, into: &str, on: &Sha, m: &Made) -> String {
    let (head, pushed) = if let Purpose::Candidate { head, pushed, .. } = purpose {
        (head.short(), pushed.as_ref())
    } else {
        ("?", None)
    };
    let merge = match pushed.filter(|_| m.into_pushed) {
        Some(x) => format!("merging its head {head} into the commit pushed last {}", x.short()),
        None => format!("merging {into}'s tip {} into its head {head}", on.short()),
    };
    format!("{merge} conflicts in {}", list(&m.conflicts))
}

/// A candidate made: an entry's conflict ends its run, our card's leaves for its join through its foreman; a commit
/// runs its checks.
pub fn made(p: &mut Poster<'_>, mut c: Card, purpose: &Purpose, m: Made, ctx: Ctx<'_>) {
    if stale(p, &c, purpose) {
        return;
    }
    let Some(Landing::Prepare(mut k)) = c.landing.clone() else { return };
    let project = c.project.clone();
    if !m.conflicts.is_empty() && crate::reviews::gate::entry(&c) {
        super::entries::conflicted(p, c, k, purpose, &m, ctx);
        return maintain(p, project.as_str(), ctx);
    }
    if !m.conflicts.is_empty() {
        release(p, &c.id, k.clone);
        let base = ctx.config.projects.get(&project).map_or("the base", |x| x.base.as_str());
        let why = conflict(purpose, base, &k.on, &m);
        say(p, &c.id, "left the merge queue: a conflict", &format!("{} left the merge queue: {why}.", c.id));
        super::members::drop_landing(p, &mut c);
        super::joins::through_foreman(p, c, &why, k.on, ctx);
        return maintain(p, project.as_str(), ctx);
    }
    let (Some(commit), Some(tree)) = (m.commit, m.tree) else {
        return hold(p, c, purpose.clone(), AskKind::Error, "its candidate named no commit or tree");
    };
    (k.commit, k.tree, k.contents) = (Some(commit.clone()), Some(tree), m.contents);
    k.review_changes = m.review_changes;
    // Its checks diff against what its clone was made on, which the clone holds: `k.on` may be the
    // merge commit the card ahead landed as since, the same tree, which the clone never fetched.
    let made_on = if let Purpose::Candidate { on, .. } = purpose { on.clone() } else { k.on.clone() };
    c.landing = Some(Landing::Prepare(k.clone()));
    p.put_card(c.clone());
    super::runs::run(p, c, k, made_on, ctx);
    maintain(p, project.as_str(), ctx);
}

/// A card's train jobs and its clone: what the start's sweep keeps of `trains/` (fac-68).
pub fn clones(state: &crate::state::State) -> Vec<String> {
    let of = |id: &CardId, clone: i64| format!("{id}-{clone}");
    let mut out: Vec<String> = state.cards.values().filter_map(|c| candidate(c).map(|k| of(&c.id, k.clone))).collect();
    out.extend(
        state
            .jobs
            .values()
            .filter(|j| j.settled.is_none())
            .filter_map(|j| j.purpose.as_ref())
            .filter_map(|purpose| Some(of(purpose.card()?, clone_of(purpose)?))),
    );
    out.sort();
    out.dedup();
    out
}
