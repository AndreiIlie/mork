//! A review card's rounds ended (fac-95, fac-111): one superseded by a new head, the reviewers whose work is over
//! released, our card's branch card ended with it, and a colleague's PR's card with its PR (fac-161).

use factory_types::action::{ActionStatus, ActionWhat};
use factory_types::card::{Card, Mode, Stage};
use factory_types::reply::Role;
use factory_types::review::{Proofs, Raised, Review, Round, Verdict};
use factory_types::work::{PrSeen, PrState};
use factory_types::{CardId, Sha};
use serde_json::{Value, json};

use super::gate::{agents_of, card_for};
use crate::decide::Ctx;
use crate::post::Poster;

/// Our card's head moved: its branch card's round under way reviews what it no longer lands.
pub fn moved(p: &mut Poster<'_>, subject: &Card, head: &Sha) {
    if let Some(rc) = card_for(p, subject) {
        supersede(p, rc, Some(head));
    }
}

/// Our card's submission withdrawn (fac-591): its running round and unsent failed report (copying or `held`) count for
/// nothing, its new findings gone; `afresh`, earlier rounds approve nothing more and their read isn't kept (fac-638).
pub fn withdrawn(
    p: &mut Poster<'_>,
    subject: &Card,
    held: bool,
    why: &str,
    afresh: bool,
    config: &factory_types::config::Config,
) {
    let Some(mut rc) = card_for(p, subject) else { return };
    let mut gone = Vec::new();
    if let Some(review) = rc.review.as_deref_mut() {
        let unsent = unsent(review, held);
        // A finding already given its fixer stays open: earlier rounds' reports were sent.
        let new =
            |f: &&mut Raised| !f.resolved && unsent.contains(&f.round) && f.action.is_none() && f.comment.is_none();
        for f in review.raised.iter_mut().filter(new) {
            f.resolved = true;
            f.withdrawn = true;
            gone.push(f.n);
        }
        if !unsent.is_empty() {
            rc.findings.clear();
        }
        if afresh {
            review.approved.clear();
            review.afresh = Some(review.rounds.last().map_or(1, |r| r.n + 1));
        }
    }
    let id = rc.id.clone();
    let why = format!("{why}, withdrawing its submission");
    superseded(p, rc, None, &why);
    if !gone.is_empty() {
        p.journal("review.findings.withdrawn", Some(id.as_str()), json!({"findings": gone}));
        release(p, &id, config);
    }
}

// The failed rounds whose report never went out: each waiting for its proofs' copy, its copy dropped, and the last
// one whose copy ended when its report is held for the suite.
fn unsent(review: &mut Review, held: bool) -> Vec<u32> {
    let pending = |r: &Round| matches!(r.proofs, Some(Proofs::Pending { .. }));
    let last = review.rounds.iter().rev().find(|r| r.verdict == Some(Verdict::Failed) && !pending(r)).map(|r| r.n);
    let mut unsent: Vec<u32> = held.then_some(last).flatten().into_iter().collect();
    for round in review.rounds.iter_mut().filter(|r| pending(r)) {
        round.proofs = None;
        unsent.push(round.n);
    }
    unsent
}

/// Whether an open round already reviews `head` as its commit, PR source head, or submitted head (fac-570).
pub(crate) fn reviews_head(open: &Round, head: &Sha) -> bool {
    open.commit == *head || open.head.as_ref() == Some(head) || open.submitted.as_ref() == Some(head)
}

/// If `subject`'s open review round already reviews `head`, records `head` as its submitted head and returns true (fac-570).
pub(crate) fn keep_open_submitted(p: &mut Poster<'_>, subject: &Card, head: &Sha) -> bool {
    let Some(mut rc) = card_for(p, subject) else { return false };
    let Some(open) = rc.review.as_deref_mut().and_then(Review::open_mut).filter(|o| reviews_head(o, head)) else {
        return false;
    };
    open.submitted = Some(head.clone());
    p.put_card(rc);
    true
}

/// The round under way reviews a head no longer current, or one Andrei asked again at its branch's
/// tip (none): superseded, its foreman told. The new head's round is asked once its candidate is made.
pub fn supersede(p: &mut Poster<'_>, rc: Card, head: Option<&Sha>) {
    if head.is_some_and(|h| rc.review.as_deref().and_then(Review::open).is_some_and(|o| reviews_head(o, h))) {
        return;
    }
    let of = super::gate::subject_of(&rc).cloned().unwrap_or_else(|| rc.id.clone());
    let why = head.map_or_else(
        || format!("Andrei asked {of}'s review again at its branch's tip"),
        |h| format!("{of} has a new head, {}", h.short()),
    );
    superseded(p, rc, head, &why);
}

/// Supersedes an adopted PR entry's open round unconditionally before its candidate is dropped (fac-570).
pub(super) fn adopted(p: &mut Poster<'_>, rc: Card, of: &CardId) {
    let why = format!("{of}'s verification took over this review");
    superseded(p, rc, None, &why);
}

fn superseded(p: &mut Poster<'_>, mut rc: Card, head: Option<&Sha>, why: &str) {
    let Some(review) = rc.review.as_deref_mut() else { return };
    review.next = None;
    let Some(open) = review.open_mut() else { return p.put_card(rc) };
    open.verdict = Some(Verdict::Superseded);
    let (n, asked) = (open.n, open.asked.is_some());
    rc.gates.review_asked = None;
    let id = rc.id.clone();
    p.put_card(rc);
    p.journal("review.superseded", Some(id.as_str()), json!({"n": n, "head": head}));
    // A round its foreman wasn't asked yet has no reviewers to stop.
    if !asked {
        return;
    }
    let text = format!(
        "Round {n} is superseded: {why}. A verdict on round {n} counts for nothing; stop its reviewers' work. The new head's round is asked once its candidate is made."
    );
    crate::messages::requests::tell_agent(p, &id, Role::Foreman, &format!("Round {n} superseded"), &text);
}

