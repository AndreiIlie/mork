//! A submission its reopened mock (fac-591) or its return to brief (fac-638) withdrew: its bundles, takes, checks,
//! push and PR opening retired by identity: none moves the head, passes or fails a later submission, asks or reruns.

use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::card::Card;
use mork_types::state::Settlement;
use mork_types::work::Purpose;
use mork_types::{CardId, JobId, Sha};
use serde_json::json;

use crate::decide::{Ctx, Intent, JobEnd};
use crate::messages::asks;
use crate::post::Poster;
use crate::state::Change;

const WHY: &str = "its submission was withdrawn: the card's mock reopened, or it went back to brief";

/// Retires the card's submission: its bundles and checks running and its takes under way listed, its checks stopped,
/// its takes not yet started dropped, and its bundle and checks owed but unrecorded given up. The clone is left as is.
pub fn retire(p: &mut Poster<'_>, id: &CardId, ctx: Ctx<'_>) {
    let Some(c) = p.card(id) else { return };
    let mut listed: Vec<JobId> = c.retired.iter().filter(|j| live(p, id, j)).cloned().collect();
    let running: Vec<_> = (p.state.jobs.values())
        .filter(|j| j.settled.is_none() && j.purpose.as_ref().is_some_and(|w| submission(w, id, true)))
        .cloned()
        .collect();
    for mut j in running {
        listed.push(j.id.clone());
        if matches!(j.purpose, Some(Purpose::Check { .. })) && !j.stop_requested {
            j.stop_requested = true;
            p.intent(Intent::StopJob { id: j.id.clone() });
            p.other.push(Change::PutJob(Box::new(j)));
        }
    }
    listed.extend(takes(p, id));
    publishing(p, id);
    let removed =
        |p: &Poster<'_>, k: &String| p.other.iter().any(|x| matches!(x, Change::RemoveOwedWork { key } if key == k));
    let owed: Vec<Purpose> = (p.state.owed_work.iter())
        .filter(|(k, w)| submission(w, id, false) && !removed(p, k))
        .map(|(_, w)| w.clone())
        .collect();
    for w in owed {
        p.other.push(Change::RemoveOwedWork { key: w.key() });
        crate::queue::leases::job_done(p, None, &w, ctx);
    }
    listed.sort();
    listed.dedup();
    let Some(mut c) = p.card(id) else { return };
    c.retired = listed;
    if c.failed_job.as_ref().is_some_and(|w| submission(w, id, false)) {
        c.failed_job = None;
    }
    p.journal("submission.retired", Some(id.as_str()), json!({"jobs": c.retired}));
    p.put_card(c);
}

// The card's takes not yet returned: one under way is listed by its bundle, the rest dropped.
fn takes(p: &mut Poster<'_>, id: &CardId) -> Vec<JobId> {
    let takes: Vec<Action> = (p.all_actions())
        .filter(|a| matches!(&a.what, ActionWhat::Take { card, .. } if card == id))
        .filter(|a| matches!(a.status, ActionStatus::Pending | ActionStatus::Started | ActionStatus::Held))
        .cloned()
        .collect();
    let mut under_way = Vec::new();
    for a in takes {
        match (&a.what, a.status) {
            (ActionWhat::Take { job, .. }, ActionStatus::Started) => under_way.push(job.clone()),
            _ => dropped(p, a),
        }
    }
    under_way
}

// The card's push and PR opening under way are marked: their success is recorded, their failure goes unanswered.
fn publishing(p: &mut Poster<'_>, id: &CardId) {
    let ours = |a: &&Action| {
        matches!(&a.what, ActionWhat::Push { card, .. } | ActionWhat::OpenPr { card, .. } if card == id)
            && a.status == ActionStatus::Started
            && !a.withdrawn
    };
    let under_way: Vec<Action> = p.all_actions().filter(ours).cloned().collect();
    for mut a in under_way {
        a.withdrawn = true;
        p.put_action(a);
    }
}

// A bundle reading the card's clone for a head, or a check of one; with `imports`, a base's import's bundle too.
fn submission(w: &Purpose, id: &CardId, imports: bool) -> bool {
    let bundle = matches!(w, Purpose::Bundle { card, base_in, refresh: None, .. } if card == id && (imports || base_in.is_none()));
    bundle || matches!(w, Purpose::Check { card, .. } if card == id)
}

