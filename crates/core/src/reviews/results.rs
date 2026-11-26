//! A review run's result (fac-269): reported to the operator, a colleague's PR result posted as its comments, ours only clean
//! (fac-375), and the card closed once they are posted. A review verdict alone never merges a PR.

use mork_types::CardId;
use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::card::{Card, Stage};
use mork_types::work::Failing;

use crate::decide::Ctx;
use crate::post::Poster;

/// A clean automatic review's one comment on the whole PR, and a verified PR of ours' (fac-375).
pub const NO_FINDINGS: &str = "No findings.";

// The start of a failed suite's one comment on the whole PR, its failing tests listed under it.
const TESTS_FAILED: &str = "Tests failed";

/// Reports the completed run, posts every PR result, and closes once posting finishes.
pub fn report(p: &mut Poster<'_>, card: Card, clean: bool, unmet: Option<String>, failing: &[Failing], ctx: Ctx<'_>) {
    let text = match card.pr {
        Some(pr) => posted(p, &card, pr, clean, unmet.as_deref(), failing, ctx),
        None => Some(branch(&card, clean, unmet.as_deref())),
    };
    if let Some(text) = text {
        super::cards::to_operator(p, &card.id, &text);
    }
    done_once_posted(p, &card.id, ctx);
}

// Each completed PR run's findings or clean result posted, and any suite failure.
fn posted(
    p: &mut Poster<'_>,
    card: &Card,
    pr: u64,
    clean: bool,
    unmet: Option<&str>,
    failing: &[Failing],
    ctx: Ctx<'_>,
) -> Option<String> {
    let has_findings =
        !card.findings.is_empty() || card.review.as_ref().is_some_and(|r| r.open_findings().next().is_some());
    if crate::queue::approval::owner(p.state, card.project.as_str(), pr).is_some() {
        return ours(p, card, pr, !clean || has_findings || unmet.is_some(), ctx);
    }
    let findings = if has_findings { super::findings::failed(p, card, ctx) } else { String::new() };
    let head = card.head.as_ref().map_or_else(String::new, |h| format!(" at {}", h.short()));
    let result = (!has_findings).then(|| NO_FINDINGS.to_owned());
    let failed = unmet.map(|_| match failing.is_empty() {
        true => format!("{TESTS_FAILED}{head}."),
        false => format!("{TESTS_FAILED}{head}.\n\n```\n{}\n```", crate::queue::failing::listed(failing)),
    });
    for body in result.into_iter().chain(failed) {
        let (id, repo) = (card.id.clone(), crate::queue::pr::repo_of(card, ctx.config));
        let what = ActionWhat::Comment { source: None, card: id, pr, file: String::new(), line: 0, body, repo };
        // Each finished run's result is its own, as a run finishes once: a retry is its same action.
        super::findings::comment(p, what, true);
    }
    if clean && unmet.is_none() {
        crate::queue::approval::notify_reviewed(p, card, ctx);
    }
    let text = match (has_findings, unmet) {
        (true, None) => format!("The review has findings: {findings}."),
        (true, Some(why)) => {
            format!("The review has findings, and {why}: {findings}, beside one saying its tests failed.")
        }
        (false, Some(why)) => {
            format!(
                "The review has no findings, but {why}: PR {pr} gets its result and one comment saying its tests failed."
            )
        }
        (false, None) => format!("The review has no findings: PR {pr} gets one comment saying so."),
    };
    let forge = ctx.config.forge_label(&card.project);
    if !ctx.config.projects.get(&card.project).is_some_and(|p| p.forge.merges_on_approval()) {
        return Some(format!("{text} Merge the PR manually on {forge}."));
    }
    Some(format!("{text} Only your approval on {forge} merges it, once its run passed on the head you approved."))
}

// A review of our own PR, an older build's or of a head no verification passed: its findings and its failed suite go
// to our card's foreman alone, the operator told nothing; the PR gets only a clean result (fac-375).
fn ours(p: &mut Poster<'_>, card: &Card, pr: u64, failed: bool, ctx: Ctx<'_>) -> Option<String> {
    if failed {
        return None;
    }
    let (id, repo) = (card.id.clone(), crate::queue::pr::repo_of(card, ctx.config));
    let what = ActionWhat::Comment {
        source: None,
        card: id,
        pr,
        file: String::new(),
        line: 0,
        body: NO_FINDINGS.to_owned(),
        repo,
    };
    super::findings::comment(p, what, true);
    crate::queue::approval::notify_reviewed(p, card, ctx);
    Some(format!("The review of our PR {pr} has no findings: it gets one comment saying so."))
}

// A branch's result: its findings, or clean, and why it can't land if so.
fn branch(card: &Card, clean: bool, unmet: Option<&str>) -> String {
    let (n, listed) = (card.findings.len(), crate::lifecycle::fixes::listed(&card.findings));
    if clean {
        match unmet {
            None => "The review is clean: nothing to comment.".to_owned(),
            Some(why) => format!("The review is clean, but {why}."),
        }
    } else {
        format!(
            "The review of branch {} failed, with {n} finding(s):\n\n{listed}",
            card.branch.as_deref().unwrap_or_default()
        )
    }
}

/// Closes a completed entry once its result comments finish, retaining its seats and review history.
pub fn done_once_posted(p: &mut Poster<'_>, id: &CardId, ctx: Ctx<'_>) {
    let Some(c) = p.card(id).filter(|c| c.stage == Stage::Review && c.landing.is_none()) else { return };
    let current = c.review.as_ref().and_then(|r| r.finished.as_ref());
    if !super::gate::entry(&c) || !current.is_some_and(|f| c.head.as_ref() == Some(&f.head)) {
        return;
    }
    let mine = own(p, id);
    if mine.iter().any(|a| !matches!(a.status, ActionStatus::Done | ActionStatus::Dropped)) {
        return;
    }
    let project = c.project.clone();
    // Our PR's ending is no news of the operator's: its results went to our card's foreman (fac-375).
    if !c.pr.is_some_and(|n| crate::queue::approval::owner(p.state, c.project.as_str(), n).is_some()) {
        let threads = super::threads::notice(p, &c);
        super::cards::to_operator(
            p,
            id,
            &format!("The review is complete and its results are posted: the review card ends.{threads}"),
        );
    }
    crate::lifecycle::stops::superseded(p, c, ctx);
    crate::agents::closing::card_ended(p, id, false, ctx.config);
    crate::queue::train::maintain(p, project.as_str(), ctx);
}

/// A review card's comments of its run, as unrelated approval comments must not close a review whose result
/// is still being posted.
pub(super) fn own(p: &Poster<'_>, id: &CardId) -> Vec<Action> {
    let review = p.card(id).and_then(|c| c.review);
    let mine = |a: &Action| review.as_ref().is_none_or(|r| r.comments.contains(&a.id));
    let of = |a: &Action| matches!(&a.what, ActionWhat::Comment { card, .. } | ActionWhat::ReplyComment { card, .. } if card == id);
    p.all_actions().filter(|a| of(a) && mine(a)).cloned().collect()
}
