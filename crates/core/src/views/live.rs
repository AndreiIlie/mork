// The Overview's live rows under its current stage (fac-691): at Verify, its checks, its suite and its review round
// with each working reviewer; at any other stage, each working seat of the card.

use factory_types::card::{Card, Landing, Mode};
use factory_types::reply::Role;
use factory_types::review::{Review, Round, Verdict};
use factory_types::state::{Agent, AgentStatus, Job};
use factory_types::view::{LiveRow, SuiteProgress};
use factory_types::work::Purpose;

use super::outside::Outside;
use crate::messages::places;
use crate::state::State;

const REVIEW_FOREMAN: &str = "review foreman";

/// Verify's rows: its candidate's checks and suite, then round `round`'s row and its working reviewers.
pub(super) fn verify(
    state: &State,
    card: &Card,
    review: Option<(&Card, &Review)>,
    round: Option<u32>,
    outside: &Outside,
) -> Vec<LiveRow> {
    let mut out = run_rows(state, card, outside);
    let Some((rc, r)) = review else { return out };
    let Some(x) = round.and_then(|n| r.rounds.iter().find(|x| x.n == n)) else { return out };
    let reviewers = seats(state, rc, outside, |a| a.role == Role::Reviewer);
    let own = format!("{}/foreman", rc.id);
    let foreman = seats(state, rc, outside, |a| a.key.as_str() == own).into_iter().next();
    out.push(round_row(state, card, (rc, x), foreman, reviewers.len()));
    out.extend(reviewers.into_iter().map(|row| LiveRow { under: true, ..row }));
    out
}

/// Each working seat of the card, with its current step and how long its turn has run.
pub(super) fn working(state: &State, card: &Card, outside: &Outside) -> Vec<LiveRow> {
    seats(state, card, outside, |_| true).into_iter().map(|row| LiveRow { topic: None, ..row }).collect()
}

// Where its candidate's run stands: a failure before its checks passed is theirs, once checked its suite's.
struct Run {
    checked: bool,
    failed: bool,
    suite_ended: bool,
    held: bool,
}

/// Where a card's verification suite stands, shared by Card Room rows and Board progress.
pub(super) enum SuiteState {
    NotStarted,
    Running(Option<SuiteProgress>),
    Ended,
}

/// The current candidate's or triaged round's suite state.
pub(super) fn suite_state(state: &State, card: &Card, outside: &Outside) -> SuiteState {
    let Some((run, jobs)) = candidate_run(state, card) else { return SuiteState::NotStarted };
    if run.suite_ended {
        return SuiteState::Ended;
    }
    if !run.checked {
        return SuiteState::NotStarted;
    }
    let phases: Vec<&&Job> = jobs.iter().filter(|j| is_suite(j)).collect();
    SuiteState::Running(counted(&phases, outside))
}

// A failed round waiting for triage has dropped its candidate, whose checks passed and whose suite ended: its jobs
// are those of the round's commit (fac-691).
fn candidate_run<'a>(state: &'a State, card: &Card) -> Option<(Run, Vec<&'a Job>)> {
    let of_card = |j: &&Job| matches!(&j.purpose, Some(Purpose::TrainRun { card: c, .. }) if *c == card.id);
    let clone_of = |j: &Job| j.purpose.as_ref().and_then(crate::queue::order::clone_of);
    if let Some(k) = crate::queue::order::candidate(card) {
        let jobs: Vec<&Job> = state.jobs.values().filter(of_card).filter(|j| clone_of(j) == Some(k.clone)).collect();
        let suite_ended = matches!(card.landing, Some(Landing::Review(_) | Landing::Passed(_)));
        let held = super::pipeline::is_held(state, card);
        return Some((Run { checked: k.checked, failed: k.failure.is_some(), suite_ended, held }, jobs));
    }
    let t = card.triage.as_ref().filter(|t| t.from_verification)?;
    let on_commit = |j: &&Job| matches!(&j.purpose, Some(Purpose::TrainRun { commit, .. }) if *commit == t.commit);
    let jobs: Vec<&Job> = state.jobs.values().filter(of_card).filter(on_commit).collect();
    let latest = jobs.iter().filter_map(|j| clone_of(j)).max();
    let jobs: Vec<&Job> = jobs.into_iter().filter(|j| clone_of(j) == latest).collect();
    Some((Run { checked: true, failed: t.suite_failure.is_some(), suite_ended: true, held: false }, jobs))
}

