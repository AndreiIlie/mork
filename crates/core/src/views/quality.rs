//! Journal-backed quality figures. Sequence identifies and orders work across cards sharing a timestamp.

pub(crate) mod findings;
pub(crate) mod runs;

use std::collections::BTreeMap;

use mork_types::quality::{FailureRate, Quality, QualityGroup, ToolRow};
use mork_types::review::Review;
use serde_json::Value;

pub const WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug)]
pub struct Event {
    pub seq: i64,
    pub time: i64,
    pub kind: String,
    pub data: Value,
}

impl Event {
    pub(crate) fn text(&self, field: &str) -> Option<&str> {
        self.data[field].as_str()
    }

    pub(crate) fn round(&self) -> Option<u64> {
        self.data["n"].as_u64()
    }
}

#[derive(Default)]
pub struct History {
    pub project: String,
    pub mode: String,
    pub rows: Vec<Event>,
    pub legacy: Option<Review>,
}

type Histories = BTreeMap<String, History>;

fn group(project: &str, role: &str, cost: Option<&Event>) -> QualityGroup {
    let field = |key| cost.and_then(|e| e.text(key)).unwrap_or("unknown");
    let model = match (field("cli"), field("model")) {
        ("unknown", "unknown") => "unknown".to_owned(),
        (cli, model) => format!("{cli} {model}"),
    };
    QualityGroup { project: project.into(), role: role.into(), model, effort: field("effort").into() }
}

fn cost_group(card: &History, row: &Event, review: bool) -> QualityGroup {
    let role = super::burn::role_of(row.text("agent").unwrap_or("unknown"));
    let role = if review && role == "foreman" { "review foreman" } else { role };
    group(&card.project, role, Some(row))
}

fn percent(part: usize, total: usize) -> Option<f64> {
    (total > 0).then(|| 100.0 * part as f64 / total as f64)
}

/// Aggregates events in insertion order, including earlier work belonging to runs in the reporting window.
pub fn aggregate(since: i64, cards: &BTreeMap<String, History>) -> Quality {
    let (builders, spend) = runs::aggregate(since, cards);
    Quality {
        since_ms: since,
        builders,
        spend,
        findings: findings::aggregate(since, cards),
        tools: tools(since, cards),
    }
}

fn tools(since: i64, cards: &Histories) -> Vec<ToolRow> {
    let mut groups = BTreeMap::<_, FailureRate>::new();
    for card in cards.values() {
        let mut review = card.mode == "review";
        for e in &card.rows {
            if e.kind == "card.start" {
                review = e.text("mode") == Some("review");
            }
            if e.kind != "cost" || e.time < since {
                continue;
            }
            let Some(tools) = e.data.get("tools") else { continue };
            let row = groups.entry(cost_group(card, e, review)).or_default();
            row.turns += 1;
            row.calls += tools["calls"].as_u64().unwrap_or_default();
            row.failed += tools["failed"].as_u64().unwrap_or_default();
        }
    }
    groups
        .into_iter()
        .map(|(group, mut counts)| {
            counts.percent = (counts.calls > 0).then(|| 100.0 * counts.failed as f64 / counts.calls as f64);
            ToolRow { group, counts }
        })
        .collect()
}

#[cfg(test)]
mod tests;
