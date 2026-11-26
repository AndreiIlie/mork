//! Per-reviewer round history (fac-651): records each reviewer's verdict and reported findings,
//! renders them for returning reviewers, and lists kept reviewers for the review foreman.

use std::fmt::Write as _;

use mork_types::CardId;
use mork_types::card::{Card, Mode};
use mork_types::reply::{Outcome, Reply, Role};
use mork_types::review::{FindingState, Raised, ReportedFinding, Review, ReviewerVerdict, Round, Verdict};
use mork_types::state::Agent;

use super::findings::at;
use crate::post::Poster;
use crate::state::State;

/// Records a reviewer's latest verdict and reported findings on the open round it reviewed.
pub(crate) fn record(p: &mut Poster<'_>, id: &CardId, round_n: u32, reviewer: &str, reply: &Reply) {
    let Some(mut rc) = p.card(id) else { return };
    let Some(round) = rc.review.as_deref_mut().and_then(|r| r.rounds.iter_mut().find(|r| r.n == round_n)) else {
        return;
    };
    let findings = reply
        .findings
        .iter()
        .map(|f| ReportedFinding { file: f.file.clone(), line: f.line, summary: f.summary.clone() })
        .collect();
    let verdict = ReviewerVerdict { passed: reply.outcome == Outcome::ReviewPass, findings };
    round.reviewers.insert(reviewer.to_owned(), verdict);
    p.put_card(rc);
}

/// What a returning reviewer is told on every turn of its earlier completed rounds and kept findings.
pub(crate) fn for_reviewer(rc: &Card, a: &Agent) -> String {
    if rc.mode != Mode::Review || a.role != Role::Reviewer {
        return String::new();
    }
    let Some(review) = rc.review.as_deref() else { return String::new() };
    let name = a.key.as_str().rsplit('/').next().unwrap_or_default();
    let entries: Vec<(&Round, Option<&ReviewerVerdict>, Vec<&Raised>)> = review
        .rounds
        .iter()
        .filter(|r| r.verdict.is_some())
        .filter_map(|r| {
            let v = r.reviewers.get(name);
            let kept = kept_for(review, r, name);
            (v.is_some() || !kept.is_empty()).then_some((r, v, kept))
        })
        .collect();
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n## Your earlier rounds\n\nWhat Mork recorded of your earlier rounds on this card, and what became of findings kept under your name:\n",
    );
    for (r, v, kept) in entries {
        write_round(&mut out, review, r, v, &kept);
    }
    out
}

// Single blindness filter: only findings kept under `name` belong in that reviewer's view.
fn kept_for<'a>(review: &'a Review, r: &Round, name: &str) -> Vec<&'a Raised> {
    review.raised.iter().filter(|f| f.by == name && (f.round == r.n || r.named.contains(&f.n))).collect()
}

fn write_round(out: &mut String, review: &Review, r: &Round, v: Option<&ReviewerVerdict>, kept: &[&Raised]) {
    let summary = v.map_or_else(|| "earlier verdict not recorded".to_owned(), verdict_summary);
    let _ = writeln!(out, "\n- Round {} ({}): {summary}", r.n, round_tags(review, r));
    if let Some(v) = v {
        for (i, f) in v.findings.iter().enumerate() {
            let _ = writeln!(out, "  {}. {}: {}", i + 1, at(&f.file, f.line), f.summary);
        }
    }
    if r.verdict != Some(Verdict::Superseded) {
        let reported = v.is_some_and(|v| !v.passed || !v.findings.is_empty());
        write_kept(out, r.n, kept, reported);
    }
}

fn endpoints(r: &Round) -> String {
    let mut out = format!("commit `{}`, base `{}`", r.commit.short(), r.base.short());
    if let Some(head) = &r.head {
        let _ = write!(out, ", head `{}`", head.short());
    }
    out
}

fn round_tags(review: &Review, r: &Round) -> String {
    let mut tags = endpoints(r);
    if review.afresh.is_some_and(|from| r.n < from) {
        tags.push_str(", under an earlier plan");
    }
    if r.verdict == Some(Verdict::Superseded) {
        tags.push_str(", superseded — no foreman verdict");
    }
    tags
}

fn verdict_summary(v: &ReviewerVerdict) -> String {
    let outcome = if v.passed { "passed" } else { "failed" };
    match (v.passed, v.findings.len()) {
        (true, 0) => outcome.to_owned(),
        (false, 0) => format!("{outcome}, no findings reported"),
        (_, n) => format!("{outcome}, {n} finding(s) reported"),
    }
}

fn write_kept(out: &mut String, round_n: u32, kept: &[&Raised], reported: bool) {
    if kept.is_empty() {
        if reported {
            let _ = writeln!(out, "  Kept under your name in round {round_n}'s verdict: none.");
        }
        return;
    }
    let _ = writeln!(out, "  Kept under your name in round {round_n}'s verdict:");
    for f in kept {
        let from = if f.round != round_n { format!("from round {}, ", f.round) } else { String::new() };
        let _ = writeln!(out, "  - #{} ({from}{}) {}: {}", f.n, status_of(f), at(&f.file, f.line), f.summary);
    }
}

// Withdrawal and PR-thread answers refine the shared finding-state ladder.
fn status_of(f: &Raised) -> &'static str {
    if f.withdrawn {
        return "withdrawn";
    }
    match crate::views::findings::state_of(f) {
        FindingState::Dropped => "dropped",
        FindingState::Declined => "declined",
        FindingState::Resolved => "resolved",
        FindingState::Open if f.answer.is_some() => "open — answered",
        FindingState::Open => "open",
    }
}

/// Live reviewers kept from earlier rounds, with each reviewer's topic and last reviewed round.
pub(super) fn kept_reviewers(state: &State, rc: &Card, n: u32) -> String {
    let Some(review) = rc.review.as_deref() else { return String::new() };
    let kept: Vec<&Agent> = crate::agents::children::of_card(state, &rc.id)
        .into_iter()
        .filter(|w| w.role == Role::Reviewer)
        .filter_map(|w| state.agents.get(&w.key).filter(|a| crate::agents::closing::kept(a)))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n\nKept reviewers from earlier rounds (each is also told its own earlier verdict and findings by Mork):",
    );
    for a in kept {
        let name = a.key.as_str().rsplit('/').next().unwrap_or_default();
        let topic = a.topic.as_deref().unwrap_or("no topic recorded");
        match last_round(review, n, name) {
            Some((r, v)) => {
                let _ = write!(out, "\n- {name} ({topic}): {}", reviewer_summary(r, v));
            }
            None => {
                let _ = write!(out, "\n- {name} ({topic}): no earlier round verdict recorded");
            }
        }
    }
    out
}

fn last_round<'a>(review: &'a Review, n: u32, name: &str) -> Option<(&'a Round, &'a ReviewerVerdict)> {
    review
        .rounds
        .iter()
        .rev()
        .filter(|r| r.n < n && r.verdict.is_some())
        .find_map(|r| r.reviewers.get(name).map(|v| (r, v)))
}

fn reviewer_summary(r: &Round, v: &ReviewerVerdict) -> String {
    let mut meta = endpoints(r);
    if r.verdict == Some(Verdict::Superseded) {
        meta.push_str(", superseded");
    }
    format!("last reviewed round {} ({meta}): {}", r.n, verdict_summary(v))
}