fn run_rows(state: &State, card: &Card, outside: &Outside) -> Vec<LiveRow> {
    let Some((run, jobs)) = candidate_run(state, card) else { return Vec::new() };
    rows(card, &run, &jobs, outside)
}

fn rows(card: &Card, run: &Run, jobs: &[&Job], outside: &Outside) -> Vec<LiveRow> {
    let mut out = vec![checks(card, run, jobs)];
    out.extend(suite(card, run, jobs, outside));
    out
}

fn is_checks(j: &Job) -> bool {
    matches!(j.purpose, Some(Purpose::TrainRun { checks: true, test: None, .. }))
}

fn is_suite(j: &Job) -> bool {
    matches!(j.purpose, Some(Purpose::TrainRun { test: Some(_), .. }))
}

fn slot_wait(card: &Card, suite: bool) -> bool {
    matches!(&card.gates.slot_wait, Some(Purpose::TrainRun { test, .. }) if test.is_some() == suite)
}

fn checks(card: &Card, run: &Run, jobs: &[&Job]) -> LiveRow {
    let job = jobs.iter().filter(|j| is_checks(j)).max_by_key(|j| j.spec.created_ms);
    let running = job.is_some_and(|j| j.settled.is_none());
    let words = if run.checked {
        "passed"
    } else if run.failed {
        "failed"
    } else if run.held {
        "held"
    } else if slot_wait(card, false) {
        "waiting for a slot"
    } else if running {
        "running"
    } else {
        "waiting"
    };
    let working = !run.checked && !run.failed && !run.held;
    step("Checks", words, working, job.map(|j| j.spec.created_ms), job.and_then(|j| j.settled_ms))
}

// Its walled and unwalled phases are one suite: from the first one's start to the last one's end.
fn suite(card: &Card, run: &Run, jobs: &[&Job], outside: &Outside) -> Option<LiveRow> {
    if !run.checked {
        return None;
    }
    let phases: Vec<&&Job> = jobs.iter().filter(|j| is_suite(j)).collect();
    let words = match (run.suite_ended, run.failed) {
        (true, true) => "failed",
        (true, false) => "passed",
        (false, _) if run.held => "held",
        (false, _) if slot_wait(card, true) => "waiting for a slot",
        (false, _) => "running",
    };
    let start = phases.iter().map(|j| j.spec.created_ms).min();
    let end = if phases.iter().any(|j| j.settled_ms.is_none()) {
        None
    } else {
        phases.iter().filter_map(|j| j.settled_ms).max()
    };
    let progress = (words == "running").then(|| counted(&phases, outside)).flatten();
    Some(LiveRow { progress, ..step("Suite", words, !run.suite_ended && !run.held, start, end) })
}

// The phases' counts are one suite's (fac-697), each phase's newest attempt alone as `resume` runs it
// again: its total only when each phase that counted gave one, the test that ended last the newest's.
fn counted(phases: &[&&Job], outside: &Outside) -> Option<SuiteProgress> {
    let unwalled = |j: &Job| matches!(j.purpose, Some(Purpose::TrainRun { unwalled: true, .. }));
    let newest = [false, true]
        .map(|u| phases.iter().filter(|j| unwalled(j) == u).max_by_key(|j| (j.spec.created_ms, j.id.clone())));
    let mut seen: Vec<(i64, &SuiteProgress)> =
        newest.into_iter().flatten().filter_map(|j| Some((j.spec.created_ms, outside.suites.get(&j.id)?))).collect();
    seen.sort_by_key(|(at, _)| *at);
    let sum = |n: fn(&SuiteProgress) -> u32| seen.iter().fold(0u32, |all, (_, p)| all.saturating_add(n(p)));
    (!seen.is_empty()).then(|| SuiteProgress {
        passed: sum(|p| p.passed),
        failed: sum(|p| p.failed),
        total: seen.iter().map(|(_, p)| p.total).sum(),
        files: seen.iter().any(|(_, p)| p.files),
        latest: seen.iter().rev().find_map(|(_, p)| p.latest.clone()),
    })
}

