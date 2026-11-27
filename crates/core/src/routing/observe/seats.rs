use super::{Event, Observer, arm, decay, ran, runs::Run};
use crate::routing::{OVERRIDE, lens, metric};
use crate::views::burn::role_of;
use mork_types::routing::{Arm, Metric};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn role<'a>(mode: &str, agent: &'a str) -> &'a str {
    match (role_of(agent), mode) {
        ("foreman", "full") => "foreman.work",
        ("foreman", "report") => "foreman.report",
        ("foreman", "review") => "review-foreman",
        (other, _) => other,
    }
}

#[derive(Default)]
struct SeatRun<'a> {
    topic: Option<&'a str>,
    route: Option<(i64, Value)>,
    last: Option<Arm>,
    tools: BTreeMap<Arm, (u64, u64, i64)>,
}

pub(super) fn turns(obs: &mut Observer, run: &Run<'_>, project: &str) {
    let mut seats = BTreeMap::<&str, SeatRun>::new();
    for e in run.rows {
        let Some(id) = e.text("agent") else { continue };
        let seat = seats.entry(id).or_default();
        let role = role(run.mode, id);
        match e.kind.as_str() {
            "child.spawned" => seat.topic = e.text("topic"),
            "route" if decay(obs.now, e.time) > 0.0 => {
                if let Some(route) = seat.route.take() {
                    finish_route(obs, route, None);
                }
                seat.route = Some((e.seq, e.data.clone()));
            }
            "cost" if ran(e) => record_turn(obs, seat, role, project, e),
            _ => {}
        }
    }
    for (id, seat) in seats {
        if let Some(route) = seat.route {
            finish_route(obs, route, None);
        }
        let role = role(run.mode, id);
        for (arm, (calls, failed, time)) in seat.tools {
            if calls == 0 || metric(role) != Metric::Reliability {
                continue;
            }
            let weight = decay(obs.now, time);
            obs.add(
                role,
                Metric::Reliability,
                &arm,
                Some(project),
                weight * (1.0 - failed.min(calls) as f64 / calls as f64),
                weight,
            );
        }
    }
}

fn record_turn(obs: &mut Observer, seat: &mut SeatRun<'_>, role: &str, project: &str, e: &Event) {
    let Some(a) = arm(e) else { return };
    if let Some(route) = seat.route.take() {
        finish_route(obs, route, Some(&a));
    }
    if let Some(old) = seat.last.as_ref().filter(|last| **last != a && e.text("by") == Some("operator")) {
        let leaf = if role == "reviewer" { seat.topic.map(lens) } else { Some(project) };
        obs.add(role, metric(role), old, leaf, 0.0, OVERRIDE * decay(obs.now, e.time));
    }
    seat.last = Some(a.clone());
    if let Some(calls) = e.data["tools"]["calls"].as_u64() {
        let totals = seat.tools.entry(a).or_default();
        totals.0 += calls;
        totals.1 += e.data["tools"]["failed"].as_u64().unwrap_or_default();
        totals.2 = e.time;
    }
}

fn finish_route(obs: &mut Observer, (seq, mut row): (i64, Value), actual: Option<&Arm>) {
    row["launched"] = json!(actual);
    if row["mode"] == "shadow" && actual.is_some() {
        let role = row["role"].as_str().unwrap_or_default();
        let count = obs.stats.shadow.entry(role.to_owned()).or_default();
        count[0] += usize::from(row["pick"] == row["launched"]);
        count[1] += 1;
    }
    obs.stats.recent.push((seq, row));
}

pub(super) fn costs(obs: &mut Observer, run: &Run<'_>, project: &str, builder: Option<&Arm>) {
    let ended = run.rows.iter().find(|e| {
        matches!(e.kind.as_str(), "card.landed" | "review.ended" | "lookback.done")
            || (run.mode != "full" && e.kind == "card.stage" && e.text("stage") == Some("done"))
    });
    let Some(end) = ended else { return };
    let weight = decay(obs.now, end.time);
    let mut costs = BTreeMap::<&str, (Option<Arm>, u128, bool)>::new();
    let mut topics = BTreeMap::new();
    for e in run.rows {
        let Some(agent) = e.text("agent") else { continue };
        if e.kind == "child.spawned" {
            topics.insert(agent, e.text("topic").map(lens));
        }
        if e.kind != "cost" || !ran(e) {
            continue;
        }
        let total = costs.entry(agent).or_default();
        if total.0.is_none() {
            total.0 = arm(e);
        }
        total.1 += u128::from(e.data["delta"].as_u64().unwrap_or_default());
        total.2 |= e.data["unknown"] == true;
    }
    for (agent, (first, micros, unknown)) in costs {
        let role = role(run.mode, agent);
        let arm = if role == "foreman.work" { builder } else { first.as_ref() };
        let leaf = if role == "reviewer" { topics.get(agent).copied().flatten() } else { Some(project) };
        if let Some(a) = arm.filter(|_| !unknown) {
            obs.add(role, Metric::Cost, a, leaf, weight * micros as f64 / 1_000_000.0, weight);
        }
    }
}
