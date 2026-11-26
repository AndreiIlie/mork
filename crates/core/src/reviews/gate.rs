//! Review cards retain their foreman across rounds; passes approve file contents (fac-95, fac-111).
//! Changed files need another round; PR and branch entries review themselves. See `verdict`.

use std::collections::BTreeSet;

use mork_types::action::{ActionStatus, ActionWhat};
use mork_types::card::{Card, Mode, Stage};
use mork_types::config::ReviewKind;
use mork_types::reply::Role;
use mork_types::review::{Contents, Review, Round, Verdict};
use mork_types::work::{Cloned, Purpose};
use mork_types::{CardId, Sha};
use serde_json::json;

use crate::decide::{Ctx, Intent};
use crate::post::Poster;
use crate::state::State;

/// Our card's review card: its branch's, which every run of it keeps (fac-111).
pub fn id_of(subject: &Card) -> CardId {
    let branch = subject.branch.clone().unwrap_or_else(|| subject.id.as_str().to_lowercase());
    super::cards::branch_id(subject.project.as_str(), &branch)
}

// Whether a card is reviewed by its branch's card: a full card. A review card is never gated, so no review card
// reviews another.
fn gated(c: &Card) -> bool {
    c.mode == Mode::Full
}

/// A PR's or a branch's card: its own train entry, reviewing itself (fac-111).
pub fn entry(c: &Card) -> bool {
    c.mode == Mode::Review && c.review.as_ref().is_some_and(|r| r.of.is_none())
}

/// Whether a candidate's verification owes a review round: an entry's, and our gated card's, on a repository with PRs
/// too, verified before its PR is published (fac-375).
pub fn reviewed(c: &Card) -> bool {
    entry(c) || gated(c)
}

/// The Mork card a branch card reviews; none for an entry.
pub fn subject_of(c: &Card) -> Option<&CardId> {
    c.review.as_ref().and_then(|r| r.of.as_ref()).filter(|_| c.mode == Mode::Review)
}

/// What a review card's rounds review, as it stands in this decision: our card, or the entry itself.
pub(super) fn subject(p: &Poster<'_>, rc: &Card) -> Option<Card> {
    if entry(rc) {
        return Some(p.card(&rc.id).unwrap_or_else(|| rc.clone()));
    }
    subject_of(rc).and_then(|s| p.card(s))
}

