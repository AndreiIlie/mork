// Four-step human pipeline (`Spec`, `Build`, `Verify`, `Land`) and compact pipelines for
// `report` and `review` cards, shared by card headers, board rows, and room rows (fac-680).

use mork_types::ask::AskKind;
use mork_types::card::{Card, Landing, MockStep, Mode, Stage};
use mork_types::config::Config;
use mork_types::reply::Role;
use mork_types::view::{AgentLight, StageAt, StageKind, StageMark};

use super::findings::{current_round, folded_review};
use super::outside::Outside;
use super::status::agent_light;
use super::timeline::{Mark, Point, Totals};
use crate::state::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Spec,
    Build,
    Verify,
    Land,
    Report,
    Review,
    Done,
}

impl Step {
    pub(super) fn kind(self) -> StageKind {
        match self {
            Self::Spec => StageKind::Spec,
            Self::Build => StageKind::Build,
            Self::Verify => StageKind::Verify,
            Self::Land => StageKind::Land,
            Self::Report => StageKind::Report,
            Self::Review => StageKind::Review,
            Self::Done => StageKind::Done,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Spec => "Spec",
            Self::Build => "Build",
            Self::Verify => "Verify",
            Self::Land => "Land",
            Self::Report => "Report",
            Self::Review => "Review",
            Self::Done => "Done",
        }
    }

    fn timeline_keys(self) -> &'static [&'static str] {
        match self {
            Self::Spec => &["starting", "brief", "plan", "mock"],
            Self::Build => &["build"],
            Self::Verify => &["checks", "review", "second", "ready", "verify"],
            Self::Land => &["land"],
            Self::Report => &["starting", "report"],
            Self::Review => &["starting", "checks", "review", "verify", "land"],
            Self::Done => &["done"],
        }
    }
}

pub(super) fn steps_for(mode: Mode) -> &'static [Step] {
    match mode {
        Mode::Report => &[Step::Report, Step::Done],
        Mode::Review => &[Step::Review, Step::Done],
        Mode::Full | Mode::Light => &[Step::Spec, Step::Build, Step::Verify, Step::Land],
    }
}

pub(super) fn stage_times(mode: Mode, totals: &Totals) -> Vec<(String, i64)> {
    steps_for(mode)
        .iter()
        .filter_map(|&step| {
            let sum: i64 = step.timeline_keys().iter().filter_map(|k| totals.stages.get(*k)).sum();
            (sum > 0).then(|| (step.label().to_owned(), sum))
        })
        .collect()
}

pub(super) fn build_pipeline(
    state: &State,
    card: &Card,
    points: &[Point],
    outside: &Outside,
    config: &Config,
) -> Vec<StageMark> {
    let own = steps_for(card.mode);
    let current = current_step(state, card).and_then(|s| own.iter().position(|&x| x == s));
    let ended = !card.stage.is_live();
    let reached = if card.stage == Stage::Done { own.len() } else { closed_reached(card.mode, own, points) };
    own.iter()
        .enumerate()
        .map(|(i, &step)| {
            let at = match current {
                Some(c) if i == c => StageAt::Current,
                Some(c) if i < c => StageAt::Passed,
                None if ended && i < reached => StageAt::Passed,
                _ => StageAt::ToCome,
            };
            let count = (at != StageAt::ToCome).then(|| step_count(state, card, step)).flatten();
            let sub = (at == StageAt::Current).then(|| step_sub(state, card, step, outside, config)).flatten();
            StageMark {
                kind: step.kind(),
                label: step.label().to_owned(),
                at,
                runs: entries(card.mode, points, step).max(1),
                count,
                sub,
            }
        })
        .collect()
}

/// Short step word for sidebar, board rows, and room rows (`Spec · Mock`, `Build 11/12`, `Verify · Round 2`, `Land · #2`).
pub(super) fn short_word(state: &State, card: &Card) -> String {
    if card.stage == Stage::Done {
        return "Done".to_owned();
    }
    if card.stage == Stage::Closed {
        return "Closed".to_owned();
    }
    match current_step(state, card) {
        Some(Step::Spec) if mock_active(card) => "Spec · Mock".to_owned(),
        Some(Step::Spec) => "Spec".to_owned(),
        Some(Step::Build) => match plan_progress(card) {
            Some((done, total)) => format!("Build {done}/{total}"),
            None => "Build".to_owned(),
        },
        Some(Step::Verify) => match current_round(state, card) {
            Some(n) => format!("Verify · Round {n}"),
            None => "Verify".to_owned(),
        },
        Some(Step::Land) => land_short(state, card),
        Some(Step::Report) => "Report".to_owned(),
        Some(Step::Review) => match current_round(state, card) {
            Some(n) => format!("Review · Round {n}"),
            None => "Review".to_owned(),
        },
        Some(Step::Done) => "Done".to_owned(),
        None => card.stage.as_str().to_owned(),
    }
}

