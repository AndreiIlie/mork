//! PR and branch entries reuse their review cards through merge, checks, then review and suite side by side (fac-111,
//! fac-375), off the merge queue. Completing verification reports the result and does not authorize a merge.

use mork_types::Sha;
use mork_types::action::ActionWhat;
use mork_types::ask::AskKind;
use mork_types::card::{Candidate, Card, Landing, Stage};
use mork_types::reply::Role;
use mork_types::review::{Finished, Verdict};
use mork_types::suites::Told;
use mork_types::work::{Made, Purpose};
use serde_json::{Value, json};

use crate::decide::Ctx;
use crate::post::Poster;
use crate::reviews::cards::to_operator;

/// A fetch read an entry's base and head: it enters the back of its project's train as our cards do, or
/// keeps its place. A run's findings are its own; earlier ones stay open with their raisers until verified.
pub fn enter(p: &mut Poster<'_>, mut c: Card, base: Sha, head: Sha, ctx: Ctx<'_>) {
    let at = if c.stage == Stage::Merging && c.landing.is_some() { super::order::place(&c) } else { p.now };
    let project = c.project.clone();
    c.base_tip = Some(base.clone());
    c.head = Some(head.clone());
    c.stage = Stage::Merging;
    c.checks_output = None;
    c.findings.clear();
    c.landing = Some(Landing::Queued { at });
    crate::messages::asks::close(p, c.id.as_str(), &[AskKind::Done]);
    p.journal("entry.entered", Some(c.id.as_str()), json!({"head": head, "on": base}));
    p.put_card(c);
    super::train::maintain(p, project.as_str(), ctx);
}

/// The tip a fetch read that an entry's candidate is made on: its PR destination's, or the base's
/// when that is its project's base or it has none; our card's the base's.
pub fn on(c: &Card, tip: &Sha, v: &Value, ctx: Ctx<'_>) -> Option<Sha> {
    let Some(dest) = c.review.as_ref().filter(|_| crate::reviews::gate::entry(c)).and_then(|r| r.dest.clone()) else {
        return Some(tip.clone());
    };
    let read = v.get("tips").and_then(|t| t.get(&dest)).and_then(Value::as_str).map(Sha::new);
    let base = ctx.config.projects.get(&c.project).is_some_and(|x| x.base == dest);
    read.or_else(|| base.then(|| tip.clone()))
}

/// Drops an entry's round, candidate and run for a new head so it re-enters on its fetched base;
/// does nothing when its open round already reviews `head` (fac-570).
pub fn moved(p: &mut Poster<'_>, c: Card, head: &Sha) {
    if reviews_open(&c, head) {
        return;
    }
    let id = c.id.clone();
    crate::reviews::end::supersede(p, c.clone(), Some(head));
    let mut c = p.card(&id).unwrap_or(c);
    super::candidates::drop_candidate(p, &mut c);
    c.landing = None;
    c.head = Some(head.clone());
    c.base_tip = None;
    c.stage = Stage::Starting;
    if let Some(r) = c.review.as_deref_mut() {
        r.finished = None;
    }
    p.put_card(c);
    p.journal("entry.moved", Some(id.as_str()), json!({"head": head}));
}

/// Restarts a live branch review at its fetched tip, abandoning the previous run while retaining
/// its foreman, reviewers and findings.
pub fn repeat(p: &mut Poster<'_>, c: Card) {
    let (id, head) = (c.id.clone(), c.head.clone());
    crate::reviews::end::supersede(p, c.clone(), None);
    let mut c = p.card(&id).unwrap_or(c);
    super::candidates::drop_candidate(p, &mut c);
    c.landing = None;
    c.head = None;
    c.base_tip = None;
    c.stage = Stage::Starting;
    c.failed_job = None;
    if let Some(r) = c.review.as_deref_mut() {
        r.finished = None;
    }
    crate::messages::asks::close(p, id.as_str(), &[AskKind::Done]);
    p.put_card(c);
    p.journal("entry.repeated", Some(id.as_str()), json!({"head": head}));
}

/// Whether the poller's read of a PR moves its live entry: a head it doesn't have and its open round isn't already
/// reviewing, on an unpaused card; a stopped one is read again after its `resume`.
pub fn moves(c: &Card, head: &Sha) -> bool {
    crate::reviews::gate::entry(c) && c.stage.is_live() && !c.paused && !has(c, head) && !reviews_open(c, head)
}

