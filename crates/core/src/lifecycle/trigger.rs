use mork_types::ProjectId;
use mork_types::config::{Cli, Seat, SeatPhase, Standing, agy_bucket};
use mork_types::lookback::{Baseline, Counts, PassConfig, Read, Trigger};
use mork_types::note::NoteWhat;
use mork_types::usage::{Limit, length_ms, short_name};

use super::lookback;
use crate::agents::{turns, usage::pace};
use crate::decide::Ctx;
use crate::post::Poster;
use crate::state::{Change, State};

pub(crate) fn due(p: &mut Poster<'_>, account: Option<&str>, ctx: Ctx<'_>) {
    let mut auto_available = None;
    for project in ctx.config.ordered_projects() {
        let passes = &ctx.config.projects[project].passes;
        if passes.is_empty() || lookback::live(p.state, project).is_some() {
            continue;
        }
        let mut start = None;
        for (&read, pass) in passes.iter().filter(|(_, pass)| pass.mode != Trigger::Manual) {
            let base = baseline(p, project, read);
            let Some(met) = threshold(p, &base, pass, ctx) else { continue };
            if pass.mode == Trigger::Auto {
                let available =
                    auto_available.get_or_insert_with(|| {
                        !p.state.cards.values().any(|c| {
                            c.stage.is_live() && c.lookback.as_ref().is_some_and(|r| r.trigger == Trigger::Auto)
                        }) && !turns::real_running(p.state)
                            && !p.all_agents().any(|a| turns::real_owed(p.state, a))
                    });
                if !*available || !quota(p.state, account, pass, ctx) {
                    continue;
                }
            }
            start.get_or_insert((read, pass.mode, met));
        }
        if let Some((read, mode, met)) = start {
            lookback::begin(p, project, read, mode, Some(met), ctx);
            if mode == Trigger::Auto {
                auto_available = Some(false);
            }
        }
    }
}

fn baseline(p: &mut Poster<'_>, project: &ProjectId, read: Read) -> Baseline {
    if let Some(b) = p.state.baselines.get(&(project.clone(), read)) {
        return b.clone();
    }
    let cursor = p.state.lookbacks.get(project).and_then(|reads| reads.get(&read));
    let base = Baseline {
        project: project.clone(),
        read,
        at_ms: cursor.map_or(p.now, |c| c.ended_ms),
        counts: lookback::counts(p.state, project),
    };
    p.other.push(Change::PutBaseline(base.clone()));
    base
}

fn threshold(p: &Poster<'_>, base: &Baseline, pass: &PassConfig, ctx: Ctx<'_>) -> Option<&'static str> {
    let Counts { landings, memory_edits, .. } = lookback::counts(p.state, &base.project);
    let days = p.now.saturating_sub(base.at_ms).max(0).unsigned_abs() / 86_400_000;
    for (name, count, limit) in [
        ("landings", landings.saturating_sub(base.counts.landings), pass.landings),
        ("edits", memory_edits.saturating_sub(base.counts.memory_edits), pass.edits),
        ("days", days, pass.days),
    ] {
        if limit.is_some_and(|n| count >= n) {
            return Some(name);
        }
    }
    let needed = usize::try_from(pass.notes?).ok()?;
    let notes = p.state.notes.range((std::ops::Bound::Excluded(base.counts.note), std::ops::Bound::Unbounded));
    let new = notes.filter(|(_, n)| {
        matches!(n.what, NoteWhat::Lesson { .. })
            && crate::memory::under(n, ctx.config.mork.project.as_ref()) == Some(&base.project)
    });
    (new.take(needed).count() == needed).then_some("notes")
}

fn quota(state: &State, account: Option<&str>, pass: &PassConfig, ctx: Ctx<'_>) -> bool {
    let at = Standing { phase: SeatPhase::Report, review: None };
    let Some(seat) = ctx.config.seat_in("foreman", at) else { return false };
    if seat.cli == Cli::Claude && account.is_none() {
        return false;
    }
    let bucket = if seat.cli == Cli::Agy { agy_bucket(&seat.model) } else { "account" };
    let applicable = |l: &&Limit| {
        l.cli == seat.cli && (seat.cli != Cli::Claude || l.account.as_ref().map(|a| a.id.as_str()) == account)
    };
    let limits = state.limits.values().filter(applicable);
    let share = 100_u32.saturating_sub(pass.spare.unwrap_or(30));
    let window = |name: &str, optional| {
        let limit = limits.clone().find(|l| l.bucket == bucket && short_name(&l.window) == name);
        allows(limit, optional, share, ctx.now)
    };
    window("week", false)
        && window("5h", true)
        && (seat.cli != Cli::Claude || !seat.model.to_ascii_lowercase().contains("fable") || window("fable", false))
        && limits.filter(|l| model_pool(&seat, l)).all(|l| allows(Some(l), false, share, ctx.now))
}

fn model_pool(seat: &Seat, limit: &Limit) -> bool {
    match seat.cli {
        Cli::Codex => limit.bucket == seat.model,
        Cli::Claude => limit.window.strip_prefix("seven_day_").is_some_and(|model| seat.model.contains(model)),
        Cli::Agy => false,
    }
}

fn allows(limit: Option<&Limit>, optional: bool, share: u32, now: i64) -> bool {
    let Some(l) = limit else { return optional };
    let reset = l.resets_at.map(|r| if r < 100_000_000_000 { r.saturating_mul(1000) } else { r });
    if reset.is_some_and(|r| r <= now) {
        return optional;
    }
    if l.reached {
        return false;
    }
    if optional && l.percent.is_none() {
        return true;
    }
    pace(l.percent, reset, length_ms(&l.window), l.seen_ms).is_some_and(|(_, end)| end <= u64::from(share))
}
