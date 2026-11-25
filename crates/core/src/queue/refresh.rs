//! A live card's clone keeps origin/<base> on its project's newest base tip (fac-333): a refresh,
//! no join, so its base tip, joins, messages and stage stay as they are.

use factory_types::card::{Card, Mode, Refresh};
use factory_types::state::Settlement;
use factory_types::work::Purpose;
use factory_types::{CardId, Sha};
use serde_json::json;

use crate::decide::{Intent, JobEnd};
use crate::post::Poster;

/// The project's base tip seen (a fetch's result, or a landing): each live clone of it that hasn't
/// it gets it refreshed in, after the clone, import or refresh of it under way.
pub fn seen(p: &mut Poster<'_>, project: &str, tip: &Sha) {
    let stale: Vec<CardId> = p
        .state
        .cards
        .values()
        .filter_map(|c| p.card(&c.id))
        // No base tip yet: its clone is still to be made, at the tip fetched.
        .filter(|c| c.project == project && c.stage.is_live() && c.base_tip.is_some() && !holds(c, tip))
        .map(|c| c.id)
        .collect();
    for id in stale {
        let Some(mut c) = p.card(&id) else { continue };
        if c.refresh.as_ref().is_none_or(|r| &r.tip != tip) {
            c.refresh = Some(Refresh { tip: tip.clone(), done: false });
            p.put_card(c);
        }
        next(p, &id);
    }
}

/// Starts the clone's waiting refresh, unless its card is stopped or its clone is unprepared,
/// being made, imported or refreshed; the tip waits for `cloned`.
pub fn next(p: &mut Poster<'_>, id: &CardId) {
    let Some(c) = p.card(id) else { return };
    let Some(r) = c.refresh.clone().filter(|r| !r.done && !holds(&c, &r.tip)) else { return };
    if c.paused || !c.stage.is_live() || !prepared(&c) || cloning(p, id) || super::joins::importing(p, id) {
        return;
    }
    let purpose = Purpose::Bundle { card: id.clone(), base_in: None, recover: None, refresh: Some(r.tip) };
    p.intent(Intent::StartJob { purpose });
}

/// A clone or install made (again on a review round: it lost earlier refreshes) brings in the
/// card's latest tip; one failed, cut or stopped leaves it waiting.
pub fn cloned(p: &mut Poster<'_>, id: &CardId, how: &Settlement) {
    if *how != Settlement::Applied {
        return;
    }
    let Some(mut c) = p.card(id) else { return };
    if let Some(r) = c.refresh.as_mut().filter(|r| r.done) {
        r.done = false;
        p.put_card(c);
    }
    next(p, id);
}

/// A refresh's job ended: its tip is recorded if it came in, then a waiting import or a newer
/// refresh starts. One that didn't waits for the next tip seen, raising nothing.
pub fn ended(p: &mut Poster<'_>, id: &CardId, tip: &Sha, end: &JobEnd) -> Settlement {
    let how = match end {
        JobEnd::Finished { reply: Ok(_), .. } => Settlement::Applied,
        JobEnd::Failed { words } | JobEnd::Finished { reply: Err(words), .. } => {
            Settlement::Failed { words: words.clone() }
        }
        JobEnd::Cut { .. } => Settlement::Cut,
        JobEnd::Stopped => Settlement::Stopped,
    };
    let came_in = how == Settlement::Applied;
    // A clone made again meanwhile lost what it brought in.
    let mine = |c: &Card| c.refresh.as_ref().is_some_and(|r| &r.tip == tip);
    match p.card(id) {
        Some(mut c) if came_in && mine(&c) && !cloning(p, id) => {
            c.refresh = Some(Refresh { tip: tip.clone(), done: true });
            p.put_card(c);
        }
        _ => p.journal("refresh.ended", Some(id.as_str()), json!({"tip": tip, "how": how})),
    }
    super::joins::start(p, id, false, None);
    if came_in || p.card(id).is_some_and(|c| c.refresh.is_some() && !mine(&c)) {
        next(p, id);
    }
    how
}

// A replacement round keeps the previous clone's fork until its own preparation succeeds.
fn prepared(c: &Card) -> bool {
    c.fork.is_some()
        && !matches!(c.failed_job, Some(Purpose::Clone { .. } | Purpose::Install { .. }))
        && c.review.as_ref().and_then(|r| r.open()).is_none_or(|r| r.asked.is_some())
}

// Its clone's origin/<base> holds the tip: refreshed in, or made or joined on it (a review card's
// clone holds its branch alone).
fn holds(c: &Card, tip: &Sha) -> bool {
    c.refresh.as_ref().is_some_and(|r| r.done && &r.tip == tip)
        || (c.mode != Mode::Review && c.base_tip.as_ref() == Some(tip))
}

// A clone or install of the card asked for (this decision's too), owed or running, the job ending aside.
fn cloning(p: &Poster<'_>, id: &CardId) -> bool {
    let of = |w: &Purpose| matches!(w, Purpose::Clone { card, .. } | Purpose::Install { card, .. } if card == id);
    let running = |j: &factory_types::state::Job| {
        p.ending.as_ref() != Some(&j.id) && j.settled.is_none() && j.purpose.as_ref().is_some_and(of)
    };
    p.intents.iter().any(|i| matches!(i, Intent::StartJob { purpose } if of(purpose)))
        || p.state.owed_work.values().any(of)
        || p.state.jobs.values().any(running)
}
