//! Joins (4.12): the foreman merges the base's tip into its branch, asked by the engine, never
//! Andrei, once the tip is in its clone; early joins when an overlapping card lands.

use factory_types::ask::AskKind;
use factory_types::card::{Card, FixImport, Join, Stage};
use factory_types::reply::Role;
use factory_types::state::Job;
use factory_types::work::Purpose;
use factory_types::{CardId, Sha};
use serde_json::json;

use crate::decide::{Ctx, Intent};
use crate::messages::requests::{ask_agent, tell_agent};
use crate::post::Poster;
use crate::state::Change;

/// Identifies a submission's join, whose base-side paths haven't been read.
pub(crate) const BEFORE_CHECKS: &str =
    "The base moved past this head's fork, so its latest tip must be joined before the checks";

pub(crate) fn base(card: &Card, ctx: Ctx<'_>) -> String {
    ctx.config.projects.get(&card.project).map_or("master".into(), |p| p.base.clone())
}

/// Sends a missing base tip or a merge conflict through the foreman as a join.
/// Records it once and brings the tip into the clone before requesting the merge (4.12.1).
pub fn through_foreman(p: &mut Poster<'_>, mut c: Card, why: &str, tip: Sha, ctx: Ctx<'_>) {
    let b = base(&c, ctx);
    let asked_of = c.head.clone();
    // The same tip asked of the same head is that join, its import pending or its request sent (fac-395).
    if !c.join.as_ref().is_some_and(|j| j.tip == tip && j.asked_of == asked_of) {
        c.join = Some(Join { why: why.to_owned(), base: b, tip: tip.clone(), asked_of, waiting: true });
    }
    c.stage = Stage::Build;
    c.gates.passed = None;
    let (id, paused) = (c.id.clone(), c.paused);
    p.put_card(c);
    // Back to build: nothing waits on its merge until the joined head is ready (R20, fac-12).
    crate::messages::asks::close(p, id.as_str(), &[AskKind::ApprovePr]);
    p.journal("join", Some(id.as_str()), json!({"why": why, "tip": tip}));
    if !paused {
        start(p, &id, false, None);
    }
}

/// An import: a bundle bringing the base's tip into a card's clone as origin/<base> (fac-97).
pub fn is_import(w: &Purpose) -> bool {
    matches!(w, Purpose::Bundle { base_in: Some(_), .. })
}

/// Starts one import of the join's current tip; imports of a clone never overlap (fac-97).
/// An import run `again` survives an already-sent join request.
pub fn start(p: &mut Poster<'_>, id: &CardId, again: bool, recover: Option<Sha>) {
    let Some(c) = p.card(id) else { return };
    // A waiting join's tip first, then a held fix's (fac-144), then an asked join's run again.
    let waiting = c.join.as_ref().filter(|j| j.waiting).map(|j| j.tip.clone());
    let fix = c.fix_import.as_ref().map(|f| f.tip.clone());
    let asked = c.join.as_ref().filter(|_| again).map(|j| j.tip.clone());
    let Some(tip) = waiting.or(fix).or(asked) else {
        if again {
            p.journal("join.import.obsolete", Some(id.as_str()), json!({}));
        }
        return;
    };
    if c.paused || !c.stage.is_live() {
        return;
    }
    if importing(p, id) {
        p.journal("join.import.waits", Some(id.as_str()), json!({"tip": tip}));
        return;
    }
    let purpose = Purpose::Bundle { card: id.clone(), base_in: Some(tip), recover, refresh: None };
    // Owed again if this decision dropped it, as the engine's start drops an older run's imports.
    if dropped(p, &purpose.key()) {
        p.other.push(Change::PutOwedWork(purpose.clone()));
    }
    p.intent(Intent::StartJob { purpose });
}