/// Whole-run progress, 0 to 100, with equal shares per step and Build/Verify filled by their own progress.
pub(super) fn progress(state: &State, card: &Card, outside: &Outside) -> u8 {
    let own = steps_for(card.mode);
    let Some(step) = current_step(state, card) else { return 0 };
    let Some(c) = own.iter().position(|&s| s == step) else { return 0 };
    let (num, den) = step_within(state, card, step, outside);
    let idx = u128::try_from(c).unwrap_or(0);
    let total_steps = u128::try_from(own.len()).unwrap_or(1).max(1);
    let pct = ((idx * den + num) * 100) / (total_steps * den);
    u8::try_from(pct.min(100)).unwrap_or(100)
}

fn step_within(state: &State, card: &Card, step: Step, outside: &Outside) -> (u128, u128) {
    match step {
        Step::Build => plan_progress(card)
            .map_or((0, 1), |(d, t)| (u128::try_from(d.min(t)).unwrap_or(0), u128::try_from(t).unwrap_or(1).max(1))),
        Step::Verify => verify_within(state, card, outside),
        Step::Spec | Step::Land | Step::Report | Step::Review | Step::Done => (0, 1),
    }
}

fn verify_within(state: &State, card: &Card, outside: &Outside) -> (u128, u128) {
    match super::live::suite_state(state, card, outside) {
        super::live::SuiteState::Ended => (1, 1),
        super::live::SuiteState::Running(Some(p)) if p.total.is_some_and(|t| t > 0) => {
            let t = p.total.unwrap_or(1);
            (u128::from(p.passed.saturating_add(p.failed).min(t)), u128::from(t))
        }
        super::live::SuiteState::NotStarted | super::live::SuiteState::Running(_) => (0, 1),
    }
}

fn current_step(state: &State, card: &Card) -> Option<Step> {
    if !card.stage.is_live() {
        return None;
    }
    match card.mode {
        Mode::Report => Some(Step::Report),
        Mode::Review => Some(Step::Review),
        Mode::Full | Mode::Light => Some(full_step(state, card)),
    }
}

fn full_step(state: &State, card: &Card) -> Step {
    match card.stage {
        Stage::Starting | Stage::Brief => Step::Spec,
        Stage::Build if crate::lifecycle::mock::open(card) => Step::Spec,
        Stage::Build if card.triage.is_some() => Step::Verify,
        Stage::Build | Stage::Report => Step::Build,
        Stage::Checks | Stage::Review | Stage::Second => Step::Verify,
        Stage::Ready => {
            let merge_ask =
                state.asks.values().any(|a| a.open && a.kind == AskKind::Merge && a.card.as_ref() == Some(&card.id));
            if merge_ask { Step::Land } else { Step::Verify }
        }
        Stage::Merging if crate::queue::order::in_queue(card) => Step::Land,
        Stage::Merging => Step::Verify,
        Stage::Done | Stage::Closed => Step::Land,
    }
}

fn mock_active(card: &Card) -> bool {
    card.mock.is_some_and(|m| matches!(m.step, MockStep::Baseline | MockStep::Open))
}

fn step_count(state: &State, card: &Card, step: Step) -> Option<String> {
    match step {
        Step::Build => plan_progress(card).map(|(d, t)| format!("{d}/{t}")),
        Step::Verify | Step::Review => current_round(state, card).map(|n| format!("Round {n}")),
        Step::Spec | Step::Land | Step::Report | Step::Done => None,
    }
}

fn step_sub(state: &State, card: &Card, step: Step, outside: &Outside, config: &Config) -> Option<String> {
    match step {
        Step::Spec => mock_active(card).then(|| "Mock".to_owned()),
        Step::Verify => verify_sub(state, card, outside),
        Step::Land => land_sub(state, card, config),
        Step::Review => review_sub(state, card, outside),
        Step::Build | Step::Report | Step::Done => None,
    }
}

