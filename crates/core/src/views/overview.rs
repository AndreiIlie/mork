// Per-card Overview stage spine built in `core` for the web Card Room (fac-684); it carries no messages (fac-719).

use factory_types::card::{Card, Mode, Stage};
use factory_types::decisions;
use factory_types::review::{FindingState, Review, Round, RoundView, Verdict};
use factory_types::view::{OverviewStage, OverviewStageAt, OverviewStageKind, OverviewView};

use super::findings::{review_card, round_findings, round_view, state_of};
use super::outside::Outside;
use super::pipeline::{land_sub, plan_progress, review_sub, verify_sub};
use super::timeline::{Mark, Point};
use crate::state::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawKind {
    Spec,
    Build,
    Verify(Option<u32>),
    Land,
    Report,
    Review(Option<u32>),
}

struct RawStage {
    kind: RawKind,
    start_ms: i64,
    end_ms: Option<i64>,
    entries: Vec<i64>,
}

struct RoundSlot {
    n: u32,
    start_ms: Option<i64>,
}

pub(super) fn overview(state: &State, card: &Card, outside: &Outside) -> OverviewView {
    // Ignore a finished review card left over from an earlier run of a restarted parent card.
    let review = review_card(state, card)
        .filter(|(rc, _)| card.mode == Mode::Review || rc.stage.is_live() || rc.first_seq >= card.first_seq);
    let points = outside.timelines.get(&card.id).map(Vec::as_slice).unwrap_or_default();
    let raw = build_stages(state, card, review, points);
    let last_spec = raw.iter().rposition(|s| s.kind == RawKind::Spec);
    let last_build = raw.iter().rposition(|s| s.kind == RawKind::Build);
    let review_ref = review.map(|(_, r)| r);
    let stages = raw
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let is_last = i + 1 == raw.len();
            let current = card.stage.is_live() && is_last;
            let flags = (current, is_last, Some(i) == last_spec, Some(i) == last_build);
            OverviewStage {
                kind: stage_kind(st.kind),
                label: stage_label(st.kind),
                at: stage_at(st.kind, current, review_ref),
                start_ms: st.start_ms,
                end_ms: st.end_ms,
                summary: stage_summary(state, card, review_ref, outside, st.kind, flags),
                round: stage_round(st.kind, review_ref),
            }
        })
        .collect();
    // A card sent back from Verify to fix its findings is at work again, its spine still ending at Verify.
    let verifying =
        card.triage.is_some() || matches!(card.stage, Stage::Merging | Stage::Ready | Stage::Review | Stage::Second);
    let live = match raw.last().map(|s| s.kind) {
        _ if !card.stage.is_live() => Vec::new(),
        Some(RawKind::Verify(n)) if verifying => super::live::verify(state, card, review, n, outside),
        _ => super::live::working(state, card, outside),
    };
    OverviewView { stages, live }
}

fn build_stages(state: &State, card: &Card, review: Option<(&Card, &Review)>, points: &[Point]) -> Vec<RawStage> {
    match card.mode {
        Mode::Report => vec![RawStage {
            kind: RawKind::Report,
            start_ms: points.first().map_or(card.started_ms, |p| p.at),
            end_ms: end_mark(points),
            entries: Vec::new(),
        }],
        Mode::Review => review_mode_stages(state, card, points),
        Mode::Full | Mode::Light => {
            let phases = phase_intervals(card, review, points);
            splice_rounds(state, card, phases, review)
        }
    }
}

fn end_mark(points: &[Point]) -> Option<i64> {
    points.iter().rev().find(|p| p.mark == Mark::End).map(|p| p.at)
}

fn review_mode_stages(state: &State, card: &Card, points: &[Point]) -> Vec<RawStage> {
    let start = points.first().map_or(card.started_ms, |p| p.at);
    let end = end_mark(points);
    let slots = card.review.as_deref().map(|r| round_slots(state, card, r, card.first_seq)).unwrap_or_default();
    if slots.is_empty() {
        return vec![RawStage { kind: RawKind::Review(None), start_ms: start, end_ms: end, entries: Vec::new() }];
    }
    let entries = review_entries(points);
    slots_to_stages(&slots, &entries, start, end, |n| RawKind::Review(Some(n)))
}

