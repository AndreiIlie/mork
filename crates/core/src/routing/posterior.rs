use super::{CLAMP, PAR, PRIOR, RELIABLE};
use mork_types::config::bench;
use mork_types::routing::{Arm, Estimate, Metric, Statistics};
use serde::Serialize;

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
        let global = shrink(bounded(prior), data.map(|l| l.global).unwrap_or_default());
        let counts = data.and_then(|l| l.leaves.get(leaf?)).copied().unwrap_or_default();
        let local = if leaf.is_some() { shrink(bounded(global.mean), counts) } else { global };
        [global, local]
    }

    pub fn synergy(&self, model: &str, partner: Option<&str>) -> f64 {
        let Some(partner) = partner else { return 0.0 };
        let Some(pair) = self.stats.partners.get(&(self.role.to_owned(), model.to_owned(), partner.to_owned())) else {
            return 0.0;
        };
        let mut own = Estimate::default();
        if let Some(arms) = self.stats.outcomes.get(&(self.role.to_owned(), super::metric(self.role))) {
            for (_, counts) in arms.iter().filter(|(a, _)| a.model == model) {
                own.add(counts.global.sum, counts.global.weight);
            }
        }
        pair.weight / (pair.weight + PRIOR) * (pair.mean(PAR) - own.mean(PAR))
    }
}
