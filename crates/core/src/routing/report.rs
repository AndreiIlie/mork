use mork_types::config::{Config, PickModel, RoutingMode, Standing, routing::ROLES};
use mork_types::routing::{Arm, Metric, Statistics};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::{
    choose::{Inputs, mean_index, price},
    metric,
    posterior::Population,
    seats::role,
};
use crate::agents::pools;

pub fn answer(config: &Config, inputs: &Inputs<'_>, stats: &Statistics) -> Value {
    let mut roles = Vec::new();
    let mut pools = BTreeMap::new();
    for &(name, at) in ROLES {
        let (table, list) = config.router_list(name, at);
        let mode = config.routing_mode(name, at);
        roles.push(profile(config, inputs, stats, (name, at), (table, list, mode), &mut pools));
    }
    if let Some(picks) = &config.picks
        && !picks.pass.is_empty()
    {
        for (name, at) in [("foreman", Standing::ALL[1]), ("researcher", Standing::WORK)] {
            roles.push(profile(
                config,
                inputs,
                stats,
                (name, at),
                ("picks.pass", &picks.pass, RoutingMode::Auto),
                &mut pools,
            ));
        }
    }
    let synergy: Vec<_> = stats
        .partners
        .iter()
        .map(|((role, model, partner), rate)| {
            let population = Population { stats, role, mean_index: 1.0 };
            let effect = population.synergy(model, Some(partner));
            json!({"role":role, "model":model, "partner":partner, "weight":rate.weight, "synergy":effect})
        })
        .collect();
    let picks: Vec<_> = stats.recent.iter().map(|(_, row)| row).collect();
    let shadow: BTreeMap<_, _> =
        stats.shadow.iter().map(|(role, [matched, total])| (role, json!({"matched":matched,"total":total}))).collect();
    json!({"routing":{"roles":roles,"pools":pools,"synergy":synergy,"picks":picks,"shadow":shadow}})
}

fn profile(
    config: &Config,
    inputs: &Inputs<'_>,
    stats: &Statistics,
    (name, at): (&str, Standing),
    (table, list, mode): (&str, &[PickModel], RoutingMode),
    pools: &mut BTreeMap<String, Value>,
) -> Value {
    let Some(seat) = config.seat_in(name, at) else { return Value::Null };
    let default;
    let list = if list.is_empty() {
        default = PickModel {
            cli: seat.cli,
            model: seat.model.clone(),
            label: seat.label.clone(),
            efforts: vec![seat.effort],
        };
        std::slice::from_ref(&default)
    } else {
        list
    };
    let offers = pools::offers(inputs.limits, inputs.now, inputs.account, &seat, list);
    let role = role(name, at.phase);
    let pop = Population { stats, role, mean_index: mean_index(&offers) };
    let mut arms = BTreeSet::new();
    for offer in offers {
        let pool = pools.entry(offer.pool.clone()).or_insert_with(|| json!({}));
        pool[&offer.model] = json!({"spare":offer.spare,"price":price(offer.spare),"open":offer.open});
        arms.insert(Arm { cli: offer.cli, model: offer.model, effort: offer.effort });
    }
    for ((r, _), rows) in &stats.outcomes {
        if r == role {
            arms.extend(rows.keys().cloned());
        }
    }
    let estimates: Vec<_> = arms
        .into_iter()
        .filter(|a| mork_types::config::bench::benchmark(&a.model, a.effort).is_some())
        .map(|a| arm_report(&pop, &a))
        .collect();
    json!({"role":role,"mode":mode,"list":table,"kind":at.review.map(|k|format!("{k:?}").to_lowercase()),"arms":estimates})
}

fn arm_report(pop: &Population<'_>, a: &Arm) -> Value {
    let mut outcomes = Vec::new();
    let mut metrics = vec![metric(pop.role), Metric::Cost];
    if metric(pop.role) == Metric::Catch {
        metrics.push(Metric::Precision);
    }
    for metric in metrics {
        let levels = pop.stats.outcomes.get(&(pop.role.to_owned(), metric)).and_then(|r| r.get(a));
        let leaves: BTreeMap<_, _> = levels
            .into_iter()
            .flat_map(|l| l.leaves.keys())
            .map(|leaf| (leaf, pop.levels(metric, a, Some(leaf))[1]))
            .collect();
        outcomes.push(json!({"metric":metric,"global":pop.levels(metric,a,None)[0],"leaves":leaves}));
    }
    json!({"arm":a,"posteriors":outcomes})
}