/// Requests a waiting join after its tip was imported with no other import in flight (fac-97).
/// A different bundle instead schedules the current tip's import.
pub fn bundle_ran(p: &mut Poster<'_>, id: &CardId, base_in: Option<&Sha>) {
    let Some(mut c) = p.card(id) else { return };
    let join = c.join.clone().filter(|j| j.waiting);
    if (join.is_none() && c.fix_import.is_none()) || c.paused || !c.stage.is_live() || importing(p, id) {
        return;
    }
    let mut joined = None;
    if let Some(j) = join.filter(|j| base_in == Some(&j.tip)) {
        c.join = Some(Join { waiting: false, ..j.clone() });
        p.put_card(c.clone());
        request(p, &c, &j);
        joined = Some(j.tip);
    }
    // A held fix goes once its tip is in, or a join's just asked, never older (fac-144).
    if let Some(f) = c.fix_import.clone()
        && let Some(tip) = joined.or_else(|| base_in.filter(|t| **t == f.tip).cloned())
    {
        fix_request(p, c, &format!("{b}'s tip {tip} is in its clone as origin/{b}.", b = f.base));
    }
    // Imports never overlap: a join or fix this bundle wasn't for starts its own only now (fac-97).
    if p.card(id).is_some_and(|c| c.join.as_ref().is_some_and(|j| j.waiting) || c.fix_import.is_some()) {
        start(p, id, false, None);
    }
}

/// A train failure's request to the foreman, held until the commit its candidate was merged onto
/// is in the card's clone as origin/<base> (fac-144).
pub fn hold_fix(p: &mut Poster<'_>, id: &CardId, fix: FixImport) {
    let Some(mut c) = p.card(id) else { return };
    c.fix_import = Some(fix);
    p.put_card(c);
    start(p, id, false, None);
}

/// An import that failed for good: a held fix request goes anyway, saying so (fac-144).
pub fn import_failed(p: &mut Poster<'_>, id: &CardId, words: &str) {
    let Some(c) = p.card(id).filter(|c| c.fix_import.is_some() && c.stage.is_live()) else { return };
    let Some(f) = c.fix_import.clone() else { return };
    let why = words.lines().next().unwrap_or("");
    let said = format!("{b}'s tip {} couldn't be brought into its clone as origin/{b}: {why}", f.tip, b = f.base);
    fix_request(p, c, &said);
}

fn fix_request(p: &mut Poster<'_>, mut c: Card, said: &str) {
    let Some(f) = c.fix_import.take() else { return };
    p.put_card(c.clone());
    let text = format!("{}\n\n{said}", f.text);
    ask_agent(p, &c.id, Role::Foreman, &f.headline, &text);
}

/// Reimports after a stopped import settles, unless its card is paused (fac-97).
pub fn import_ended(p: &mut Poster<'_>, id: &CardId, stopped: bool) {
    if stopped {
        start(p, id, true, None);
    }
}

/// Recovers owed imports on the current join tip, retaining recovery fork points (fac-97).
/// Paused cards keep imports owed until resume; each clone imports one at a time.
pub fn recover(p: &mut Poster<'_>) {
    let mut cards: Vec<CardId> =
        p.state.owed_work.values().filter(|w| is_import(w)).filter_map(Purpose::card).cloned().collect();
    cards.sort();
    cards.dedup();
    for id in cards {
        if p.card(&id).is_some_and(|c| c.paused && c.stage.is_live()) {
            continue;
        }
        let (_, recover) = drop_owed(p, &id);
        start(p, &id, true, recover);
    }
}

/// `resume`: a stopped card's import an older run left owed runs again, its join asked or not, as at
/// the engine's start; else the join's tip is brought in if it waits for it (fac-97).
pub fn resumed(p: &mut Poster<'_>, id: &CardId) {
    let stopped = p.state.cards.get(id).is_some_and(|c| c.paused);
    let (again, recover) = if stopped { drop_owed(p, id) } else { (false, None) };
    start(p, id, again, recover);
}

/// A closing card gives up its owed imports so its final bundle can finish removing its clone.
pub fn closed(p: &mut Poster<'_>, id: &CardId) {
    let _ = drop_owed(p, id);
    if let Some(mut c) = p.card(id).filter(|c| c.fix_import.is_some()) {
        c.fix_import = None;
        p.put_card(c);
    }
}