/// Syncs `c.head` and its open round's source head when `head` is already what that round reviews (fac-570).
/// Skips while `Prepare` runs so an in-flight candidate job for the previous head is not invalidated.
pub fn sync_open_head(c: &mut Card, head: &Sha) -> bool {
    if !crate::reviews::gate::entry(c)
        || !c.stage.is_live()
        || c.paused
        || matches!(c.landing, Some(Landing::Prepare(_)))
        || has(c, head)
        || !reviews_open(c, head)
    {
        return false;
    }
    c.head = Some(head.clone());
    if let Some(open) = c.review.as_deref_mut().and_then(mork_types::review::Review::open_mut) {
        open.head = Some(head.clone());
    }
    true
}

pub fn has(c: &Card, head: &Sha) -> bool {
    c.head.as_ref() == Some(head)
}

fn reviews_open(c: &Card, head: &Sha) -> bool {
    c.review
        .as_deref()
        .and_then(mork_types::review::Review::open)
        .is_some_and(|o| crate::reviews::end::reviews_head(o, head))
}

/// Its candidate conflicts with the base's tip: its run stops there, said on its PR when its review
/// posts, and our card whose PR it is goes to its join through its foreman.
pub fn conflicted(p: &mut Poster<'_>, mut c: Card, k: Candidate, purpose: &Purpose, m: &Made, ctx: Ctx<'_>) {
    super::candidates::release(p, &c.id, k.clone);
    let into = dest(&c, ctx);
    let why = super::candidates::conflict(purpose, &into, &k.on, m);
    finished(&mut c, &k, false, true);
    c.landing = None;
    c.stage = Stage::Review;
    p.put_card(c.clone());
    p.journal("entry.conflict", Some(c.id.as_str()), json!({"head": c.head, "on": k.on, "files": m.conflicts}));
    // Our PR's conflict goes to its card's foreman alone, as a join (fac-375).
    let ours = c.pr.and_then(|n| super::approval::owner(p.state, c.project.as_str(), n)).cloned();
    if let Some(ours) = ours.and_then(|o| p.card(&o.id)) {
        super::approval::join(p, ours, &why, k.on.clone(), ctx);
    } else {
        to_operator(p, &c.id, &format!("Its run stops: {why}."));
        if let Some(pr) = c.pr {
            let body = format!("This PR conflicts with {into}: {why}.");
            let repo = super::pr::repo_of(&c, ctx.config);
            let what =
                ActionWhat::Comment { source: None, card: c.id.clone(), pr, file: String::new(), line: 0, body, repo };
            crate::reviews::findings::comment(p, what, false);
        }
    }
    crate::reviews::results::done_once_posted(p, &c.id, ctx);
}

/// Records and reports a completed run, its round and its suite both ended (fac-375): it only reports, never lands.
/// Clean requires approved content, no findings awaiting verification, and no failure; a remembered suite pass suffices.
pub fn finish(p: &mut Poster<'_>, mut c: Card, k: Candidate, failure: Option<String>, ctx: Ctx<'_>) {
    super::candidates::release(p, &c.id, k.clone);
    let covered = c.review.as_ref().is_some_and(|r| r.covers(&k.contents));
    let waiting: Vec<String> = c.review.iter().flat_map(|r| r.open_findings()).map(|x| format!("#{}", x.n)).collect();
    let reviewed = covered && waiting.is_empty();
    finished(&mut c, &k, reviewed && failure.is_none(), failure.is_some());
    c.landing = None;
    c.stage = Stage::Review;
    p.put_card(c.clone());
    let data = json!({"head": c.head, "on": k.on, "reviewed": reviewed, "failed": failure.is_some()});
    p.journal("entry.finished", Some(c.id.as_str()), data);
    let at = k.commit.as_ref().map_or("?", |x| x.short());
    // Our PR's failure goes to its card's foreman alone, in its fix request (fac-375).
    let ours = c.pr.and_then(|n| super::approval::owner(p.state, c.project.as_str(), n)).map(|o| o.id.clone());
    if let Some(f) = failure.as_ref().filter(|_| ours.is_none()) {
        super::suite_news::send(p, &c.id, Told::Operator { card: c.id.clone(), text: f.clone() });
    }
    // A finding's raiser still verifies its fix, a revert's too; approval alone isn't verification.
    let failed = c.review.as_ref().and_then(|r| r.rounds.last()?.verdict) == Some(Verdict::Failed);
    if covered && !waiting.is_empty() && failure.is_none() && !failed {
        let text = format!(
            "Its rounds approve all it has, but finding(s) {} wait for their raisers to verify their fixes: it isn't clean until they do, or you resolve them.",
            waiting.join(", ")
        );
        match &ours {
            Some(o) => {
                crate::messages::requests::tell_agent(
                    p,
                    o,
                    Role::Foreman,
                    "its PR's findings wait for their raisers",
                    &text,
                );
            }
            None => to_operator(p, &c.id, &text),
        }
        return if c.pr.is_some() {
            crate::reviews::results::report(p, c, false, None, &[], ctx)
        } else {
            crate::reviews::results::done_once_posted(p, &c.id, ctx)
        };
    }
    let unmet = failure.as_ref().map(|_| format!("its run on {at} failed"));
    crate::reviews::results::report(p, c.clone(), reviewed, unmet, &k.failing, ctx);
    fix_request(p, &c, at, failure.as_deref(), ctx);
}

