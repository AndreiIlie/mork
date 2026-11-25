//! Our PRs wait outside the merge queue for Andrei's current-head approval, asked once verified; PRs merge only on it,
//! with his credential (fac-98, fac-111, fac-375). Conflicts never retry: ours joins, others stay held for him.

use factory_types::action::{Action, ActionStatus, ActionWhat};
use factory_types::ask::{AskKind, Color};
use factory_types::card::{Candidate, Card, Landing, Mode, Stage};
use factory_types::work::{ActionFailure, OpenPr, PrSeen, PrState};
use factory_types::{CardId, ProjectId, Sha};
use serde_json::json;

use super::pr::repo_of;
use crate::decide::Ctx;
use crate::decide::actions::DROPPED_BY_ANDREI;
use crate::messages::asks::{self, Raise};
use crate::post::Poster;
use crate::state::State;

/// The branch a PR goes into when the poller read none.
pub const MASTER: &str = "master";

/// What a merge dropped for its card's join says: the PR's review is renewed once it is published again.
pub const LEFT_FOR_JOIN: &str = "the PR conflicts with master: left for its join";

/// The PRs a live card of ours holds, by project: the poller reads them as that card's, never as a
/// colleague's, whatever the card is doing.
pub fn owned(state: &State) -> Vec<(ProjectId, u64)> {
    let ours = state.cards.values().filter(|c| c.mode != Mode::Review && c.stage.is_live());
    ours.filter_map(|c| c.pr.map(|pr| (c.project.clone(), pr))).collect()
}

/// Our live card holding a project's PR.
pub fn owner<'a>(state: &'a State, project: &str, pr: u64) -> Option<&'a Card> {
    state.cards.values().find(|c| holds(c, project, pr))
}

/// Whether a card is our live one holding a project's PR.
pub fn holds(c: &Card, project: &str, pr: u64) -> bool {
    c.project == project && c.mode != Mode::Review && c.stage.is_live() && c.pr == Some(pr)
}

/// Merges our published PR on Andrei's current-head approval once that head was verified on the base it merges into:
/// before publishing (fac-375), or by an older build's PR entry; waits for its card's turns to end (fac-111).
pub fn own(p: &mut Poster<'_>, c: &Card, seen: &PrSeen, ctx: Ctx<'_>) {
    let Some(Landing::Approval { tree, base, commit, verified }) = c.landing.clone() else { return };
    let (Some(head), Some(cfg)) = (c.pushed.clone(), ctx.config.projects.get(&c.project)) else { return };
    if c.paused || !seen.approved || seen.dest.as_deref() != Some(cfg.base.as_str()) {
        return;
    }
    let base = if verified {
        (commit.as_ref() == Some(&head)).then_some(base)
    } else {
        ran(p, &crate::reviews::cards::card_id(c.project.as_str(), seen.pr), &head)
    };
    let Some(base) = base else { return };
    let (repo, dest) = (repo_of(c, ctx.config), Some(cfg.base.clone()));
    let what = ActionWhat::Merge { card: c.id.clone(), pr: seen.pr, head, tree, base, repo, dest };
    if merge(p, what) {
        let words = format!("You approved {}'s PR {}: it merges into {}.", c.id, seen.pr, cfg.base);
        super::members::say(p, &c.id, "its PR is approved", &words);
    }
}

/// Merges an unpaused PR entry on current-head approval after its run passed on that base; live
/// cards own their PRs. Findings remain Andrei's to weigh (fac-98, fac-111).
pub fn colleague(p: &mut Poster<'_>, c: &Card, seen: &PrSeen, ctx: Ctx<'_>) {
    let Some(cfg) = ctx.config.projects.get(&c.project) else { return };
    let asked = c.review.as_ref().is_some_and(|r| r.merge);
    let Some(into) = seen.dest.clone().filter(|d| asked || crate::reviews::watch::watched(cfg, d)) else { return };
    if c.paused || c.pr != Some(seen.pr) || seen.state != PrState::Open || !seen.approved {
        return;
    }
    if owner(p.state, c.project.as_str(), seen.pr).is_some() {
        return;
    }
    let (head, repo) = (seen.head.clone(), repo_of(c, ctx.config));
    let Some(base) = ran(p, &c.id, &head) else { return };
    let what = ActionWhat::Merge { card: c.id.clone(), pr: seen.pr, head, tree: None, base, repo, dest: Some(into) };
    if merge(p, what) {
        let words = format!("You approved PR {} on its head {}: it merges.", seen.pr, seen.head.short());
        crate::reviews::cards::to_andrei(p, &c.id, &words);
    }
}