// Drops the card's owed imports: whether there were any, and a recovery's fork point.
fn drop_owed(p: &mut Poster<'_>, id: &CardId) -> (bool, Option<Sha>) {
    let owed: Vec<(String, Purpose)> = p
        .state
        .owed_work
        .iter()
        .filter(|(k, w)| is_import(w) && w.card() == Some(id) && !dropped(p, k))
        .map(|(k, w)| (k.clone(), w.clone()))
        .collect();
    let (mut any, mut fork) = (false, None);
    for (key, w) in owed {
        p.other.push(Change::RemoveOwedWork { key });
        let Purpose::Bundle { base_in, recover, .. } = w else { continue };
        p.journal("join.import.dropped", Some(id.as_str()), json!({"tip": base_in}));
        (any, fork) = (true, fork.or(recover));
    }
    (any, fork)
}

// An import or refresh of the card's clone asked for (this decision's too), owed or running, the
// job ending aside: one at a time (fac-97, fac-333).
pub(crate) fn importing(p: &Poster<'_>, id: &CardId) -> bool {
    let of = |w: &Purpose| matches!(w, Purpose::Bundle { refresh: Some(_), .. }) || is_import(w);
    let of = |w: &Purpose| of(w) && w.card() == Some(id);
    let running =
        |j: &Job| p.ending.as_ref() != Some(&j.id) && j.settled.is_none() && j.purpose.as_ref().is_some_and(of);
    p.intents.iter().any(|i| matches!(i, Intent::StartJob { purpose } if of(purpose)))
        || p.state.owed_work.iter().any(|(k, w)| of(w) && !dropped(p, k))
        || p.state.jobs.values().any(running)
}

// Owed work this decision drops.
fn dropped(p: &Poster<'_>, key: &str) -> bool {
    p.other.iter().any(|c| matches!(c, Change::RemoveOwedWork { key: k } if k == key))
}

fn request(p: &mut Poster<'_>, c: &Card, j: &Join) {
    let (b, tip) = (&j.base, &j.tip);
    let text = format!(
        "{}. Join the base now: {b}'s tip {tip} is in its clone as origin/{b}, already fetched: the clone has no remote to fetch from. Once it is joined and its checks pass, it is verified again and enters the merge queue at its back. The `joins-and-the-train` skill has how.",
        j.why.trim_end_matches('.')
    );
    ask_agent(p, &c.id, Role::Foreman, "Join the base", &text);
}

/// Schedules imports for overlapping live peers after a landing, deferring join requests until import.
/// Brief cards receive an update; stopped cards wait for resume, and ready cards return to build.
pub fn early(p: &mut Poster<'_>, landed: &Card, tip: Option<Sha>, ctx: Ctx<'_>) {
    let Some(tip) = tip else { return };
    let others: Vec<Card> = p
        .state
        .cards
        .values()
        .filter_map(|c| p.card(&c.id))
        .filter(|c| c.id != landed.id && c.project == landed.project && c.stage.is_live())
        .collect();
    for mut c in others {
        let shared: Vec<String> = c.changed.iter().filter(|f| landed.changed.contains(f)).cloned().collect();
        let on_tip = c.fork.as_ref() == Some(&tip) || c.base_tip.as_ref() == Some(&tip);
        // A card in the merge queue: the runner incorporates the base when its turn comes (fac-10).
        let queued = c.stage == Stage::Merging && c.landing.is_some();
        if shared.is_empty() || on_tip || queued || c.join.as_ref().is_some_and(|j| j.tip == tip) {
            continue;
        }
        let id = c.id.clone();
        let words = format!(
            "{} merged into {}, and it changes files yours does: {}.",
            landed.id,
            base(&c, ctx),
            shared.join(", ")
        );
        if c.stage == Stage::Brief {
            let words = format!("{words} A join will be needed once the card builds.");
            tell_agent(p, &id, Role::Foreman, "A peer merged", &words);
            continue;
        }
        let b = base(&c, ctx);
        c.join = Some(Join { why: words.clone(), base: b, tip: tip.clone(), asked_of: c.head.clone(), waiting: true });
        let was_ready = c.stage == Stage::Ready;
        if was_ready {
            c.stage = Stage::Build;
            c.gates.passed = None;
        }
        let paused = c.paused;
        p.put_card(c);
        if was_ready {
            // Back to build: nothing waits on its merge now (R20).
            crate::messages::asks::close(p, id.as_str(), &[AskKind::ApprovePr]);
        }
        if !paused {
            start(p, &id, false, None);
        }
    }
}