/// The reviewers of a review card with no finding of theirs left to verify, released: their work is
/// over. One with an open finding stays, for the round that verifies its fix.
pub(super) fn release(p: &mut Poster<'_>, id: &CardId, config: &factory_types::config::Config) {
    let Some(rc) = p.card(id) else { return };
    let keep: Vec<String> = rc
        .review
        .as_ref()
        .map(|r| {
            r.raised
                .iter()
                .filter(|f| {
                    !f.resolved
                        || f.dropped
                            .as_ref()
                            .and_then(|id| p.action(id.as_str()))
                            .is_some_and(|a| !matches!(a.status, ActionStatus::Done | ActionStatus::Dropped))
                })
                .map(|f| f.by.clone())
                .collect()
        })
        .unwrap_or_default();
    for key in agents_of(p, id) {
        let name = key.as_str().split_once('/').map_or("", |(_, n)| n);
        let ending = p.agent(&key).is_some_and(|a| a.retiring.is_some());
        if crate::agents::children::role_of(name) == Some(Role::Reviewer) && !keep.iter().any(|k| k == name) && !ending
        {
            crate::agents::closing::retire(p, &key, true, config);
            p.journal("review.released", Some(id.as_str()), json!({"agent": key}));
        }
    }
}

/// Our card landed or was closed: its branch card ends with it.
pub fn end(p: &mut Poster<'_>, subject: &Card, ctx: Ctx<'_>) {
    if super::gate::entry(subject) {
        return;
    }
    if let Some(rc) = card_for(p, subject) {
        finish(p, rc, ctx);
    }
}

/// A review card ends: its turns stopped, done, its asks closed, its agents released and its clone
/// removed.
pub fn finish(p: &mut Poster<'_>, rc: Card, ctx: Ctx<'_>) {
    let id = rc.id.clone();
    let of = super::gate::subject_of(&rc).cloned();
    crate::lifecycle::stops::stop_in(p, rc, ctx);
    let Some(mut rc) = p.card(&id) else { return };
    crate::queue::candidates::drop_candidate(p, &mut rc);
    rc.landing = None;
    rc.stage = Stage::Done;
    rc.paused = false;
    let run = rc.run;
    p.put_card(rc);
    for a in crate::messages::asks::open_of_card(p, id.as_str()) {
        crate::messages::asks::close_one(p, &a.id);
    }
    crate::agents::closing::card_ended(p, &id, true, ctx.config);
    p.act(ActionWhat::RemoveClone { card: id.clone(), run, keep_proofs: false });
    p.journal("review.ended", Some(id.as_str()), json!({"of": of}));
}

/// Reports a colleague PR's merge, including action.done, and ends its live review card (fac-161).
pub fn merged(p: &mut Poster<'_>, c: Card, v: &Value, ctx: Ctx<'_>) {
    let pr = c.pr.unwrap_or_default();
    let text = match (v.get("by_hand"), v.get("commit").and_then(Value::as_str)) {
        (Some(_), _) => format!("PR {pr} is marked merged by hand."),
        (None, Some(commit)) => format!("PR {pr} is merged: its merge commit is {}.", Sha::new(commit).short()),
        (None, None) => format!("PR {pr} is merged; the mirror doesn't hold its merge commit yet."),
    };
    if ends_with_pr(&c) {
        with_pr(p, c, &format!("{text} The review card ends."), ctx);
    } else {
        super::cards::to_andrei(p, &c.id, &text);
    }
}

/// A PR read that isn't open: its live review card ends, merged or declined (a superseded PR is
/// declined), but for a merge of the factory's under way, whose return ends it (fac-161).
pub fn closed(p: &mut Poster<'_>, c: Card, seen: &PrSeen, ctx: Ctx<'_>) {
    if !ends_with_pr(&c) || c.pr != Some(seen.pr) {
        return;
    }
    let how = match seen.state {
        PrState::Open => return,
        PrState::Merged if merging(p, &c) => return,
        PrState::Merged => "merged",
        PrState::Declined | PrState::Superseded => "declined",
    };
    with_pr(p, c, &format!("PR {} was {how} on Bitbucket: the review card ends.", seen.pr), ctx);
}

/// A live PR's review card, which ends with its PR: never our card's branch gate, which ends with it.
pub(super) fn ends_with_pr(c: &Card) -> bool {
    c.mode == Mode::Review && c.pr.is_some() && c.stage.is_live() && super::gate::subject_of(c).is_none()
}

// Whether the factory's merge of the card's PR is waiting or under way: its own, or our card's.
fn merging(p: &Poster<'_>, c: &Card) -> bool {
    let of = |card: &CardId| *card == c.id || p.card(card).is_some_and(|o| o.project == c.project);
    p.all_actions().any(|a| {
        matches!(&a.what, ActionWhat::Merge { card, pr, .. } if c.pr == Some(*pr) && of(card))
            && matches!(a.status, ActionStatus::Pending | ActionStatus::Started)
    })
}

// A review card ended with its PR, as `done` ends it, its room told why: an entry given up in its
// train, what was built on it is made again without it.
fn with_pr(p: &mut Poster<'_>, c: Card, words: &str, ctx: Ctx<'_>) {
    super::cards::to_andrei(p, &c.id, words);
    crate::lifecycle::stops::ended(p, c, ctx);
}