// The base an approved head's run merged in, its entry out of the train; a run that conflicted or failed merges
// nothing, told once.
fn ran(p: &mut Poster<'_>, id: &CardId, head: &Sha) -> Option<Sha> {
    let mut e = p.card(id).filter(|e| crate::reviews::gate::entry(e) && e.landing.is_none())?;
    let f = e.review.as_ref()?.finished.clone()?;
    if f.head != *head {
        return None;
    }
    if !f.failed {
        return Some(f.on);
    }
    let r = e.review.as_deref_mut()?;
    if r.approval.as_ref() != Some(head) {
        r.approval = Some(head.clone());
        p.put_card(e.clone());
        let pr = e.pr.unwrap_or_default();
        let text = format!(
            "You approved PR {pr} on its head {}, but its run there conflicted or failed: it doesn't merge. A new head goes through the merge train again.",
            head.short()
        );
        crate::reviews::cards::to_andrei(p, id, &text);
    }
    None
}

/// An approved merge found its destination moved since its run (fac-111): our verified PR is verified again on the
/// new tip (fac-375), any other's entry runs again on it with its review; either merges after while still approved.
pub fn moved(p: &mut Poster<'_>, a: Action, words: &str, tip: &Sha, ctx: Ctx<'_>) {
    let ActionWhat::Merge { card, pr, head, .. } = a.what.clone() else {
        return crate::decide::actions::hold(p, a, words);
    };
    let ours = p.card(&card).filter(|c| matches!(c.landing, Some(Landing::Approval { verified: true, .. })));
    if let Some(c) = ours.filter(|c| c.pushed.as_ref() == Some(&head)) {
        dropped(p, a, &format!("{words}: its verification starts again"));
        return reverify(p, c, words, tip, head, ctx);
    }
    let project = p.card(&card).map(|c| c.project.clone());
    let entry = project.and_then(|x| p.card(&crate::reviews::cards::card_id(x.as_str(), pr)));
    let again = |e: &Card| e.stage.is_live() || e.stage == Stage::Done;
    let Some(mut e) = entry.filter(|e| crate::reviews::gate::entry(e) && again(e)) else {
        return crate::decide::actions::hold(p, a, words);
    };
    // A done one reopens as its next run, out of reach of its last run's clone removal. The run its
    // merge was refused on merges nothing more: the next one on the new tip does (fac-134).
    if e.stage == Stage::Done {
        e.run += 1;
        e.first_seq = p.state.next_seq;
        e.delegated |= crate::messages::auto::on(p.state, &e.project);
    }
    if let Some(r) = e.review.as_deref_mut() {
        r.finished = None;
    }
    dropped(p, a, &format!("{words}: it goes through the merge train again"));
    let text = format!(
        "Your approved PR {pr} doesn't merge yet: {words}. It goes through the merge train again on {}, and merges after while your approval stands.",
        tip.short()
    );
    crate::reviews::cards::to_andrei(p, &e.id, &text);
    super::entries::enter(p, e, tip.clone(), head, ctx);
}

// A merge given up for a run on the moved destination, which acts it again once passed.
fn dropped(p: &mut Poster<'_>, mut a: Action, why: &str) {
    let card = a.what.card().cloned();
    a.status = ActionStatus::Dropped;
    a.words = Some(why.to_owned());
    a.done_ms = Some(p.now);
    p.journal("action.dropped", card.as_ref().map(CardId::as_str), json!({"id": a.id, "words": why}));
    p.put_action(a);
}

// Our approved head, verified before it was published, merged with the destination's new tip and verified again: its
// checks and suite, a round only if the merge changed files its rounds approved; nothing is pushed (fac-375).
fn reverify(p: &mut Poster<'_>, mut c: Card, words: &str, tip: &Sha, head: Sha, ctx: Ctx<'_>) {
    let text = format!(
        "Your approved PR {} doesn't merge yet: {words}. Verification of its head {} started again on its destination's tip {}; it merges once that passes, while your approval stands.",
        c.pr.unwrap_or_default(),
        head.short(),
        tip.short()
    );
    super::members::say(p, &c.id, "its PR's destination moved: verification started again", &text);
    let (at, project) = (p.now, c.project.clone());
    c.landing = None;
    super::candidates::start_approved(p, c, at, tip.clone(), head);
    super::train::maintain(p, project.as_str(), ctx);
}

// The merge of an approved head, acted once across polls and restarts: whether it is new. One held,
// or dropped by a conflict or by Andrei, stands; any other dropped one is acted again.
fn merge(p: &mut Poster<'_>, what: ActionWhat) -> bool {
    let key = what.key();
    let same = |a: &Action| a.key == key || a.key.starts_with(&format!("{key}@"));
    let by_andrei = |a: &Action| a.words.as_deref() == Some(DROPPED_BY_ANDREI);
    let known = p.all_actions().any(|a| same(a) && (a.status != ActionStatus::Dropped || left(a) || by_andrei(a)))
        || p.state.unloaded.alike(&key, |s| s != ActionStatus::Dropped).is_some();
    if known {
        return false;
    }
    let card = what.card().cloned();
    let pr = if let ActionWhat::Merge { pr, .. } = &what { *pr } else { 0 };
    p.act(what);
    p.journal("pr.approved", card.as_ref().map(CardId::as_str), json!({"pr": pr}));
    true
}

