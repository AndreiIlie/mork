use super::{CLAMP, PAR, PRIOR, RELIABLE};
use mork_types::config::bench;
use mork_types::routing::{Arm, Estimate, Metric, PartnerEffect, Statistics};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Serialize)]
pub(super) struct Posterior {
    pub mean: f64,
    pub weight: f64,
}

fn shrink(prior: f64, counts: Estimate) -> Posterior {
    let weight = counts.weight + PRIOR;
    Posterior { mean: (prior * PRIOR + counts.sum) / weight, weight }
}

pub(super) struct Population<'a> {
    pub stats: &'a Statistics,
    pub role: &'a str,
    pub mean_index: f64,
}

impl Population<'_> {
    pub fn levels(&self, metric: Metric, a: &Arm, leaf: Option<&str>) -> [Posterior; 2] {
        let arms = self.stats.outcomes.get(&(self.role.to_owned(), metric));
        let mut population = Estimate::default();
        let mut benchmark = Estimate::default();
        for (arm, data) in arms.into_iter().flatten() {
            let Some((index, cost)) = bench::benchmark(&arm.model, arm.effort) else { continue };
            let reference = if metric == Metric::Cost { cost } else { index };
            benchmark.add(reference * data.global.weight, data.global.weight);
            population.add(data.global.sum, data.global.weight);
        }
        let (index, cost) = bench::benchmark(&a.model, a.effort).unwrap_or_default();
        let prior = match metric {
            Metric::Cost if population.sum <= 0.0 => cost,
            Metric::Cost => population.mean(cost) * cost / benchmark.mean(cost),
            Metric::Reliability => population.mean(RELIABLE),
            Metric::Quality | Metric::Catch | Metric::Precision => {
                population.mean(PAR) * index / benchmark.mean(self.mean_index)
            }
        };
        let bounded = |p: f64| if metric == Metric::Cost { p } else { p.clamp(CLAMP.0, CLAMP.1) };
        let data = arms.and_then(|group| group.get(a));
        let estimate = |prior, counts| {
            let mut p = shrink(bounded(prior), counts);
            if metric == Metric::Cost && population.sum <= 0.0 {
                p.mean = cost;
            }
            p
        };
        let global = estimate(prior, data.map(|l| l.global).unwrap_or_default());
        let counts = data.and_then(|l| l.leaves.get(leaf?)).copied().unwrap_or_default();
        let local = if leaf.is_some() { estimate(global.mean, counts) } else { global };
        [global, local]
    }

    pub fn synergy(&self, model: &str, partner: Option<&str>) -> f64 {
        let Some(partner) = partner else { return 0.0 };
        self.stats
            .partners
            .get(&(self.role.to_owned(), model.to_owned(), partner.to_owned()))
            .map_or(0.0, |p| p.synergy)
    }
}

pub(super) fn fit_partners(
    counts: BTreeMap<(String, String), Estimate>,
) -> impl Iterator<Item = ((String, String), PartnerEffect)> {
    let mut models = BTreeMap::<&str, Vec<usize>>::new();
    let mut partners = BTreeMap::<&str, Vec<usize>>::new();
    let mut weights = Vec::new();
    let mut rates = Vec::new();
    for (i, ((model, partner), count)) in counts.iter().enumerate() {
        models.entry(model).or_default().push(i);
        partners.entry(partner).or_default().push(i);
        weights.push(count.weight);
        rates.push(count.mean(PAR));
    }
    let models: Vec<_> = models.into_values().collect();
    let partners: Vec<_> = partners.into_values().collect();
    center_pairs(&mut rates, &weights, &models, &partners);
    counts.into_iter().zip(rates).map(|((key, count), residual)| {
        let weight = count.weight;
        let synergy = if residual.abs() < 1e-12 { 0.0 } else { residual * weight / (weight + PRIOR) };
        (key, PartnerEffect { synergy, weight })
    })
}

fn group_mean(values: &[f64], weights: &[f64], group: &[usize]) -> f64 {
    let first = values[group[0]];
    let mut offsets = Estimate::default();
    for &i in group {
        offsets.add((values[i] - first) * weights[i], weights[i]);
    }
    first + offsets.mean(0.0)
}

fn center_models(values: &mut [f64], weights: &[f64], models: &[Vec<usize>]) {
    for group in models {
        let mean = group_mean(values, weights, group);
        for &i in group {
            values[i] -= mean;
        }
    }
}

fn weighted_dot(a: &[f64], b: &[f64], weights: &[f64]) -> f64 {
    a.iter().zip(b).zip(weights).map(|((a, b), w)| a * b * w).sum()
}

fn remove_projection(values: &mut [f64], unit: &[f64], weights: &[f64]) {
    let projection = weighted_dot(values, unit, weights);
    for (value, axis) in values.iter_mut().zip(unit) {
        *value -= projection * axis;
    }
}

fn center_pairs(rates: &mut [f64], weights: &[f64], models: &[Vec<usize>], partners: &[Vec<usize>]) {
    center_models(rates, weights, models);
    let mut directions = Vec::<Vec<f64>>::new();
    let tolerance = f64::EPSILON * weights.iter().sum::<f64>().sqrt();
    // After model centering, at most one independent correction per partner except the last remains.
    for _ in 1..partners.len() {
        let mut shift = vec![0.0; rates.len()];
        for group in partners {
            let mean = group_mean(rates, weights, group);
            for &i in group {
                shift[i] = mean;
            }
        }
        center_models(&mut shift, weights, models);
        // Orthogonal corrections keep earlier centering work, even when a rare pair links two common pairs.
        for previous in &directions {
            remove_projection(&mut shift, previous, weights);
        }
        let norm = weighted_dot(&shift, &shift, weights).sqrt();
        if norm <= tolerance {
            break;
        }
        for value in &mut shift {
            *value /= norm;
        }
        remove_projection(rates, &shift, weights);
        directions.push(shift);
    }
}
