use super::{Event, History, Observer, arm, decay};
use crate::routing::{PRIOR, lens};
use crate::views::quality::findings;
use mork_types::config::{Cli, Effort};
use mork_types::routing::{Arm, Estimate, Metric};
use std::collections::BTreeMap;

struct Reviewer {
    arm: Arm,
    lens: Option<String>,
    fixed: usize,
}

pub(super) struct Round {
    reviewers: Vec<Reviewer>,
    partner: Option<String>,
    weight: f64,
}

pub(super) fn collect(obs: &mut Observer, card: &History, partners: &BTreeMap<i64, String>, out: &mut Vec<Round>) {
    if !card.rows.iter().any(|r| r.kind == "review.verdict") {
        return;
    }
    let epochs = findings::generations(&card.rows);
    for (i, rows) in epochs.iter().enumerate() {
        let snapshot = card.legacy.as_ref().filter(|_| i + 1 == epochs.len());
        let decided = findings::decisions(card, obs.now.saturating_sub(crate::routing::WINDOW), rows, snapshot);
        let mut fixed = BTreeMap::<(u64, String), usize>::new();
        for f in decided.values().filter(|f| f.by != "foreman" && (f.fixed || f.declined)) {
            if f.fixed {
                *fixed.entry((f.round, f.by.clone())).or_default() += 1;
            }
            let Some((cli, model)) = f.group.model.split_once(' ') else { continue };
            let (Some(cli), Some(effort)) = (Cli::parse(cli), Effort::parse(&f.group.effort)) else { continue };
            let arm = Arm { cli, model: model.to_owned(), effort };
            let leaf = (f.lens != "unknown").then(|| lens(&f.lens));
            let weight = decay(obs.now, f.time);
            obs.add("reviewer", Metric::Precision, &arm, leaf, weight * f64::from(f.fixed), weight);
        }
        bound(obs, rows, partners, &fixed, out);
    }
}

fn bound(
    obs: &Observer,
    rows: &[Event],
    partners: &BTreeMap<i64, String>,
    fixed: &BTreeMap<(u64, String), usize>,
    out: &mut Vec<Round>,
) {
    let mut topics = BTreeMap::new();
    let mut jobs = BTreeMap::new();
    let mut builders = BTreeMap::new();
    let mut reports = BTreeMap::<u64, BTreeMap<&str, Reviewer>>::new();
    for e in rows {
        match e.kind.as_str() {
            "child.spawned" => {
                topics.insert(e.text("agent"), e.text("topic").map(lens));
            }
            "cost" => {
                if let (Some(job), Some(a)) = (e.text("job"), arm(e)) {
                    jobs.insert(job, a);
                }
            }
            "review.asked" => {
                builders.insert(e.round(), partners.get(&e.seq));
            }
            "reply" if matches!(e.text("outcome"), Some("review-pass" | "review-fail")) => {
                let (Some(n), Some(id), Some(job)) = (e.data["round"].as_u64(), e.text("agent"), e.text("job")) else {
                    continue;
                };
                let Some(arm) = jobs.get(job).cloned() else { continue };
                let by = id.rsplit('/').next().unwrap_or(id);
                let fixed = fixed.get(&(n, by.to_owned())).copied().unwrap_or_default();
                let lens = topics.get(&Some(id)).copied().flatten().map(str::to_owned);
                reports.entry(n).or_default().insert(by, Reviewer { arm, lens, fixed });
            }
            "review.verdict" => {
                let Some(n) = e.round() else { continue };
                let reviewers = reports.remove(&n).unwrap_or_default().into_values().collect();
                let partner = builders.get(&Some(n)).copied().flatten().cloned();
                out.push(Round { reviewers, partner, weight: decay(obs.now, e.time) });
            }
            _ => {}
        }
    }
}

pub(super) fn catch(obs: &mut Observer, rounds: &[Round]) {
    let mut opportunities = BTreeMap::<Option<&str>, Estimate>::new();
    let mut overall = Estimate::default();
    for r in rounds {
        for v in &r.reviewers {
            let fixed = v.fixed as f64 * r.weight;
            opportunities.entry(v.lens.as_deref()).or_default().add(fixed, r.weight);
            overall.add(fixed, r.weight);
        }
    }
    let expected = |v: &Reviewer| {
        let rate = opportunities[&v.lens.as_deref()];
        (rate.sum + PRIOR * overall.mean(1.0)) / (rate.weight + PRIOR)
    };
    for r in rounds.iter().filter(|r| r.reviewers.len() >= 2 && r.weight > 0.0) {
        let total = r.reviewers.iter().map(|v| v.fixed).sum::<usize>();
        if total == 0 {
            continue;
        }
        let all: f64 = r.reviewers.iter().map(expected).sum();
        for v in &r.reviewers {
            let baseline = expected(v);
            let wins = v.fixed as f64 * r.weight;
            let losses = (total - v.fixed) as f64 * r.weight * baseline / (all - baseline);
            obs.add("reviewer", Metric::Catch, &v.arm, v.lens.as_deref(), wins, wins + losses);
            obs.pair("reviewer", &v.arm, r.partner.as_deref(), wins, wins + losses);
        }
    }
}
