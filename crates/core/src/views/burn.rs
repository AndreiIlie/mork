//! Rolling 7-day burn metrics for `usage`: role burn, ended-run average, cache hit rate, and tightest pool's cards left.

use std::collections::{BTreeMap, BTreeSet};

use mork_types::config::{Cli, agy_bucket};
use mork_types::cost::Tokens;
use mork_types::usage::{CardsLeft, DayBurn, ModelCache, RecentBurn, RoleBurn};
use serde_json::Value;

/// Seven days in milliseconds.
pub const WEEK_MS: i64 = 7 * DAY_MS;
const DAY_MS: i64 = 86_400_000;
/// The daily series' length (fac-696).
pub const DAYS: i64 = 14;

/// One journal row handed into burn computation: `(time_ms, card_id, kind, data)`.
pub type Row<'a> = (i64, Option<&'a str>, &'a str, &'a Value);

/// One weekly usage pool's reading for `cards_left`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekPool {
    pub name: String,
    pub cli: Cli,
    pub bucket: String,
    pub percent: Option<u32>,
    pub reached: bool,
    pub stale: bool,
    pub signed_out: bool,
    pub reset_ms: Option<i64>,
    pub length_ms: Option<i64>,
}

/// Extracts the role from an agent key: drops trailing `/dm`, takes the last segment, drops trailing `-<digits>`.
pub fn role_of(agent: &str) -> &str {
    let base = agent.strip_suffix("/dm").unwrap_or(agent);
    let seat = base.rsplit('/').next().unwrap_or(base);
    match seat.rsplit_once('-') {
        Some((head, tail)) if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => seat,
    }
}