// Still to settle or return: a job not settled, or a take of it under way.
fn live(p: &Poster<'_>, id: &CardId, job: &JobId) -> bool {
    p.state.jobs.get(job).is_some_and(|j| j.settled.is_none())
        || p.all_actions().any(|a| {
            matches!(&a.what, ActionWhat::Take { card, job: j, .. } if card == id && j == job)
                && !matches!(a.status, ActionStatus::Done | ActionStatus::Dropped)
        })
}

/// A retired bundle's or check's end, settled with nothing routed: no head taken, no check counted, no ask, no run
/// again. None for a job not retired, or a base's import, whose join goes on.
pub(crate) fn ended(p: &mut Poster<'_>, job: &JobId, purpose: &Purpose, end: &JobEnd) -> Option<Settlement> {
    let card = purpose.card().cloned()?;
    if !p.card(&card).is_some_and(|c| c.retired.contains(job)) || !submission(purpose, &card, false) {
        return None;
    }
    p.journal("job.retired", Some(card.as_str()), json!({"job": job}));
    Some(match end {
        JobEnd::Finished { reply: Ok(_), .. } => {
            // A base's import waiting for the clone starts once this read of it ended, as any bundle's end has it.
            if matches!(purpose, Purpose::Bundle { .. }) {
                crate::queue::joins::bundle_ran(p, &card, None);
            }
            Settlement::Applied
        }
        JobEnd::Finished { reply: Err(words), .. } | JobEnd::Failed { words } => {
            Settlement::Failed { words: words.clone() }
        }
        JobEnd::Cut { .. } => Settlement::Cut,
        JobEnd::Stopped => Settlement::Stopped,
    })
}

/// A settled job leaves the card's retired list.
pub(crate) fn settled(p: &mut Poster<'_>, purpose: &Purpose, job: &JobId) {
    forget(p, purpose.card(), job);
}

/// A retired bundle's take returned: its head isn't taken.
pub(crate) fn take_returned(p: &mut Poster<'_>, card: &Card, head: &Sha, job: &JobId) {
    p.journal("take.retired", Some(card.id.as_str()), json!({"head": head, "job": job}));
    forget(p, Some(&card.id), job);
}

/// Whether the action is a withdrawn submission's: a retired bundle's take, or a push or PR opening under way at the
/// withdrawal. Its failure, or a recovery finding it undone, drops it: never held, sent back or run again.
pub(crate) fn withdrawn(p: &Poster<'_>, a: &Action) -> bool {
    let take =
        matches!(&a.what, ActionWhat::Take { card, job, .. } if p.card(card).is_some_and(|c| c.retired.contains(job)));
    take || a.withdrawn
}

/// Drops a withdrawn submission's action so a failure or recovery can't run it again.
pub(crate) fn drop_action(p: &mut Poster<'_>, a: Action) {
    let take = if let ActionWhat::Take { card, job, .. } = &a.what { Some((card.clone(), job.clone())) } else { None };
    dropped(p, a);
    if let Some((card, job)) = take {
        forget(p, Some(&card), &job);
    }
}

fn dropped(p: &mut Poster<'_>, mut a: Action) {
    let id = a.id.clone();
    a.status = ActionStatus::Dropped;
    a.words = Some(WHY.to_owned());
    a.done_ms = Some(p.now);
    p.journal("action.dropped", a.what.card().map(CardId::as_str), json!({"id": id, "words": WHY}));
    p.put_action(a);
    let tied: Vec<_> =
        p.all_asks().filter(|x| x.open && x.action.as_ref() == Some(&id)).map(|x| x.id.clone()).collect();
    for x in tied {
        asks::close_one(p, x.as_str());
    }
}

fn forget(p: &mut Poster<'_>, card: Option<&CardId>, job: &JobId) {
    let Some(mut c) = card.and_then(|c| p.card(c)).filter(|c| c.retired.contains(job)) else { return };
    c.retired.retain(|j| j != job);
    p.put_card(c);
}