// Our card's PR came back with findings, or a failed run: its foreman is asked to fix them on its
// branch, whose new head the poller reads and sends through the next round.
fn fix_request(p: &mut Poster<'_>, c: &Card, at: &str, failure: Option<&str>, ctx: Ctx<'_>) {
    let Some(pr) = c.pr else { return };
    let Some(ours) = super::approval::owner(p.state, c.project.as_str(), pr).cloned() else { return };
    let mut what = Vec::new();
    if !c.findings.is_empty() {
        let n = c.findings.len();
        what.push(format!(
            "Its review found {n} finding(s), to fix:\n\n{}",
            crate::lifecycle::fixes::listed(&c.findings)
        ));
    }
    if let Some(f) = failure {
        what.push(format!("Its run on {at} failed:\n\n{f}"));
    }
    if what.is_empty() {
        return;
    }
    // Its last round with a verdict failed: its own proofs went to our clone, or the copy's result says why not (fac-377).
    let round = c.review.as_ref().and_then(|r| r.rounds.iter().rev().find(|x| x.verdict.is_some()));
    let failed = round.filter(|r| r.verdict == Some(Verdict::Failed));
    let proofs = failed.map(crate::reviews::verdict::proofs).unwrap_or_default();
    let moved = read(c, &ours, ctx);
    let closing = if failure.is_some() {
        format!(
            "A `built` of the same head runs nothing. For a failure outside {0}'s files, such as a flaky test, `mork ctl rereview {0}` runs the same head again; else fix them on {0}'s branch and submit its new commit: its new head is published to PR {pr}, and the next run is on it, the reviewer of each finding verifying its fix.",
            ours.id
        )
    } else {
        format!(
            "Fix them on {}'s branch and submit as before: its new head is published to PR {pr}, and the next round runs on it, the reviewer of each finding verifying its fix.",
            ours.id
        )
    };
    let text = format!(
        "PR {pr}'s review of {} came back.{moved}\n\n{}{proofs}\n\n{closing}",
        c.head.as_ref().map_or("its head", |h| h.short()),
        what.join("\n\n")
    );
    let (role, headline) = (Role::Foreman, format!("PR {pr}'s findings"));
    super::suite_news::send(p, &c.id, Told::Agent { card: ours.id.clone(), role, headline, text });
    p.journal("entry.fix_request", Some(ours.id.as_str()), json!({"review": c.id, "findings": c.findings.len()}));
}

// Our card's clone never has its published merge (fac-354): the head it was made of names what
// the review read, the moved-on sentence only a head past that one.
fn read(c: &Card, ours: &Card, ctx: Ctx<'_>) -> String {
    let pushed = |r: &&Sha| ours.pushed.as_ref() == Some(*r);
    let made = |r: &Sha| ours.pushed_head.clone().filter(|h| h != r);
    let Some((r, h)) = c.head.as_ref().filter(pushed).and_then(|r| Some((r, made(r)?))) else {
        return crate::reviews::verdict::moved_on(c.head.as_ref(), ours);
    };
    let moved = crate::reviews::verdict::moved_on(Some(&h), ours);
    format!(" {} is {}'s head {} with {} merged in.{moved}", r.short(), ours.id, h.short(), dest(c, ctx))
}

// A conflict or a failed run is `failed`: no approval merges it.
fn finished(c: &mut Card, k: &Candidate, clean: bool, failed: bool) {
    let head = c.head.clone().or_else(|| k.commit.clone());
    if let (Some(r), Some(head)) = (c.review.as_deref_mut(), head) {
        r.finished = Some(Finished { head, on: k.on.clone(), clean, failed });
    }
}

// The branch its candidate merges in: its PR's destination, else the project's base.
fn dest(c: &Card, ctx: Ctx<'_>) -> String {
    let base = || ctx.config.projects.get(&c.project).map(|x| x.base.clone());
    c.review.as_ref().and_then(|r| r.dest.clone()).or_else(base).unwrap_or_default()
}