// Pause/resume re-journals the unchanged stage, so only actual stage changes and Reverify count as entries.
fn review_entries(points: &[Point]) -> Vec<i64> {
    let mut stage = None;
    let mut out = Vec::new();
    for pt in points {
        match pt.mark {
            Mark::Stage(s) if stage == Some(s) => {}
            Mark::Stage(s) => {
                stage = Some(s);
                if matches!(s, Stage::Review | Stage::Second | Stage::Merging) {
                    out.push(pt.at);
                }
            }
            Mark::Reverify => out.push(pt.at),
            Mark::Waiting
            | Mark::Answered
            | Mark::Paused
            | Mark::Resumed
            | Mark::Planning
            | Mark::Planned
            | Mark::Mocking
            | Mark::Mocked
            | Mark::Verified
            | Mark::End => {}
        }
    }
    out
}

// Submission checks stay in Build; pause/resume stage repeats are ignored so Land and Verify entries stay intact.
fn phase_intervals(card: &Card, review: Option<(&Card, &Review)>, points: &[Point]) -> Vec<RawStage> {
    if points.is_empty() {
        return fallback_phases(card, review);
    }
    let mut out: Vec<RawStage> = Vec::new();
    let mut stage: Option<Stage> = None;
    for pt in points {
        if let Mark::Stage(s) = pt.mark {
            if stage == Some(s) {
                continue;
            }
            stage = Some(s);
        }
        apply_phase_mark(&mut out, pt);
    }
    if out.is_empty() { fallback_phases(card, review) } else { out }
}

fn apply_phase_mark(out: &mut Vec<RawStage>, pt: &Point) {
    match pt.mark {
        Mark::Stage(Stage::Starting | Stage::Brief) => push_if_diff(out, RawKind::Spec, pt.at),
        Mark::Stage(Stage::Build) => {
            if out.is_empty() || out.last().is_some_and(|s| s.kind == RawKind::Spec) {
                push_phase(out, RawKind::Build, pt.at);
            }
        }
        Mark::Stage(Stage::Merging | Stage::Review | Stage::Second) | Mark::Reverify => {
            if let Some(last) = out.last_mut().filter(|s| matches!(s.kind, RawKind::Verify(_))) {
                last.entries.push(pt.at);
            } else {
                push_phase(out, RawKind::Verify(None), pt.at);
            }
        }
        Mark::Verified => push_if_diff(out, RawKind::Land, pt.at),
        Mark::End => {
            if let Some(last) = out.last_mut() {
                last.end_ms = Some(pt.at);
            }
        }
        Mark::Stage(Stage::Checks | Stage::Ready | Stage::Report | Stage::Done | Stage::Closed)
        | Mark::Waiting
        | Mark::Answered
        | Mark::Paused
        | Mark::Resumed
        | Mark::Planning
        | Mark::Planned
        | Mark::Mocking
        | Mark::Mocked => {}
    }
}

fn push_if_diff(out: &mut Vec<RawStage>, kind: RawKind, at: i64) {
    if out.last().is_some_and(|s| s.kind == kind) {
        return;
    }
    push_phase(out, kind, at);
}

fn push_phase(out: &mut Vec<RawStage>, kind: RawKind, at: i64) {
    if let Some(last) = out.last_mut() {
        last.end_ms = Some(at);
    }
    out.push(RawStage { kind, start_ms: at, end_ms: None, entries: vec![at] });
}

fn fallback_phases(card: &Card, review: Option<(&Card, &Review)>) -> Vec<RawStage> {
    let t = card.started_ms;
    let mut out = vec![RawStage { kind: RawKind::Spec, start_ms: t, end_ms: None, entries: vec![t] }];
    if matches!(card.stage, Stage::Starting | Stage::Brief) {
        return out;
    }
    push_phase(&mut out, RawKind::Build, t);
    let has_rounds = review.is_some_and(|(rc, r)| {
        let min_seq = card.first_seq.max(rc.first_seq);
        r.rounds.iter().any(|x| is_run_round(x, min_seq))
    });
    if has_rounds || card.triage.is_some() || matches!(card.stage, Stage::Review | Stage::Second | Stage::Merging) {
        push_phase(&mut out, RawKind::Verify(None), t);
    }
    if matches!(card.stage, Stage::Done) || crate::queue::order::in_queue(card) {
        push_phase(&mut out, RawKind::Land, t);
    }
    out
}

// Excludes superseded rounds and rounds carried over from earlier runs of a reopened review card.
fn is_run_round(r: &Round, min_seq: u64) -> bool {
    r.verdict != Some(Verdict::Superseded) && r.asked.map_or(r.verdict.is_none(), |seq| seq >= min_seq)
}