// The round's row is its review foreman's (fac-696): working only while its own turn runs, its reviewers under it.
fn round_row(
    state: &State,
    card: &Card,
    (rc, round): (&Card, &Round),
    own: Option<LiveRow>,
    reviewers: usize,
) -> LiveRow {
    let words = match round.verdict {
        None => match &own {
            Some(turn) => turn.words.clone(),
            None if reviewers > 0 => {
                format!("waiting on {reviewers} {}", if reviewers == 1 { "reviewer" } else { "reviewers" })
            }
            None => "waiting".to_owned(),
        },
        Some(Verdict::Passed) => "passed".to_owned(),
        Some(Verdict::Failed) => match card.triage.as_ref().filter(|t| t.round == round.n) {
            Some(t) if t.stalemate => format!("stalemate · {} to triage", t.items.len()),
            Some(t) => format!("failed · {} to triage", t.items.len()),
            None => "failed".to_owned(),
        },
        Some(Verdict::Superseded) => "superseded".to_owned(),
    };
    let start = super::overview::round_start(state, rc, round);
    let end = round.verdict.and_then(|_| verdict_ms(state, rc, round, start));
    LiveRow { seat: true, ..step(REVIEW_FOREMAN, &words, own.is_some(), start, end) }
}

// The verdict's time: the review card foreman's latest turn asked about the round's commit, settled.
fn verdict_ms(state: &State, rc: &Card, round: &Round, start: Option<i64>) -> Option<i64> {
    let foreman = format!("{}/foreman", rc.id);
    state
        .jobs
        .values()
        .filter(|j| j.owner.as_str() == foreman && j.head.as_ref() == Some(&round.commit))
        .filter(|j| start.is_none_or(|s| j.spec.created_ms >= s))
        .filter_map(|j| j.settled_ms)
        .max()
}

fn step(name: &str, words: &str, working: bool, start_ms: Option<i64>, end_ms: Option<i64>) -> LiveRow {
    LiveRow {
        name: name.to_owned(),
        topic: None,
        words: words.to_owned(),
        working,
        under: false,
        seat: false,
        start_ms,
        end_ms,
        progress: None,
    }
}

// A seat in a turn, oldest turn first: its words are what its stream shows it doing now.
fn seats(state: &State, card: &Card, outside: &Outside, keep: impl Fn(&Agent) -> bool) -> Vec<LiveRow> {
    let mut rows: Vec<LiveRow> = state
        .agents
        .values()
        .filter(|a| keep(a) && places::card_of(state, a.place.as_str()).as_ref() == Some(&card.id))
        .filter_map(|a| {
            let AgentStatus::Working { job } = &a.status else { return None };
            let words = if outside.slot_waits.contains(&a.key) {
                "waiting for a slot".to_owned()
            } else {
                let seen = outside.agents.get(&a.key);
                seen.and_then(|s| s.doing.clone().or_else(|| s.steps.last().cloned())).unwrap_or_default()
            };
            Some(LiveRow {
                name: seat_name(card, a),
                topic: a.topic.clone(),
                words,
                working: true,
                under: false,
                seat: true,
                start_ms: state.jobs.get(job).map(|j| j.spec.created_ms),
                end_ms: None,
                progress: None,
            })
        })
        .collect();
    rows.sort_by_key(|r| r.start_ms);
    rows
}

fn seat_name(card: &Card, a: &Agent) -> String {
    if card.mode == Mode::Review && a.key.as_str() == format!("{}/foreman", card.id) {
        return REVIEW_FOREMAN.to_owned();
    }
    a.key.as_str().split('/').skip(1).collect::<Vec<_>>().join("/")
}
