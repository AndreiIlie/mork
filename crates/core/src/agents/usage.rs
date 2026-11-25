//! Usage limits in the state (1.4.7.2): the latest value of each window, changed (and
//! journaled) when a read differs; fresh reads also retain when the window was last seen.

use factory_types::JobId;
use factory_types::config::{Cli, agy_bucket};
use factory_types::state::Agent;
use factory_types::usage::Limit;
use serde_json::json;

use crate::decide::{Ctx, Decision, SessionSeen};
use crate::post::Poster;
use crate::state::{Change, State};

/// A pool's whole percentages of window elapsed and projected use at reset, at the reading's time.
/// None without a share, a known window, or a reading strictly inside that window.
pub fn pace(percent: Option<u32>, reset_ms: Option<i64>, length_ms: Option<i64>, seen_ms: i64) -> Option<(u32, u64)> {
    let length = length_ms?;
    let start = reset_ms?.checked_sub(length)?;
    let elapsed = u128::try_from(seen_ms.checked_sub(start)?).ok()?;
    let length = u128::try_from(length).ok()?;
    if elapsed == 0 || elapsed >= length {
        return None;
    }
    let gone = (elapsed * 100 + length / 2) / length;
    let end = (u128::from(percent?) * length + elapsed / 2) / elapsed;
    Some((u32::try_from(gone).ok()?, u64::try_from(end).ok()?))
}

/// A read of some windows: each that differs is put, with one journal line and a stream's position (1.4.7.2). A whole
/// read (Codex's) lifts a reached window it doesn't list; one newly reached is told to Andrei (fac-119).
pub fn seen(state: &State, limits: &[Limit], at: Option<&(JobId, u64)>, whole: Option<Cli>, ctx: Ctx<'_>) -> Decision {
    let mut d = Decision::default();
    let fresh: Vec<Limit> = limits
        .iter()
        .filter(|l| state.limits.get(&l.key()).is_none_or(|old| l.seen_ms >= old.seen_ms))
        .cloned()
        .collect();
    let limits = fresh.as_slice();
    let listed: Vec<String> = limits.iter().map(Limit::key).collect();
    let lifted: Vec<Limit> = state
        .limits
        .values()
        .filter(|l| Some(l.cli) == whole && l.reached && !listed.contains(&l.key()))
        .map(|l| Limit { reached: false, ..l.clone() })
        .collect();
    let changed: Vec<&Limit> = limits
        .iter()
        .filter(|l| state.limits.get(&l.key()).is_none_or(|old| !old.same_as(l)))
        .chain(lifted.iter())
        .collect();
    let moved = at.and_then(|(id, to)| state.jobs.get(id).filter(|j| j.limits_at < *to).map(|j| (j, *to)));
    let what: Vec<_> = changed
        .iter()
        .map(|l| json!({"cli": l.cli.as_str(), "account": l.account, "bucket": l.bucket, "window": l.window, "percent": l.percent, "resets_at": l.resets_at, "reached": l.reached}))
        .collect();
    let read = moved.as_ref().map(|(j, to)| json!({"job": j.id.as_str(), "at": to}));
    let refreshed: Vec<_> =
        limits.iter().chain(lifted.iter()).filter(|l| state.limits.get(&l.key()) != Some(l)).collect();
    if !refreshed.is_empty() || moved.is_some() {
        d.journal("limits", None, json!({"changed": what, "read": read}));
    }
    let reached: Vec<&Limit> = changed
        .iter()
        .copied()
        .filter(|l| l.reached && state.limits.get(&l.key()).is_none_or(|o| !o.reached))
        .collect();
    let mut p = Poster::for_ctx(state, ctx);
    if !reached.is_empty() {
        tell_andrei(&mut p, &reached);
    }
    for l in refreshed {
        d.changes.push(Change::PutLimit(l.clone()));
    }
    p.finish(&mut d);
    if let Some((j, to)) = moved {
        let mut j = j.clone();
        j.limits_at = to;
        d.changes.push(Change::PutJob(Box::new(j)));
    }
    d
}

// Every card waits on a reached limit: Andrei hears of it in main, once per window reached.
fn tell_andrei(p: &mut Poster<'_>, reached: &[&Limit]) {
    let windows: Vec<String> = reached
        .iter()
        .map(|l| {
            let reset = l.resets_at.map_or_else(
                || "its reset unknown".to_owned(),
                |s| {
                    let (y, mo, day, h, mi, _) = crate::state::names::civil_from_ms(s.saturating_mul(1000));
                    format!("resets {y:04}-{mo:02}-{day:02} {h:02}:{mi:02} UTC")
                },
            );
            let label = if l.cli == Cli::Agy { l.pool_name() } else { format!("{} {}", l.cli.as_str(), l.bucket) };
            format!("{label} {} ({reset})", l.window)
        })
        .collect();
    let text = format!("A usage limit was reached: {}. Every turn on it waits for its reset.", windows.join("; "));
    let main = crate::messages::places::MAIN;
    let to = Some("andrei");
    p.post(main, factory_types::state::Sender::Engine, to, factory_types::state::MessageKind::Engine, "", &text);
}

/// When a turn's reached limit resets, if it reached one (1.4.10.2). An agy rejection that gives no reset waits for its
/// model group's stored reached windows that are still ahead (fac-657, fac-661).
pub(super) fn limit_reset(state: &State, a: &Agent, session: &SessionSeen, ctx: Ctx<'_>) -> Option<i64> {
    let seat = super::models::seat(ctx.config, a);
    if session.limit_reached {
        let stored = || {
            let bucket = seat.as_ref().filter(|s| s.cli == Cli::Agy).map(|s| agy_bucket(&s.model))?;
            let reached = state.limits.values().filter(|l| l.cli == Cli::Agy && l.bucket == bucket && l.reached);
            reached.filter_map(|l| l.resets_at).map(|s| s.saturating_mul(1000)).filter(|r| *r > ctx.now).max()
        };
        // A reset neither gave is unknown, never 0: look again in a minute.
        let reset = session.limit_reset.map(|s| s.saturating_mul(1000)).or_else(stored).unwrap_or(ctx.now + 60_000);
        return Some(reset.max(ctx.now + 1000));
    }
    let cli = seat?.cli;
    if cli != Cli::Codex {
        return None;
    }
    let reached: Vec<&Limit> = state.limits.values().filter(|l| l.cli == Cli::Codex && l.reached).collect();
    if reached.is_empty() {
        return None;
    }
    let reset = reached.iter().filter_map(|l| l.resets_at).max().map(|s| s.saturating_mul(1000));
    Some(reset.unwrap_or(ctx.now + 60_000).max(ctx.now))
}