fn round_slots(state: &State, rc: &Card, review: &Review, min_seq: u64) -> Vec<RoundSlot> {
    review
        .rounds
        .iter()
        .filter(|r| is_run_round(r, min_seq))
        .map(|r| RoundSlot { n: r.n, start_ms: round_start(state, rc, r) })
        .collect()
}

pub(super) fn round_start(state: &State, rc: &Card, r: &Round) -> Option<i64> {
    let seq = r.asked?;
    state.messages_in(rc.id.as_str()).find(|m| m.seq == seq).map(|m| m.time)
}

fn splice_rounds(state: &State, card: &Card, phases: Vec<RawStage>, review: Option<(&Card, &Review)>) -> Vec<RawStage> {
    let slots = review.map(|(rc, r)| round_slots(state, rc, r, card.first_seq.max(rc.first_seq))).unwrap_or_default();
    if slots.is_empty() {
        return phases;
    }
    let last_verify = phases.iter().rposition(|p| matches!(p.kind, RawKind::Verify(_)));
    let mut cursor = 0usize;
    let mut out = Vec::with_capacity(phases.len() + slots.len());
    for (i, phase) in phases.into_iter().enumerate() {
        if !matches!(phase.kind, RawKind::Verify(_)) {
            out.push(phase);
            continue;
        }
        let is_last = Some(i) == last_verify;
        let end = cursor
            + slots[cursor..]
                .iter()
                .take_while(|s| match (s.start_ms, phase.end_ms) {
                    (_, None) => true,
                    (Some(t), Some(e)) => t < e,
                    (None, Some(_)) => is_last,
                })
                .count();
        let slice = &slots[cursor..end];
        cursor = end;
        if slice.is_empty() {
            out.push(phase);
        } else {
            out.extend(slots_to_stages(slice, &phase.entries, phase.start_ms, phase.end_ms, |n| {
                RawKind::Verify(Some(n))
            }));
        }
    }
    out
}

// A later round whose request is not yet sent starts at its verification re-entry, where the round before it ends.
fn slots_to_stages(
    slots: &[RoundSlot],
    entries: &[i64],
    first_start: i64,
    last_end: Option<i64>,
    make: impl Fn(u32) -> RawKind,
) -> Vec<RawStage> {
    let mut out = Vec::with_capacity(slots.len());
    let mut prev = first_start;
    for (i, slot) in slots.iter().enumerate() {
        let start_ms = if i == 0 {
            first_start
        } else {
            slot.start_ms.unwrap_or_else(|| entries.iter().copied().rev().find(|&t| t > prev).unwrap_or(prev)).max(prev)
        };
        prev = start_ms;
        out.push(RawStage { kind: make(slot.n), start_ms, end_ms: None, entries: Vec::new() });
    }
    for i in 0..out.len() {
        out[i].end_ms = out.get(i + 1).map(|next| next.start_ms).or(last_end);
    }
    out
}

fn stage_kind(kind: RawKind) -> OverviewStageKind {
    match kind {
        RawKind::Spec => OverviewStageKind::Spec,
        RawKind::Build => OverviewStageKind::Build,
        RawKind::Verify(_) => OverviewStageKind::Verify,
        RawKind::Land => OverviewStageKind::Land,
        RawKind::Report => OverviewStageKind::Report,
        RawKind::Review(_) => OverviewStageKind::Review,
    }
}

fn stage_round(kind: RawKind, review: Option<&Review>) -> Option<RoundView> {
    let (RawKind::Verify(Some(n)) | RawKind::Review(Some(n))) = kind else { return None };
    let review = review?;
    review.rounds.iter().find(|r| r.n == n).map(|r| round_view(review, r))
}

fn stage_label(kind: RawKind) -> String {
    match kind {
        RawKind::Spec => "Spec".to_owned(),
        RawKind::Build => "Build".to_owned(),
        RawKind::Verify(None) => "Verify".to_owned(),
        RawKind::Verify(Some(n)) => format!("Verify · Round {n}"),
        RawKind::Land => "Land".to_owned(),
        RawKind::Report => "Report".to_owned(),
        RawKind::Review(None) => "Review".to_owned(),
        RawKind::Review(Some(n)) => format!("Review · Round {n}"),
    }
}