/// Branch tips needed by live PR destinations, branch entries without heads, and continued card
/// runs (fac-111).
pub fn branches(state: &State, project: &str) -> Vec<String> {
    let starting = |c: &Card| c.mode != Mode::Review && c.stage == Stage::Starting && c.base_tip.is_none();
    let mut out: Vec<String> = state
        .cards
        .values()
        .filter(|c| c.project == project && c.stage.is_live())
        .filter_map(|c| {
            if starting(c) {
                return c.branch.clone();
            }
            let r = c.review.as_ref().filter(|_| entry(c))?;
            r.dest.clone().or_else(|| c.branch.clone().filter(|_| c.head.is_none()))
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn review_card(state: &State, subject: &Card) -> Option<Card> {
    if entry(subject) {
        return Some(subject.clone());
    }
    if !gated(subject) {
        return None;
    }
    let rc = state.cards.get(&id_of(subject)).filter(|c| subject_of(c) == Some(&subject.id)).cloned();
    rc.or_else(|| state.cards.values().find(|c| renamed(c, subject)).cloned())
}

/// The candidate and base of the subject's last review round with a passed or failed verdict.
pub fn last_read(state: &State, subject: &Card) -> Option<(Sha, Sha)> {
    let rc = review_card(state, subject)?;
    let round = rc.review.as_deref()?.last_read()?;
    Some((round.commit.clone(), round.base.clone()))
}

/// The paths whose content a reviewed card's candidate is read for: each its head touches and each a
/// passed round approved, so that one whose diff the base caught up with is still compared.
pub fn paths(state: &State, subject: &Card) -> BTreeSet<String> {
    if !reviewed(subject) {
        return BTreeSet::new();
    }
    let approved = |c: &Card| {
        c.review.as_ref().map(|r| {
            let mut paths: BTreeSet<String> = r.approved.keys().cloned().collect();
            if entry(c) {
                let read = r.rounds.iter().filter(|r| matches!(r.verdict, Some(Verdict::Passed | Verdict::Failed)));
                paths.extend(read.flat_map(|round| round.contents().into_keys()));
            }
            paths
        })
    };
    let approved = review_card(state, subject).as_ref().and_then(approved);
    subject.contents.keys().cloned().chain(approved.unwrap_or_default()).collect()
}

/// The live review card at `id` that isn't `subject`'s: the operator's review of its branch, which its
/// gate waits for and never replaces (fac-111).
pub fn taken(p: &Poster<'_>, id: &CardId, subject: &CardId) -> Option<CardId> {
    p.card(id).filter(|c| c.stage.is_live() && subject_of(c) != Some(subject)).map(|c| c.id)
}

/// Our card's first round waits: another review holds its gate's id (fac-111).
pub fn blocked(p: &Poster<'_>, subject: &Card) -> Option<CardId> {
    let first = gated(subject) && card_for(p, subject).is_none();
    first.then(|| taken(p, &id_of(subject), &subject.id)).flatten()
}

/// A card's review card, live, as it stands in this decision: an entry's is itself.
pub fn card_for(p: &Poster<'_>, subject: &Card) -> Option<Card> {
    if entry(subject) {
        return p.card(&subject.id).filter(|c| c.stage.is_live());
    }
    let gate = p.card(&id_of(subject)).filter(|c| c.stage.is_live() && subject_of(c) == Some(&subject.id));
    gate.or_else(|| p.card_where(|c| renamed(c, subject)))
}

// The live review card of our card's branch from before its name was made one the push accepts (fac-394).
fn renamed(c: &Card, subject: &Card) -> bool {
    c.stage.is_live() && subject_of(c) == Some(&subject.id)
}

/// Whether a card's `contents` may go on: a round passed, and every file of it is approved as it is.
pub fn approves(p: &Poster<'_>, subject: &Card, contents: &Contents) -> bool {
    card_for(p, subject).and_then(|c| c.review).is_some_and(|r| r.covers(contents))
}

/// Whether our card's `contents` merged onto `on` goes on as its last round read it, failed on findings all declined
/// since (fac-638): the round read this submitted head on this base, and every file no pass approved as it is.
pub fn declined(p: &Poster<'_>, subject: &Card, on: &Sha, contents: &Contents) -> bool {
    let Some(review) = card_for(p, subject).filter(|_| gated(subject)).and_then(|c| c.review) else { return false };
    review.declined().is_some_and(|r| {
        let read = r.contents();
        let same = |(f, c): (&String, &String)| review.approved.get(f) == Some(c) || read.get(f) == Some(c);
        r.submitted.is_some() && r.submitted == subject.head && r.base == *on && contents.iter().all(same)
    })
}

/// Whether a card's `contents` goes on with no round: approved, and no finding left to verify.
pub fn settles(p: &Poster<'_>, subject: &Card, contents: &Contents) -> bool {
    card_for(p, subject).and_then(|c| c.review).is_some_and(|r| !new_pr_head(subject, &r) && r.settled(contents))
}

/// Whether an entry's new head keeps its last failed round: none of `changes`, its changes since,
/// reaches the candidate, and findings stay open, so no round is asked (fac-350).
pub fn keeps(p: &Poster<'_>, subject: &Card, changes: Option<&Contents>) -> bool {
    card_for(p, subject).and_then(|c| c.review).is_some_and(|r| kept(subject, &r, changes))
}

fn kept(subject: &Card, review: &Review, changes: Option<&Contents>) -> bool {
    entry(subject)
        && changes.is_some_and(Contents::is_empty)
        && review.last_read().is_some_and(|r| r.verdict == Some(Verdict::Failed))
        && review.open_findings().next().is_some()
}

// An entry's changes since its last read source head, a PR's or a branch's, as its candidate has them (fac-375).
fn changes(subject: &Card) -> Option<&Contents> {
    crate::queue::order::candidate(subject).filter(|_| entry(subject)).and_then(|k| k.review_changes.as_ref())
}

fn new_pr_head(subject: &Card, review: &Review) -> bool {
    let same = review.last_read().and_then(|r| r.head.as_ref()).zip(subject.head.as_ref());
    entry(subject) && !same.is_some_and(|(a, b)| a == b)
}

fn wanted(
    subject: &Card,
    review: &Review,
    commit: &Sha,
    base: &Sha,
    contents: &Contents,
    test_changes: Vec<String>,
) -> Round {
    let (carried, files) = match changes(subject) {
        Some(changes) => {
            let carried = contents.iter().filter(|(f, _)| !changes.contains_key(*f));
            (carried.map(|(f, c)| (f.clone(), c.clone())).collect(), changes.clone())
        }
        None => {
            let read = review.read();
            review.uncovered(contents).into_iter().partition(|(f, c)| read.get(f) == Some(c))
        }
    };
    let head = subject.head.clone().filter(|_| entry(subject));
    Round {
        n: 0,
        test_changes,
        head,
        commit: commit.clone(),
        base: base.clone(),
        files,
        carried,
        asked: None,
        verdict: None,
        proofs: None,
        submitted: subject.head.clone().filter(|_| !entry(subject)),
        named: Vec::new(),
        reviewers: std::collections::BTreeMap::new(),
    }
}

/// Requests a round for unapproved content or open findings, immediately or after the current one.
/// Fix rounds carry unchanged files and return findings to their raisers (fac-111).
pub fn ask(
    p: &mut Poster<'_>,
    subject: &Card,
    commit: &Sha,
    base: &Sha,
    contents: &Contents,
    test_changes: Vec<String>,
    ctx: Ctx<'_>,
) {
    if !gated(subject) && !entry(subject) {
        return;
    }
    let found = card_for(p, subject).or_else(|| adopt(p, subject, base, ctx));
    let Some(mut rc) = found.or_else(|| new_card(p, subject, base, ctx)) else { return };
    let mut review = rc.review.take().map(|r| *r).unwrap_or_default();
    let wanted = wanted(subject, &review, commit, base, contents, test_changes);
    let reviews =
        |r: &Round| r.head == wanted.head && wanted.contents().iter().all(|(f, c)| r.contents().get(f) == Some(c));
    let clone = match review.open().cloned() {
        _ if !new_pr_head(subject, &review) && review.settled(contents) || kept(subject, &review, changes(subject)) => {
            review.next = None;
            None
        }
        Some(open) if reviews(&open) => {
            review.next = None;
            None
        }
        // Its clone isn't made yet: the round reviews this instead, its clone made again if need be.
        Some(open) if open.asked.is_none() => {
            review.next = None;
            if let Some(r) = review.open_mut() {
                *r = Round { n: open.n, ..wanted };
            }
            Some(commit.clone())
        }
        Some(_) => {
            review.next = Some(wanted);
            None
        }
        // A failed round's proofs are copied before a round's clone replaces them (fac-377).
        None if review.proofs_pending() => {
            review.next = Some(wanted);
            None
        }
        None => {
            review.next = None;
            let n = review.rounds.last().map_or(1, |r| r.n + 1);
            review.rounds.push(Round { n, ..wanted });
            Some(commit.clone())
        }
    };
    if let Some(r) = clone.as_ref().and_then(|_| review.open()) {
        let files: Vec<&String> = r.files.keys().collect();
        p.journal("review.round", Some(rc.id.as_str()), json!({"n": r.n, "commit": r.commit, "files": files}));
    }
    rc.review = Some(Box::new(review));
    p.put_card(rc.clone());
    if let Some(at) = clone {
        clone_round(p, &rc, &at);
    }
}

// Our card's PR an older build published, its own review card live or ended with findings open: our card's verification
// takes it over as its gate, its findings kept for their raisers, a round or run of it under way dropped: no fix returns.
fn adopt(p: &mut Poster<'_>, subject: &Card, base: &Sha, ctx: Ctx<'_>) -> Option<Card> {
    let id = super::cards::card_id(subject.project.as_str(), subject.pr?);
    let prior = p.card(&id).filter(|c| entry(c) && c.pr == subject.pr)?;
    let mut rc = if prior.stage.is_live() {
        super::end::adopted(p, prior, &subject.id);
        let mut rc = p.card(&id)?;
        crate::queue::candidates::drop_candidate(p, &mut rc);
        rc
    } else if prior.review.as_ref().is_some_and(|r| r.open_findings().next().is_some()) {
        super::cards::reopen(p, prior, base, ctx)
    } else {
        return None;
    };
    rc.landing = None;
    rc.delegated = subject.delegated;
    rc.self_sustained = subject.delegated && subject.self_sustained;
    rc.stage = Stage::Review;
    rc.gates.review_asked = None;
    if let Some(r) = rc.review.as_deref_mut() {
        r.of = Some(subject.id.clone());
        r.finished = None;
    }
    p.put_card(rc.clone());
    p.journal("review.adopted", Some(id.as_str()), json!({"of": subject.id, "run": rc.run}));
    Some(rc)
}

// Our card's first round makes its branch card, its clone made at each round's commit; none while
// another review holds its id.
fn new_card(p: &mut Poster<'_>, subject: &Card, base: &Sha, ctx: Ctx<'_>) -> Option<Card> {
    let id = id_of(subject);
    if taken(p, &id, &subject.id).is_some() {
        return None;
    }
    let run = p.card(&id).map_or(1, |c| c.run + 1);
    for key in agents_of(p, &id) {
        p.remove_agent(&key);
    }
    let branch = subject.branch.clone().unwrap_or_else(|| subject.id.as_str().to_lowercase());
    let title = format!("Branch {branch} of {}: {}", subject.id, subject.title);
    let mut c = Card::new(id.clone(), subject.project.as_str(), &title, Mode::Review, run, ctx.now);
    let carries: Vec<&str> = subject.carries.iter().map(|k| k.id.as_str()).collect();
    let carrying = if carries.is_empty() { String::new() } else { format!(", which carries {}", carries.join(", ")) };
    c.description = format!(
        "The review of {}'s branch{carrying}: its verification passes only what its rounds approved.",
        subject.id
    );
    c.delegated = subject.delegated;
    c.self_sustained = subject.delegated && subject.self_sustained;
    c.stage = Stage::Review;
    c.branch = Some(branch);
    c.base_tip = Some(base.clone());
    c.first_seq = p.state.next_seq;
    c.review = Some(Box::new(Review { of: Some(subject.id.clone()), ..Review::default() }));
    p.touch_place(id.as_str());
    p.journal("card.start", Some(id.as_str()), json!({"run": run, "mode": "review", "of": subject.id}));
    Some(c)
}

/// Linked branch reviews and our live cards' PRs use inside defaults; other reviews use outside defaults.
pub fn kind(state: &State, rc: &Card) -> ReviewKind {
    if subject_of(rc).is_some() || owner_of(state, rc).is_some() { ReviewKind::Inside } else { ReviewKind::Outside }
}

// The existing PR ownership rule also determines its review policy (fac-469).
fn owner_of<'a>(state: &'a State, rc: &Card) -> Option<&'a Card> {
    let n = rc.pr.filter(|_| entry(rc))?;
    crate::queue::approval::owner(state, rc.project.as_str(), n)
}

/// A card's agents, its foreman and its reviewers.
pub fn agents_of(p: &Poster<'_>, id: &CardId) -> Vec<mork_types::AgentKey> {
    p.state.agents.keys().filter(|k| k.as_str().split('/').next() == Some(id.as_str())).cloned().collect()
}

// The round's clone, made fresh at its commit; while one is being made or installed, its end
// makes the next.
fn clone_round(p: &mut Poster<'_>, rc: &Card, at: &Sha) {
    let id = rc.id.clone();
    let mine = |x: &Purpose| matches!(x, Purpose::Clone { card, .. } | Purpose::Install { card, .. } if *card == id);
    let running = p
        .state
        .jobs
        .values()
        .any(|j| j.settled.is_none() && p.ending.as_ref() != Some(&j.id) && j.purpose.as_ref().is_some_and(mine));
    let owed = p.state.owed_work.values().any(mine)
        || p.intents.iter().any(|i| matches!(i, Intent::StartJob { purpose } if mine(purpose)));
    if running || owed || removing(p, &id) {
        return;
    }
    let branch = rc.branch.clone().unwrap_or_else(|| id.as_str().to_owned());
    p.intent(Intent::StartJob { purpose: Purpose::Clone { card: id.clone(), at: at.clone(), branch } });
}

// An ended run's clone removal under way, which a reopened card's new clone waits for: one not started yet sees its run
// is over, and leaves the clone.
fn removing(p: &Poster<'_>, id: &CardId) -> bool {
    p.all_actions().any(|a| {
        matches!(&a.what, ActionWhat::RemoveClone { card, .. } if card == id) && a.status == ActionStatus::Started
    })
}

/// A review card's round still unasked makes its clone, unless one is under way: after a clone removal ended, done or
/// not, and on `/resume`, an entry's suite beside it may have taken the card's one failed job (fac-375).
pub fn reclone(p: &mut Poster<'_>, id: &CardId) {
    let Some(rc) = p.card(id).filter(|c| c.mode == Mode::Review && c.stage.is_live()) else { return };
    let open = rc.review.as_ref().and_then(|r| r.open()).filter(|o| o.asked.is_none()).map(|o| o.commit.clone());
    if let Some(at) = open {
        clone_round(p, &rc, &at);
    }
}

/// A review card's clone is made at `at`, or `installed` in it, as its clone's end read it: its `install` first
/// (fac-153), then its foreman is asked the round; or, the round asked of another commit since, the clone again.
pub fn ready(p: &mut Poster<'_>, mut rc: Card, c: &Cloned, installed: bool, ctx: Ctx<'_>) {
    let at = &c.at;
    let Some(subject) = subject(p, &rc) else { return };
    let Some(open) = rc.review.as_ref().and_then(|r| r.open()).cloned() else { return };
    if open.asked.is_some() {
        return;
    }
    if open.commit != *at {
        return clone_round(p, &rc, &open.commit);
    }
    let install = ctx.config.projects.get(&rc.project).is_some_and(|pr| pr.install.is_some());
    if install && !installed {
        let purpose = Purpose::Install { card: rc.id.clone(), cloned: Some(Box::new(c.clone())) };
        return p.intent(Intent::StartJob { purpose });
    }
    // An entry's head stays its PR's or its branch's, which the poller compares, and it stays in
    // the train, its round one of its steps (fac-111).
    if !entry(&rc) {
        rc.head = Some(open.commit.clone());
        rc.stage = Stage::Review;
    }
    rc.fork = Some(open.base.clone());
    rc.gates.review_asked = Some(open.commit.clone());
    rc.findings.clear();
    super::threads::keep(&mut rc, &c.threads);
    let ours = match rc.pr.filter(|_| entry(&rc)) {
        Some(n) => crate::queue::approval::owner(p.state, rc.project.as_str(), n).and_then(|c| p.card(&c.id)),
        None => subject_of(&rc).map(|_| subject.clone()),
    };
    let text = super::rounds::request(p.state, &rc, &subject, &open, ours.as_ref(), c, ctx);
    p.put_card(rc.clone());
    let id = rc.id.clone();
    crate::agents::closing::resume(p, &id);
    crate::agents::models::reround(p, &id);
    let seq = crate::messages::requests::ask_agent(p, &id, Role::Foreman, &format!("Review round {}", open.n), &text);
    if let Some(r) = rc.review.as_deref_mut().and_then(Review::open_mut) {
        r.asked = Some(seq);
    }
    p.put_card(rc);
    p.journal("review.asked", Some(id.as_str()), json!({"n": open.n, "commit": open.commit, "seq": seq}));
}