fn left(a: &Action) -> bool {
    a.status == ActionStatus::Dropped && a.words.as_deref() == Some(LEFT_FOR_JOIN)
}

/// Comments once on a merge conflict, then requests our waiting card's join or holds the merge; no
/// refused merge retries automatically (fac-98).
pub fn conflicted(p: &mut Poster<'_>, mut a: Action, f: &ActionFailure, ctx: Ctx<'_>) {
    let ActionFailure::Conflict { words, tip, files } = f else { return };
    let ActionWhat::Merge { card, pr, head, repo, dest, .. } = a.what.clone() else { return };
    let into = dest.unwrap_or_else(|| MASTER.to_owned());
    let body = format!("Cannot merge this PR: it conflicts with {into} (at its head {}).", head.short());
    p.act(ActionWhat::Comment { card: card.clone(), pr, file: String::new(), line: 0, body, repo });
    let pushed = |c: &Card| c.pushed.as_ref() == Some(&head);
    let (Some(c), Some(tip)) = (p.card(&card).filter(|c| waits(c) && pushed(c)), tip.clone()) else {
        return crate::decide::actions::hold(p, a, words);
    };
    a.status = ActionStatus::Dropped;
    a.words = Some(LEFT_FOR_JOIN.to_owned());
    a.done_ms = Some(p.now);
    p.journal("action.dropped", Some(card.as_str()), json!({"id": a.id, "words": LEFT_FOR_JOIN}));
    p.put_action(a);
    let shown: Vec<&str> = files.iter().take(10).map(String::as_str).collect();
    let mut why = format!("Bitbucket can't merge PR {pr}: it conflicts with {into}'s tip {}", tip.short());
    if !shown.is_empty() {
        why.push_str(&format!(" in {}", shown.join(", ")));
    }
    join(p, c, &why, tip, ctx);
}

// Our card's PR waiting for Andrei's approval.
fn waits(c: &Card) -> bool {
    c.mode != Mode::Review && waiting(c)
}

/// Our card's PR waiting for approval conflicts with master's `tip`, as its merge or its entry's
/// candidate found (fac-111): the card goes to its join, its joined head updating the same PR.
pub fn join(p: &mut Poster<'_>, mut c: Card, why: &str, tip: Sha, ctx: Ctx<'_>) {
    if !waits(&c) {
        return;
    }
    let card = c.id.clone();
    let text = format!(
        "{card} goes to its join: {why}. Its joined head updates the same PR, and you are told once its review of the new head finds nothing. The review starts from the changed-file list and traces fixes and merged base changes into affected callers and dependents."
    );
    super::members::say(p, &card, "its PR conflicts with master", &text);
    super::members::drop_landing(p, &mut c);
    let project = c.project.clone();
    super::joins::through_foreman(p, c, why, tip, ctx);
    super::train::maintain(p, project.as_str(), ctx);
}

/// Leaves a published PR waiting for Andrei's approval, with its link: verified before it was published, it says so in
/// one comment and asks his approval at once (fac-375); an older build's waits for its PR entry's review (fac-98).
pub fn wait(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    let (Some(Landing::Published { tree, base, commit, verified }), Some(pr)) = (c.landing.clone(), c.pr) else {
        return;
    };
    c.landing = Some(Landing::Approval { tree, base, commit: commit.clone(), verified });
    let (id, project) = (c.id.clone(), c.project.clone());
    p.put_card(c.clone());
    let link = link(&c, pr, ctx);
    p.journal("pr.waiting", Some(id.as_str()), json!({"pr": pr}));
    if verified {
        let words = format!(
            "{id}'s PR {pr} is open in your name: {link}. Its review round and suite passed on this head; nothing merges it before your approval."
        );
        super::members::say(p, &id, "its PR is open and awaiting approval", &words);
        let repo = repo_of(&c, ctx.config);
        let body = crate::reviews::results::NO_FINDINGS.to_owned();
        p.act_as(ActionWhat::Comment { card: id, pr, file: String::new(), line: 0, body, repo }, true);
        notify_approval(p, &c, commit.as_ref(), ctx);
    } else {
        let words = format!(
            "{id}'s PR {pr} is open in your name: {link}. Its review runs first: you are told once the review of this head finds nothing; nothing merges it before your approval."
        );
        super::members::say(p, &id, "its PR is open and awaiting review", &words);
        if let Some(review) = p.card(&crate::reviews::cards::card_id(c.project.as_str(), pr)) {
            notify_reviewed(p, &review, ctx);
        }
    }
    super::train::maintain(p, &project, ctx);
}