pub(super) fn verify_sub(state: &State, card: &Card, outside: &Outside) -> Option<String> {
    if let Some(t) = &card.triage {
        let k = t.items.len();
        let word = if k == 1 { "finding waits" } else { "findings wait" };
        let lead = if t.stalemate { "stalemate" } else { "failed" };
        // A triage given to the orchestrator waits for it, not for the operator (fac-691).
        let given = state.asks.get(&t.ask).is_some_and(|a| a.open && a.given);
        let whom = if given { "orchestrator" } else { "you" };
        return Some(format!("{lead} · {k} {word} for {whom}"));
    }
    if is_held(state, card) {
        return Some("held".to_owned());
    }
    if matches!(card.stage, Stage::Checks | Stage::Ready) {
        return Some(if card.gates.slot_wait.is_some() { "waiting for a slot" } else { "checks" }.to_owned());
    }
    let working = folded_review(state, card).map_or(0, |rc| reviewer_counts(state, rc, outside).0);
    let rev = (working > 0).then(|| {
        let word = if working == 1 { "reviewer" } else { "reviewers" };
        format!("{working} {word} working")
    });
    match card.landing.as_ref() {
        Some(Landing::Test(k)) if !k.checked => {
            Some(if card.gates.slot_wait.is_some() { "waiting for a slot" } else { "checks" }.to_owned())
        }
        Some(Landing::Test(_)) => {
            let suite = if card.gates.slot_wait.is_some() { "suite waiting for a slot" } else { "suite running" };
            Some(join_parts(rev, Some(suite.to_owned())))
        }
        Some(Landing::Review(k)) => {
            let suite = if k.failure.is_some() { "suite failed" } else { "suite passed" };
            Some(join_parts(rev, Some(suite.to_owned())))
        }
        Some(Landing::Queued { .. } | Landing::Fetch { .. } | Landing::Prepare(_)) => Some("checks".to_owned()),
        _ => rev,
    }
}

pub(super) fn is_held(state: &State, card: &Card) -> bool {
    card.alarm
        || crate::queue::order::is_train(card.failed_job.as_ref())
        || state.asks.values().any(|a| {
            a.open
                && a.card.as_ref() == Some(&card.id)
                && (matches!(a.kind, AskKind::Alarm | AskKind::Checks) || is_held_landing_ask(state, a))
        })
}

fn is_held_landing_ask(state: &State, a: &mork_types::ask::Ask) -> bool {
    a.kind == AskKind::Held
        && a.action
            .as_ref()
            .and_then(|id| state.actions.get(id))
            .is_some_and(|act| crate::queue::holds::landing_action(&act.what))
}

fn land_held(state: &State, card: &Card) -> bool {
    card.alarm
        || state.asks.values().any(|a| {
            a.open && a.card.as_ref() == Some(&card.id) && (a.kind == AskKind::Alarm || is_held_landing_ask(state, a))
        })
}

pub(super) fn land_sub(state: &State, card: &Card, config: &Config) -> Option<String> {
    if land_held(state, card)
        && matches!(card.landing, Some(Landing::Published { .. } | Landing::Approval { .. } | Landing::Merge { .. }))
    {
        return Some(card.pr.map_or_else(|| "held".to_owned(), |n| format!("PR #{n} held")));
    }
    match card.landing.as_ref() {
        Some(Landing::Published { .. }) => {
            Some(card.pr.map_or_else(|| "published".to_owned(), |n| format!("PR #{n} published")))
        }
        Some(Landing::Approval { .. }) => {
            let status = if !config.projects.get(&card.project).is_some_and(|p| p.forge.merges_on_approval()) {
                "waits for your merge"
            } else if card.gates.approved.is_some() {
                "merging"
            } else {
                "waits for your approval"
            };
            Some(card.pr.map_or_else(|| status.to_owned(), |n| format!("PR #{n} {status}")))
        }
        Some(Landing::Merge { .. }) => {
            Some(card.pr.map_or_else(|| "merging".to_owned(), |n| format!("PR #{n} merging")))
        }
        Some(Landing::HandMerged) => Some("merged outside Mork".to_owned()),
        _ => queue_position(state, card)
            .map(|pos| if pos == 0 { "in slot".to_owned() } else { format!("#{pos} in line") })
            .or_else(|| {
                let merge_ask = state
                    .asks
                    .values()
                    .any(|a| a.open && a.kind == AskKind::Merge && a.card.as_ref() == Some(&card.id));
                merge_ask.then(|| "waits for your merge".to_owned())
            }),
    }
}