/// Deduplicated `(card, run, ended_ms)` entries for runs that entered `done` or `closed` in `stage_rows`.
pub fn ended_runs(stage_rows: &[Row<'_>]) -> Vec<(String, u64, i64)> {
    let mut seen: BTreeMap<(String, u64), i64> = BTreeMap::new();
    for (time, card, kind, data) in stage_rows {
        if *kind != "card.stage" {
            continue;
        }
        let Some(card) = *card else { continue };
        let stage = data.get("stage").and_then(Value::as_str);
        if !matches!(stage, Some("done" | "closed")) {
            continue;
        }
        let run = data.get("run").and_then(Value::as_u64).unwrap_or(1);
        seen.insert((card.to_owned(), run), *time);
    }
    seen.into_iter().map(|((card, run), at)| (card, run, at)).collect()
}

/// Computes the rolling 7-day burn summary from journal rows, review-card parent links, and weekly pools.
pub fn recent(
    since_ms: i64,
    cost_rows: &[Row<'_>],
    stage_rows: &[Row<'_>],
    card_rows: &BTreeMap<String, Vec<Row<'_>>>,
    parents: &BTreeMap<String, String>,
    week_pools: &[WeekPool],
) -> RecentBurn {
    let (total_cents, unknown, roles, cache_by_model) = roles_and_cache(cost_rows);
    let ended = ended_runs(stage_rows);
    let (ended_cards, average_cents) = average_burn(&ended, card_rows, parents);
    let cards_left = tightest_cards_left(week_pools, cost_rows, parents);
    let daily = Vec::new();
    RecentBurn { since_ms, total_cents, unknown, roles, ended_cards, average_cents, cache_by_model, cards_left, daily }
}

// A time's local day, counted from the epoch, at one fixed offset: the screen's current one.
fn local_day(ms: i64, tz_offset_min: i32) -> i64 {
    (ms + i64::from(tz_offset_min) * 60_000).div_euclid(DAY_MS)
}

/// Where the daily series' first local day starts, in epoch ms.
pub fn series_start(now_ms: i64, tz_offset_min: i32) -> i64 {
    (local_day(now_ms, tz_offset_min) - DAYS + 1) * DAY_MS - i64::from(tz_offset_min) * 60_000
}

/// The last `DAYS` local days' spend (`cost` rows) and landed cards (`card.landed` rows), oldest first (fac-696).
pub fn daily(now_ms: i64, tz_offset_min: i32, cost_rows: &[Row<'_>], landed_rows: &[Row<'_>]) -> Vec<DayBurn> {
    let today = local_day(now_ms, tz_offset_min);
    let n = usize::try_from(DAYS).unwrap_or(0);
    let slot = |ms: i64| usize::try_from(local_day(ms, tz_offset_min) - today + DAYS - 1).ok().filter(|i| *i < n);
    let mut days: Vec<(u64, bool, u64)> = vec![(0, false, 0); n];
    for (time, _, kind, data) in cost_rows {
        let Some(i) = slot(*time).filter(|_| *kind == "cost") else { continue };
        days[i].0 = days[i].0.saturating_add(data.get("delta").and_then(Value::as_u64).unwrap_or(0));
        days[i].1 |= data.get("unknown").and_then(Value::as_bool) == Some(true);
    }
    for (time, _, kind, _) in landed_rows {
        if let Some(i) = slot(*time).filter(|_| *kind == "card.landed") {
            days[i].2 += 1;
        }
    }
    let first = series_start(now_ms, tz_offset_min);
    days.into_iter()
        .zip(0..)
        .map(|((micros, unknown, landed), i)| DayBurn {
            day_ms: first + i * DAY_MS,
            cents: micros.div_ceil(10_000),
            unknown,
            landed,
        })
        .collect()
}

fn roles_and_cache(cost_rows: &[Row<'_>]) -> (u64, bool, Vec<RoleBurn>, Vec<ModelCache>) {
    let mut by_role: BTreeMap<String, (u64, bool)> = BTreeMap::new();
    let (mut total_micros, mut total_unknown) = (0u64, false);
    let mut by_model = BTreeMap::new();
    for (_, _, kind, data) in cost_rows {
        if *kind != "cost" {
            continue;
        }
        let delta = data.get("delta").and_then(Value::as_u64).unwrap_or(0);
        let unknown = data.get("unknown").and_then(Value::as_bool) == Some(true);
        total_micros = total_micros.saturating_add(delta);
        total_unknown |= unknown;
        if let Some(agent) = data.get("agent").and_then(Value::as_str) {
            let entry = by_role.entry(role_of(agent).to_owned()).or_default();
            entry.0 = entry.0.saturating_add(delta);
            entry.1 |= unknown;
        }
        count_prompt_tokens(&mut by_model, data);
    }
    let mut roles: Vec<RoleBurn> = by_role
        .into_iter()
        .map(|(role, (micros, unknown))| RoleBurn { role, cents: micros.div_ceil(10_000), unknown })
        .filter(|r| r.cents > 0 || r.unknown)
        .collect();
    roles.sort_by(|a, b| b.cents.cmp(&a.cents).then_with(|| a.role.cmp(&b.role)));
    let cache_by_model = by_model
        .into_iter()
        .map(|((cli, model), (reads, prompt))| ModelCache {
            cli: cli.to_owned(),
            model: model.to_owned(),
            cache_hit_percent: (reads * 100 + prompt / 2).checked_div(prompt).and_then(|pct| u32::try_from(pct).ok()),
        })
        .collect();
    (total_micros.div_ceil(10_000), total_unknown, roles, cache_by_model)
}

fn count_prompt_tokens<'a>(by_model: &mut BTreeMap<(&'a str, &'a str), (u128, u128)>, data: &'a Value) {
    let (Some(cli), Some(model)) = (data["cli"].as_str(), data["model"].as_str()) else { return };
    let (reads, prompt) = by_model.entry((cli, model)).or_default();
    if let Some(t) = data.get("tokens").and_then(|v| serde_json::from_value::<Tokens>(v.clone()).ok()) {
        *reads += u128::from(t.cache_read);
        *prompt += u128::from(t.input) + u128::from(t.cache_read) + u128::from(t.cache_write);
    }
}

fn slice_run<'a>(rows: &'a [Row<'a>], run: u64) -> &'a [Row<'a>] {
    let mut starts: Vec<(usize, u64)> = Vec::new();
    for (i, (_, _, kind, data)) in rows.iter().enumerate() {
        if let Some(r) = run_start(kind, data, starts.last().map_or(1, |(_, cur)| *cur)) {
            // A PR approval can reopen a done review as run + 1 without writing a card.start row (fac-557).
            starts.push((i, r));
        }
    }
    if let Some(pos) = starts.iter().rposition(|(_, r)| *r == run) {
        let from = starts[pos].0;
        let to = starts.get(pos + 1).map_or(rows.len(), |(i, _)| *i);
        return &rows[from..to];
    }
    if run <= 1 {
        let to = starts.first().map_or(rows.len(), |(i, _)| *i);
        return &rows[..to];
    }
    &[]
}

pub(super) fn run_start(kind: &str, data: &Value, current: u64) -> Option<u64> {
    let run = data["run"].as_u64().unwrap_or(1);
    (kind == "card.start" || (kind == "card.stage" && run > current)).then_some(run)
}

pub(super) fn parent_of<'a>(kind: &str, data: &'a Value) -> Option<&'a str> {
    matches!(kind, "card.start" | "review.adopted" | "review.ended").then(|| data["of"].as_str()).flatten()
}

fn run_parent<'a>(slice: &'a [Row<'a>], fallback: Option<&'a str>) -> Option<&'a str> {
    let from_slice = slice.iter().rev().find_map(|(_, _, k, d)| parent_of(k, d));
    from_slice.or(fallback)
}

fn sum_micros(slice: &[Row<'_>]) -> u64 {
    slice
        .iter()
        .filter(|(_, _, k, _)| *k == "cost")
        .fold(0u64, |acc, (_, _, _, d)| acc.saturating_add(d.get("delta").and_then(Value::as_u64).unwrap_or(0)))
}

fn average_burn(
    ended: &[(String, u64, i64)],
    card_rows: &BTreeMap<String, Vec<Row<'_>>>,
    parents: &BTreeMap<String, String>,
) -> (u64, Option<u64>) {
    let mut counted: Vec<(&str, i64, u64)> = Vec::new();
    let mut folded: Vec<(&str, i64, u64)> = Vec::new();
    for (card, run, end_ms) in ended {
        let rows = card_rows.get(card).map(Vec::as_slice).unwrap_or_default();
        let slice = slice_run(rows, *run);
        let micros = sum_micros(slice);
        let parent = run_parent(slice, parents.get(card).map(String::as_str));
        match parent {
            Some(p) => folded.push((p, *end_ms, micros)),
            None => counted.push((card.as_str(), *end_ms, micros)),
        }
    }
    for (parent, end_ms, micros) in folded {
        if let Some(target) = best_parent_run(&mut counted, parent, end_ms) {
            target.2 = target.2.saturating_add(micros);
        }
    }
    let count = u64::try_from(counted.len()).unwrap_or(u64::MAX);
    if count == 0 {
        return (0, None);
    }
    let sum: u128 = counted.iter().map(|(_, _, m)| u128::from(*m)).sum();
    let avg = u64::try_from(sum / u128::from(count)).unwrap_or(u64::MAX).div_ceil(10_000);
    (count, Some(avg))
}

fn best_parent_run<'a, 'b>(
    counted: &'a mut [(&'b str, i64, u64)],
    parent: &str,
    review_end_ms: i64,
) -> Option<&'a mut (&'b str, i64, u64)> {
    counted
        .iter_mut()
        .filter(|(c, _, _)| *c == parent)
        .min_by_key(|(_, end, _)| (*end < review_end_ms, end.abs_diff(review_end_ms)))
}

