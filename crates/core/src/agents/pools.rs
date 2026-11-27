//! Ranking model and effort pairs against the quota windows used by their CLI.

use std::collections::BTreeMap;

use mork_types::config::{Cli, Effort, PickModel, Seat, agy_bucket, bench};
use mork_types::usage::{Limit, length_ms, pool_name, short_name};

use super::usage::pace;
use mork_types::config::Wall;

// An unwalled Claude seat's deny list exists only in Claude Code (fac-25, fac-402); agy's wall alone holds it (fac-657).
pub(super) fn walls_kept(seat: &Seat, cli: Cli) -> bool {
    seat.wall != Wall::None || (cli != Cli::Agy && (seat.cli == Cli::Codex || cli == Cli::Claude))
}

const POLLED_MAX_AGE: i64 = 10 * 60_000;
const UNTOUCHED_SPARE: u32 = 50;
const SHORT_CEILING: u32 = 60;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Offer {
    pub cli: Cli,
    pub model: String,
    pub effort: Effort,
    pub index: f64,
    pub scale: f64,
    pub spare: u32,
    pub open: bool,
    pub pool: String,
}

impl Offer {
    pub(crate) fn choice(&self, phase: mork_types::config::SeatPhase) -> mork_types::state::Choice {
        mork_types::state::Choice {
            cli: self.cli,
            model: self.model.clone(),
            effort: self.effort,
            by: "router".into(),
            phase: Some(phase),
        }
    }
}

/// Quota and benchmark inputs in the configured order, restricted to this seat’s wall.
pub fn offers(
    limits: &BTreeMap<String, Limit>,
    now: i64,
    account: Option<&str>,
    seat: &Seat,
    allowed: &[PickModel],
) -> Vec<Offer> {
    let mut offers = Vec::new();
    for entry in allowed {
        let cli = entry.cli;
        if !walls_kept(seat, cli) {
            continue;
        }
        let bucket = if cli == Cli::Agy { agy_bucket(&entry.model) } else { "account" };
        let pool = pool_name(cli, bucket);
        let pool = if let Some(id) = account.filter(|_| cli == Cli::Claude) { format!("{pool} {id}") } else { pool };
        let spare = headroom(limits, entry, bucket, account, now);
        for &effort in &entry.efforts {
            let Some((index, _)) = bench::benchmark(&entry.model, effort) else { continue };
            if !cli.takes(&entry.model, effort) {
                continue;
            }
            offers.push(Offer {
                cli,
                model: entry.model.clone(),
                effort,
                pool: pool.clone(),
                open: spare.is_some(),
                spare: spare.unwrap_or_default(),
                index,
                scale: bench::cost_scale(cli, bucket),
            });
        }
    }
    offers
}

fn headroom(
    limits: &BTreeMap<String, Limit>,
    entry: &PickModel,
    bucket: &str,
    account: Option<&str>,
    now: i64,
) -> Option<u32> {
    if entry.cli == Cli::Claude && account.is_none() {
        return None;
    }
    let rows = limits.values().filter(|l| {
        l.cli == entry.cli && (l.cli != Cli::Claude || l.account.as_ref().map(|a| a.id.as_str()) == account)
    });
    let required = |name| weekly(rows.clone().find(|l| l.bucket == bucket && short_name(&l.window) == name)?, now);
    let mut spare = required("week")?;
    if entry.cli == Cli::Claude && entry.model.to_ascii_lowercase().contains("fable") {
        spare = spare.min(required("fable")?);
    }
    for row in rows.filter(|l| !l.expired(now)) {
        let name = short_name(&row.window);
        let model_row = match entry.cli {
            Cli::Claude => name.strip_prefix("week ").is_some_and(|suffix| entry.model.contains(suffix)),
            Cli::Codex => row.bucket == entry.model,
            Cli::Agy => false,
        };
        if !(model_row || row.bucket == bucket && name == "5h") {
            continue;
        }
        if row.reached || (name == "5h" && row.percent.is_some_and(|p| p > SHORT_CEILING)) {
            return None;
        }
        if model_row && (name == "week" || name.starts_with("week ")) {
            spare = spare.min(weekly(row, now)?);
        }
    }
    Some(spare)
}

fn weekly(row: &Limit, now: i64) -> Option<u32> {
    if row.reached || (row.cli != Cli::Claude && now.saturating_sub(row.seen_ms) > POLLED_MAX_AGE) {
        return None;
    }
    if row.reset_ms().is_none_or(|reset| reset <= now) {
        return (row.cli != Cli::Claude && row.percent == Some(0)).then_some(UNTOUCHED_SPARE);
    }
    let length = length_ms(&row.window);
    let (_, end) = pace(row.percent, row.reset_ms(), length, row.seen_ms)?;
    let (gone, _) = pace(Some(0), row.reset_ms(), length, now)?;
    let spare = (100_u64.saturating_sub(end) * u64::from(gone) + 50) / 100;
    u32::try_from(spare).ok()
}