fn land_short(state: &State, card: &Card) -> String {
    if matches!(card.landing, Some(Landing::Published { .. } | Landing::Approval { .. } | Landing::Merge { .. }))
        && let Some(pr) = card.pr
    {
        return format!("Land · PR #{pr}");
    }
    match queue_position(state, card) {
        Some(0) => "Land · in slot".to_owned(),
        Some(n) => format!("Land · #{n}"),
        None => "Land".to_owned(),
    }
}

pub(super) fn review_sub(state: &State, card: &Card, outside: &Outside) -> Option<String> {
    let pr = card.pr.map(|n| format!("PR #{n}"));
    let (working, total) = reviewer_counts(state, card, outside);
    let rev = if working > 0 && total > 0 {
        let word = if total == 1 { "reviewer" } else { "reviewers" };
        Some(format!("{working} of {total} {word} working"))
    } else {
        None
    };
    let text = join_parts(pr, rev);
    (!text.is_empty()).then_some(text)
}

fn join_parts(a: Option<String>, b: Option<String>) -> String {
    [a, b].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

pub(super) fn plan_progress(card: &Card) -> Option<(usize, usize)> {
    let plan = card.plan.as_ref()?;
    let total: usize = plan.deliverables.iter().map(|d| d.tasks.len()).sum();
    if total == 0 {
        return None;
    }
    let done: usize = plan
        .deliverables
        .iter()
        .enumerate()
        .map(|(i, d)| match i.cmp(&plan.current) {
            std::cmp::Ordering::Less => d.tasks.len(),
            std::cmp::Ordering::Equal => d.current.min(d.tasks.len()),
            std::cmp::Ordering::Greater => 0,
        })
        .sum();
    Some((done, total))
}

fn reviewer_counts(state: &State, rc: &Card, outside: &Outside) -> (usize, usize) {
    let reviewers = state.agents.values().filter(|a| {
        a.role == Role::Reviewer && crate::messages::places::card_of(state, a.place.as_str()).as_ref() == Some(&rc.id)
    });
    let mut working = 0usize;
    let mut total = 0usize;
    for a in reviewers {
        total += 1;
        if agent_light(state, &a.key, outside) == AgentLight::Working {
            working += 1;
        }
    }
    (working, total)
}

fn queue_position(state: &State, card: &Card) -> Option<usize> {
    super::boards::queue_cards(state, &card.project).iter().position(|c| c.id == card.id)
}

fn step_sequence(mode: Mode, points: &[Point]) -> Vec<Step> {
    let mut out = Vec::new();
    let mut stage: Option<Stage> = None;
    for p in points {
        let next = match &p.mark {
            Mark::Stage(s) => {
                if stage == Some(*s) {
                    continue;
                }
                stage = Some(*s);
                stage_step(mode, *s)
            }
            Mark::Verified if matches!(mode, Mode::Full | Mode::Light) => Some(Step::Land),
            Mark::Reverify if matches!(mode, Mode::Full | Mode::Light) => Some(Step::Verify),
            Mark::Verified
            | Mark::Reverify
            | Mark::Waiting
            | Mark::Answered
            | Mark::Paused
            | Mark::Resumed
            | Mark::Planning
            | Mark::Planned
            | Mark::Mocking
            | Mark::Mocked
            | Mark::End => None,
        };
        if let Some(step) = next
            && out.last() != Some(&step)
        {
            out.push(step);
        }
    }
    out
}

fn stage_step(mode: Mode, stage: Stage) -> Option<Step> {
    match mode {
        Mode::Report => Some(if stage == Stage::Done { Step::Done } else { Step::Report }),
        Mode::Review => Some(if stage == Stage::Done { Step::Done } else { Step::Review }),
        Mode::Full | Mode::Light => match stage {
            Stage::Starting | Stage::Brief => Some(Step::Spec),
            Stage::Build => Some(Step::Build),
            Stage::Checks | Stage::Review | Stage::Second | Stage::Ready | Stage::Merging => Some(Step::Verify),
            Stage::Report | Stage::Done | Stage::Closed => None,
        },
    }
}

fn closed_reached(mode: Mode, own: &[Step], points: &[Point]) -> usize {
    step_sequence(mode, points).last().and_then(|s| own.iter().position(|o| o == s)).map_or(0, |i| i + 1)
}

fn entries(mode: Mode, points: &[Point], step: Step) -> u32 {
    step_sequence(mode, points).into_iter().filter(|&s| s == step).count() as u32
}