fn stage_at(kind: RawKind, current: bool, review: Option<&Review>) -> OverviewStageAt {
    if current {
        return OverviewStageAt::Current;
    }
    if let RawKind::Verify(Some(n)) | RawKind::Review(Some(n)) = kind
        && review.and_then(|r| r.rounds.iter().find(|x| x.n == n)).is_some_and(|r| r.verdict == Some(Verdict::Failed))
    {
        return OverviewStageAt::Failed;
    }
    OverviewStageAt::Passed
}

fn stage_summary(
    state: &State,
    card: &Card,
    review: Option<&Review>,
    outside: &Outside,
    kind: RawKind,
    flags: (bool, bool, bool, bool),
) -> String {
    let (current, is_last, is_last_spec, is_last_build) = flags;
    match kind {
        RawKind::Spec if is_last_spec => spec_summary(state, card),
        RawKind::Build if is_last_build => {
            plan_progress(card).map(|(d, t)| format!("{d}/{t} tasks")).unwrap_or_default()
        }
        RawKind::Spec | RawKind::Build => String::new(),
        RawKind::Verify(None) if current => verify_sub(state, card, outside).unwrap_or_default(),
        RawKind::Verify(Some(n)) if current => current_round_summary(state, card, outside, n),
        RawKind::Review(_) if current => review_sub(state, card, outside).unwrap_or_default(),
        RawKind::Verify(Some(n)) | RawKind::Review(Some(n)) => round_summary(review, n),
        RawKind::Land if current => land_sub(state, card).unwrap_or_default(),
        RawKind::Land => ended_word(card, is_last, "landed"),
        RawKind::Report if current => String::new(),
        RawKind::Report | RawKind::Review(None) | RawKind::Verify(None) => ended_word(card, is_last, "done"),
    }
}

fn spec_summary(state: &State, card: &Card) -> String {
    let n = standing(state, card).len();
    if n == 0 { String::new() } else { format!("{n} {}", if n == 1 { "decision" } else { "decisions" }) }
}

/// The card's standing decisions' words, oldest first (fac-691).
pub(super) fn standing(state: &State, card: &Card) -> Vec<String> {
    let board = state.board.get(&card.id);
    let description = board.map_or(&card.description, |b| &b.description);
    let notes = crate::board::notes::apart(state, board, description).0;
    let standing = decisions::standing(&notes);
    standing.into_iter().rev().filter_map(|n| decisions::decision_words(n)).map(str::to_owned).collect()
}

fn current_round_summary(state: &State, card: &Card, outside: &Outside, round: u32) -> String {
    if let Some(t) = card.triage.as_ref().filter(|t| t.round == round) {
        let label = if t.stalemate { "stalemate · " } else { "" };
        return format!("{label}{} to triage", t.items.len());
    }
    verify_sub(state, card, outside).unwrap_or_default()
}

fn ended_word(card: &Card, is_last: bool, done_word: &str) -> String {
    if !is_last {
        return String::new();
    }
    match card.stage {
        Stage::Done => done_word.to_owned(),
        Stage::Closed => "closed".to_owned(),
        Stage::Starting
        | Stage::Brief
        | Stage::Build
        | Stage::Checks
        | Stage::Review
        | Stage::Second
        | Stage::Ready
        | Stage::Merging
        | Stage::Report => String::new(),
    }
}

fn round_summary(review: Option<&Review>, n: u32) -> String {
    let Some(review) = review else { return String::new() };
    let Some(round) = review.rounds.iter().find(|r| r.n == n) else { return String::new() };
    match round.verdict {
        Some(Verdict::Passed) => "passed".to_owned(),
        Some(Verdict::Failed) => failed_summary(review, round),
        Some(Verdict::Superseded) | None => String::new(),
    }
}

fn failed_summary(review: &Review, round: &Round) -> String {
    let (mut fixed, mut declined, mut dropped, mut open) = (0usize, 0usize, 0usize, 0usize);
    for f in round_findings(review, round) {
        match state_of(f) {
            FindingState::Dropped => dropped += 1,
            FindingState::Declined => declined += 1,
            FindingState::Resolved if !f.withdrawn => fixed += 1,
            FindingState::Resolved => {}
            FindingState::Open => open += 1,
        }
    }
    let parts: Vec<String> = [
        (fixed > 0).then(|| format!("{fixed} fixed")),
        (declined > 0).then(|| format!("{declined} declined")),
        (dropped > 0).then(|| format!("{dropped} dropped")),
        (open > 0).then(|| format!("{open} open")),
    ]
    .into_iter()
    .flatten()
    .collect();
    parts.join(", ")
}
