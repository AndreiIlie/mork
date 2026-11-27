mod reviewers;
mod seats;

use std::collections::BTreeMap;

use mork_types::config::{Cli, Effort};
use mork_types::routing::{Arm, Metric, Statistics};

use super::{HALF_LIFE, PLANNER, WINDOW};
use crate::views::quality::{Event, History, runs};

pub(super) fn decay(now: i64, time: i64) -> f64 {
    let age = now.saturating_sub(time).max(0);
    if age > WINDOW { 0.0 } else { (-((age as f64) / HALF_LIFE)).exp2() }
}

fn arm(row: &Event) -> Option<Arm> {
    Some(Arm {
        cli: Cli::parse(row.text("cli")?)?,
        model: row.text("model")?.to_owned(),
        effort: Effort::parse(row.text("effort")?)?,
    })
}

fn ran(row: &Event) -> bool {
    row.data["ran"].as_bool().unwrap_or_else(|| {
        row.data["delta"].as_u64().unwrap_or(0) > 0
            || row.data["tokens"].as_object().is_some_and(|t| t.values().any(|v| v.as_u64().unwrap_or(0) > 0))
    })
}

struct Observer {
    now: i64,
    stats: Statistics,
}

impl Observer {
    fn add(&mut self, role: &str, metric: Metric, arm: &Arm, leaf: Option<&str>, sum: f64, weight: f64) {
        if weight <= 0.0 {
            return;
        }
        let levels = self.stats.outcomes.entry((role.to_owned(), metric)).or_default().entry(arm.clone()).or_default();
        levels.global.add(sum, weight);
        if let Some(leaf) = leaf {
            levels.leaves.entry(leaf.to_owned()).or_default().add(sum, weight);
        }
    }

    fn pair(&mut self, role: &str, arm: &Arm, partner: Option<&str>, sum: f64, weight: f64) {
        if let Some(partner) = partner.filter(|_| weight > 0.0) {
            self.stats
                .partners
                .entry((role.to_owned(), arm.model.clone(), partner.to_owned()))
                .or_default()
                .add(sum, weight);
        }
    }
}

/// Builds a snapshot from complete, sequenced histories for outcomes in the observation window.
pub fn aggregate(now: i64, cards: &BTreeMap<String, History>) -> Statistics {
    let mut obs = Observer { now, stats: Statistics::default() };
    let mut partners = BTreeMap::new();
    runs::with_runs(cards, |id, run, reviews| {
        let asked = reviews.iter().copied().filter(|r| r.kind == "review.asked");
        for event in asked.clone() {
            if let Some(a) = runs::builder(run.rows, Some(event)).and_then(arm) {
                partners.insert(event.seq, a.model);
            }
        }
        let builder = runs::builder(run.rows, asked.clone().next()).and_then(arm);
        let project = cards[id].project.as_str();
        seats::costs(&mut obs, run, project, builder.as_ref());
        verified(&mut obs, project, run, reviews, builder.as_ref());
    });
    let mut rounds = Vec::new();
    for card in cards.values() {
        for run in runs::runs(card) {
            seats::turns(&mut obs, &run, &card.project);
            if run.mode != "full" {
                seats::costs(&mut obs, &run, &card.project, None);
            }
        }
        reviewers::collect(&mut obs, card, &partners, &mut rounds);
    }
    reviewers::catch(&mut obs, &rounds);
    obs.stats
}

fn verified(obs: &mut Observer, project: &str, run: &runs::Run<'_>, reviews: &[&Event], builder: Option<&Arm>) {
    let Some((event, first_pass)) = runs::verified(run, reviews) else { return };
    let weight = decay(obs.now, event.time);
    let first = f64::from(first_pass);
    if let Some(builder) = builder {
        obs.add("foreman.work", Metric::Quality, builder, Some(project), weight * first, weight);
    }
    let rows = &run.rows[..run.rows.partition_point(|e| e.seq < event.seq)];
    let Some(planner) = approved(rows) else { return };
    if let Some(builder) = builder {
        obs.pair("foreman.work", builder, Some(&planner.model), weight * first, weight);
    }
    let objections = rows.iter().filter(|e| e.kind == "challenge.answered" && e.data["objections"] == true).count();
    let stayed = !rows.iter().any(|e| e.kind == "card.rebriefed");
    let score = PLANNER[0] / (1.0 + objections as f64) + PLANNER[1] * f64::from(stayed) + PLANNER[2] * first;
    obs.add("planner", Metric::Quality, &planner, Some(project), weight * score, weight);
}

fn approved(rows: &[Event]) -> Option<Arm> {
    let approval = rows.iter().rposition(|r| r.kind == "brief.approved")?;
    let planner = |e: &Event| e.text("agent").is_some_and(|a| a.ends_with("/planner"));
    let recap = rows[..approval]
        .iter()
        .rposition(|e| e.kind == "reply" && e.text("outcome") == Some("brief-ready") && planner(e))?;
    rows[..recap].iter().rev().find(|e| e.kind == "cost" && planner(e)).and_then(arm)
}
