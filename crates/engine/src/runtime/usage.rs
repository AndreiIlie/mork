use mork_core::state::State;
use mork_types::config::Cli;
use mork_types::usage::{Limit, LimitView};
use mork_types::view::Share;

use crate::claude_accounts::Accounts;
use crate::cli::limits::ACCOUNT;
use mork_types::usage::{length_ms, short_name};

struct Reading<'a> {
    limit: &'a Limit,
    name: String,
    window: String,
    reset_ms: Option<i64>,
    stale: bool,
    signed_out: bool,
    pace: Option<(u32, u64)>,
}

fn readings<'a>(state: &'a State, now: i64, accounts: &Accounts) -> Vec<Reading<'a>> {
    let active = accounts.live.as_ref();
    let saved = &accounts.saved;
    let mut out: Vec<_> = state
        .limits
        .values()
        .filter(|l| {
            l.cli != Cli::Claude
                || l.account.as_ref().is_some_and(|account| {
                    active.is_some_and(|live| live.id == account.id)
                        || saved.values().any(|saved| saved.id == account.id)
                })
        })
        .map(|l| {
            let name = l.account.as_ref().map_or_else(
                || l.pool_name(),
                |a| {
                    saved
                        .iter()
                        .find(|(_, saved)| saved.id == a.id)
                        .and_then(|(name, _)| accounts.header(name))
                        .map_or_else(|| format!("claude {}", a.plan), str::to_owned)
                },
            );
            let reset_ms = l.reset_ms();
            let stale = l.expired(now);
            let signed_out = l.cli == Cli::Claude && l.account.as_ref().map(|a| &a.id) != active.map(|a| &a.id);
            let pace = (!stale)
                .then(|| mork_core::agents::usage::pace(l.percent, reset_ms, length_ms(&l.window), l.seen_ms))
                .flatten();
            Reading { limit: l, name, window: short_name(&l.window), reset_ms, stale, signed_out, pace }
        })
        .collect();
    out.sort_by_key(|r| {
        let window = match r.window.as_str() {
            "5h" => 0,
            "week" => 1,
            "fable" => 2,
            _ => 3,
        };
        let agy_3p = r.limit.cli == Cli::Agy && r.limit.bucket != ACCOUNT;
        (
            r.limit.cli != Cli::Claude,
            r.signed_out,
            r.limit.cli.as_str(),
            agy_3p,
            r.name.clone(),
            r.limit.account.as_ref().map(|a| a.id.clone()),
            r.limit.bucket != ACCOUNT,
            window,
        )
    });
    out
}

fn pool(r: &Reading<'_>) -> bool {
    match r.limit.cli {
        Cli::Claude => r.limit.bucket == ACCOUNT && ["5h", "week", "fable"].contains(&r.window.as_str()),
        Cli::Codex => r.limit.bucket == ACCOUNT && r.window == "week",
        Cli::Agy => {
            [ACCOUNT, mork_types::config::AGY_3P_BUCKET].contains(&r.limit.bucket.as_str())
                && ["5h", "week"].contains(&r.window.as_str())
        }
    }
}

fn age(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match minutes {
        n if n >= 1440 => format!("{}d", n / 1440),
        n if n >= 60 => format!("{}h", n / 60),
        n => format!("{n}m"),
    }
}

pub(super) fn shares(state: &State, now: i64, accounts: &Accounts) -> Vec<Share> {
    readings(state, now, accounts)
        .into_iter()
        .filter(|r| pool(r) && (r.limit.percent.is_some() || r.limit.reached))
        .map(|r| {
            let seen = state.limits.values().filter(|l| l.account == r.limit.account).map(|l| l.seen_ms).max();
            Share {
                cli: r.name,
                window: r.window,
                percent: r.limit.percent,
                reached: r.limit.reached,
                stale: r.stale,
                reset_ms: r.reset_ms,
                ends_percent: r.pace.map(|(_, end)| end),
                signed_out: r.signed_out,
                switch_account: r.limit.account.as_ref().filter(|_| r.signed_out).and_then(|account| {
                    accounts.saved.iter().find(|(_, saved)| saved.id == account.id).map(|(name, _)| name.clone())
                }),
                read_ago: r.signed_out.then(|| age(now.saturating_sub(seen.unwrap_or(r.limit.seen_ms)))),
            }
        })
        .collect()
}

pub(super) fn week_pools(state: &State, now: i64, accounts: &Accounts) -> Vec<mork_core::views::burn::WeekPool> {
    readings(state, now, accounts)
        .into_iter()
        .filter(|r| pool(r) && r.window == "week")
        .map(|r| mork_core::views::burn::WeekPool {
            name: r.name,
            cli: r.limit.cli,
            bucket: r.limit.bucket.clone(),
            percent: r.limit.percent,
            reached: r.limit.reached,
            stale: r.stale,
            signed_out: r.signed_out,
            reset_ms: r.reset_ms,
            length_ms: length_ms(&r.limit.window),
        })
        .collect()
}

pub(super) fn answer(state: &State, now: i64, accounts: &Accounts) -> (Vec<String>, Vec<LimitView>) {
    let readings = readings(state, now, accounts);
    let lines = readings.iter().filter(|r| pool(r)).map(|r| line(r, now)).collect();
    let limits = readings
        .iter()
        .map(|r| {
            let l = r.limit;
            let source = match l.cli {
                Cli::Claude => "claude's stream (rate_limit_event)",
                Cli::Codex => "codex's limits read (app-server)",
                Cli::Agy => "agy's limits read (/usage)",
            };
            LimitView {
                cli: l.cli,
                account: l.account.clone(),
                bucket: l.bucket.clone(),
                window: r.window.clone(),
                name: l.window.clone(),
                percent: l.percent,
                resets_at: l.resets_at,
                reached: l.reached,
                seen_ms: l.seen_ms,
                length_ms: length_ms(&l.window),
                source: source.to_owned(),
                age_ms: now.saturating_sub(l.seen_ms),
                unknown: l.percent.is_none().then(|| "the CLI gave no share for this window".to_owned()),
            }
        })
        .collect();
    (lines, limits)
}

fn line(r: &Reading<'_>, now: i64) -> String {
    let used =
        if r.stale { "stale".into() } else { r.limit.percent.map_or_else(|| "?".into(), |p| format!("{p}% used")) };
    let mut out = format!("{} {}: {used}", r.name, r.window);
    if let Some((gone, end)) = r.pace {
        let period = if r.window == "5h" { "window" } else { "week" };
        out.push_str(&format!(", {gone}% of the {period} gone: ends near {end}%"));
    }
    if r.signed_out {
        out.push_str(&format!(" (read {} ago)", age(now.saturating_sub(r.limit.seen_ms))));
    }
    out
}