fn tightest_cards_left(
    pools: &[WeekPool],
    cost_rows: &[Row<'_>],
    parents: &BTreeMap<String, String>,
) -> Option<CardsLeft> {
    let mut best: Option<CardsLeft> = None;
    for pool in pools {
        let Some(cards) = pool_cards_left(pool, cost_rows, parents) else { continue };
        if best.as_ref().is_none_or(|b| cards < b.cards) {
            best = Some(CardsLeft { pool: pool.name.clone(), cards });
        }
    }
    best
}

fn pool_cards_left(pool: &WeekPool, cost_rows: &[Row<'_>], parents: &BTreeMap<String, String>) -> Option<u64> {
    if pool.signed_out || pool.stale {
        return None;
    }
    if pool.reached || pool.percent.is_some_and(|p| p >= 100) {
        return Some(0);
    }
    let percent = pool.percent.filter(|&p| p > 0 && p < 100)?;
    let start = pool.reset_ms?.checked_sub(pool.length_ms?)?;
    let mut cards: BTreeSet<&str> = BTreeSet::new();
    for (time, card, kind, data) in cost_rows {
        if *kind != "cost" || *time < start || !matches_pool(pool, data) {
            continue;
        }
        if let Some(id) = *card {
            cards.insert(parents.get(id).map_or(id, String::as_str));
        }
    }
    let count = u64::try_from(cards.len()).ok()?;
    (count > 0).then(|| count.saturating_mul(u64::from(100 - percent)) / u64::from(percent))
}

fn matches_pool(pool: &WeekPool, data: &Value) -> bool {
    let cli = data.get("cli").and_then(Value::as_str).and_then(Cli::parse);
    if cli != Some(pool.cli) {
        return false;
    }
    if pool.cli == Cli::Agy {
        return data.get("model").and_then(Value::as_str).is_some_and(|m| agy_bucket(m) == pool.bucket);
    }
    true
}
