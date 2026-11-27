//! An agent's cost and context window, as its turns' streams showed them (1.4.8, Claude 7 and
//! 9), kept in the state so a restart loses neither (2.2.1).

use std::collections::BTreeMap;

use mork_types::config::{AGY_PRICES, CODEX_PRICES, Cli, Config};
use mork_types::cost::{Selection, Tokens};
use mork_types::state::{Agent, Job};
use serde_json::{Value, json};

use crate::decide::{JobEnd, SessionSeen};

/// Stored context usage, effective window, and window model for a Claude or agy seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredContext<'a> {
    pub used: u64,
    pub window: Option<u64>,
    pub model: Option<&'a str>,
}

/// Stored context used, effective window and window model for a Claude or agy seat; none for Codex or before a report.
pub fn stored_context<'a>(config: &Config, a: &'a Agent) -> Option<StoredContext<'a>> {
    let cli = super::models::seat(config, a).map(|s| s.cli);
    if cli == Some(Cli::Codex) {
        return None;
    }
    let used = a.context?;
    // agy reports no window; ignore any stale Claude window kept across a CLI switch.
    let own = a.window.as_ref().filter(|_| cli != Some(Cli::Agy));
    let reported = own.map(|(_, w)| *w).filter(|w| *w > 0);
    Some(StoredContext { used, window: config.mork.effective_window(reported), model: own.map(|(m, _)| m.as_str()) })
}

/// Records a job's cost once, with its share of the running token and dollar totals (fac-526).
pub(super) fn note(a: &mut Agent, s: &SessionSeen, job: &Job, now: i64, replied: bool) -> Option<Value> {
    if let Some(w) = &s.window {
        a.window = Some(w.clone());
    }
    if s.context.is_some() {
        a.context = s.context;
    }
    if a.cost_job.as_ref() == Some(&job.id) {
        return None;
    }
    let session = a.session.as_ref().map_or_else(|| "?".to_owned(), ToString::to_string);
    let before = a.costs.get(&session).copied().unwrap_or(0);
    let selection = s.selection.clone().or_else(|| Selection::from_argv(&job.spec.argv));
    let tokens = token_share(a, &s.token_totals);
    let delta = s.cost_micros.map(|total| total.saturating_sub(before)).or_else(|| {
        let model = selection.as_ref()?;
        priced(model.cli, &model.model, tokens?)
    });
    let total = s.cost_micros.or_else(|| delta.map(|d| before.saturating_add(d)));
    if let Some(total) = total {
        a.costs.insert(session.clone(), total);
    }
    a.cost_job = Some(job.id.clone());
    let mut line = json!({
        "ran": replied || s.answered || s.tools || delta.is_some_and(|n| n > 0) || tokens.is_some_and(|t| t != Tokens::default()),
        "by": job.chosen_by, "agent": a.key.as_str(), "job": job.id, "session": session, "at": now,
        "cli": selection.as_ref().map(|s| s.cli), "model": selection.as_ref().map(|s| s.model.as_str()),
        "effort": selection.as_ref().map(|s| s.effort), "delta": delta.unwrap_or(0), "unknown": delta.is_none(),
    });
    if let Some(tools) = &s.tool_counts {
        line["tools"] = json!(tools);
    }
    if let Some(total) = total {
        line["total"] = json!(total);
    }
    if let Some(tokens) = tokens {
        line["tokens"] = json!(tokens);
    }
    Some(line)
}

/// A job's cost at settlement, unless its captured session already recorded it.
pub(super) fn of_end(a: &mut Agent, end: &JobEnd, job: &Job, now: i64) -> Option<Value> {
    let empty = SessionSeen::default();
    let session = match end {
        JobEnd::Finished { session, .. } | JobEnd::Cut { session } => session,
        JobEnd::Stopped | JobEnd::Failed { .. } => &empty,
    };
    note(a, session, job, now, matches!(end, JobEnd::Finished { reply: Ok(_), .. }))
}

fn token_share(a: &mut Agent, totals: &BTreeMap<String, Tokens>) -> Option<Tokens> {
    if totals.is_empty() {
        return None;
    }
    Some(totals.iter().fold(Tokens::default(), |sum, (thread, total)| {
        let before = a.token_totals.insert(thread.clone(), *total).unwrap_or_default();
        sum.saturating_add(total.saturating_sub(before))
    }))
}

// Codex and agy report tokens alone, priced here; Claude its dollars.
fn priced(cli: Cli, model: &str, tokens: Tokens) -> Option<u64> {
    let prices = match cli {
        Cli::Codex => CODEX_PRICES,
        Cli::Agy => AGY_PRICES,
        Cli::Claude => return None,
    };
    let price = prices.iter().find(|(m, _)| *m == model)?.1;
    let counts = [tokens.input, tokens.cache_read, tokens.cache_write, tokens.output];
    let millis: u128 = counts.into_iter().zip(price).map(|(n, p)| u128::from(n) * u128::from(p)).sum();
    ((millis + 500) / 1000).try_into().ok()
}