/// Our approved PR's head verified again on its destination's new tip (fac-375): it waits for the next read of its
/// approval, which merges it on that tip.
pub fn reverified(p: &mut Poster<'_>, mut c: Card, k: Candidate, head: Sha) {
    let text = format!(
        "Its approved head {} passed again on {}: it merges while your approval stands.",
        head.short(),
        k.on.short()
    );
    c.landing = Some(Landing::Approval { tree: k.tree, base: k.on, commit: Some(head), verified: true });
    let id = c.id.clone();
    p.put_card(c);
    super::members::say(p, &id, "verified again: its PR merges on approval", &text);
}

/// Asks Andrei's approval once the review of our published head is clean; a verdict on another head sends no
/// notice, as approval merges that head (fac-132).
pub fn notify_reviewed(p: &mut Poster<'_>, review: &Card, ctx: Ctx<'_>) {
    let Some(pr) = review.pr else { return };
    let Some(done) = review.review.as_ref().and_then(|r| r.finished.as_ref()).filter(|f| f.clean && !f.failed) else {
        return;
    };
    if review.head.as_ref() != Some(&done.head) {
        return;
    }
    // The owner as this decision has it: its publication may be recorded in this poster alone.
    let Some(c) = p.card_where(|c| holds(c, &review.project, pr)).filter(waiting) else {
        return;
    };
    notify_approval(p, &c, Some(&done.head), ctx);
}

fn notify_approval(p: &mut Poster<'_>, c: &Card, reviewed: Option<&Sha>, ctx: Ctx<'_>) {
    let Some(pr) = c.pr else { return };
    if !c.pushed.as_ref().zip(reviewed).is_some_and(|(a, b)| a == b) {
        return;
    }
    let Some(head) = c.pushed.as_ref() else { return };
    if c.gates.approved.as_ref().is_some_and(|(approved, _)| approved == head) {
        return;
    }
    let current = p.state.world.open_prs.get(&c.project).and_then(|prs| prs.iter().find(|x| x.pr == pr));
    if current.is_some_and(|pr| pr.head != *head) {
        return;
    }
    let (id, link) = (c.id.clone(), link(c, pr, ctx));
    let words = format!(
        "PR {pr} at {head} was reviewed with no findings. Please review and approve my PR on Bitbucket: {link}."
    );
    let stale: Vec<factory_types::AskId> = p
        .all_asks()
        .filter(|a| a.open && a.place == id.as_str() && a.kind == AskKind::ApprovePr && a.text != words)
        .map(|a| a.id.clone())
        .collect();
    for ask in stale {
        asks::close_one(p, ask.as_str());
    }
    asks::raise(p, Raise::new(id.as_str(), AskKind::ApprovePr, Color::Yellow, words));
}

/// Tracks the approval observed on our current pushed head, closing obsolete approval asks.
pub fn approval_read(p: &mut Poster<'_>, c: &mut Card, seen: &PrSeen, ctx: Ctx<'_>) {
    let current = seen.state == PrState::Open && c.pushed.as_ref() == Some(&seen.head);
    let approved = current && seen.approved;
    let was = c.gates.approved.clone().filter(|(h, _)| *h == seen.head);
    let next = approved.then(|| was.unwrap_or_else(|| (seen.head.clone(), p.now)));
    if c.gates.approved != next {
        c.gates.approved = next;
        p.put_card(c.clone());
    }
    if approved || !current {
        asks::close(p, c.id.as_str(), &[AskKind::ApprovePr]);
    } else if let Some(Landing::Approval { commit, verified: true, .. }) = c.landing.clone() {
        notify_approval(p, c, commit.as_ref(), ctx);
    } else if let Some(review) = p.card(&crate::reviews::cards::card_id(c.project.as_str(), seen.pr)) {
        notify_reviewed(p, &review, ctx);
    }
}

/// Whether a card's PR waits for Andrei's approval, out of the merge train: still read by the
/// poller, and a new head takes it back through its gates.
pub fn waiting(c: &Card) -> bool {
    matches!(c.landing, Some(Landing::Approval { .. }))
}

fn link(c: &Card, pr: u64, ctx: Ctx<'_>) -> String {
    format!("https://bitbucket.org/{}/pull-requests/{pr}", crate::queue::pr::repo_of(c, ctx.config))
}

/// A new head pushed past the one Andrei was asked to approve closes that ask.
pub fn close_stale(p: &mut Poster<'_>, project: &str, pr: &OpenPr) {
    let Some(c) = p.card_where(|c| holds(c, project, pr.pr)) else { return };
    if c.pushed.as_ref().is_some_and(|h| *h != pr.head) && c.previous_push.as_ref() != Some(&pr.head) {
        asks::close(p, c.id.as_str(), &[AskKind::ApprovePr]);
    }
}
