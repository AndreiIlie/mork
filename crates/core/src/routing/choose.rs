use mork_types::config::{PickModel, Seat, SeatPhase};
use mork_types::routing::{Arm, Metric, Statistics};
use mork_types::usage::Limit;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::posterior::{Population, Posterior};
use super::sample::Draw;
use super::{PRICE, metric};
use crate::agents::pools::{self, Offer};

pub struct Request<'a> {
    pub agent: &'a str,
    pub run: u32,
    pub phase: SeatPhase,
    pub role: &'a str,
    pub leaf: Option<&'a str>,
    pub partner: Option<&'a str>,
}

#[derive(Clone, Serialize)]
pub struct Candidate {
    #[serde(flatten)]
    pub offer: Offer,
    pub mean: f64,
    pub draw: f64,
    pub cost: f64,
    pub relative_cost: f64,
    pub synergy: f64,
    pub price: f64,
    pub score: f64,
}

impl Candidate {
    pub fn arm(&self) -> Arm {
        Arm { cli: self.offer.cli, model: self.offer.model.clone(), effort: self.offer.effort }
    }
}

#[derive(Clone)]
pub struct Recommendation {
    pub candidates: Vec<Candidate>,
    pub winner: Option<usize>,
}

impl Recommendation {
    pub fn best(&self) -> Option<&Candidate> {
        self.candidates.get(self.winner?)
    }
    pub fn row(&self, req: &Request<'_>, mode: mork_types::config::RoutingMode) -> Value {
        json!({"agent":req.agent,"run":req.run,"role":req.role,"leaf":req.leaf,"partner":req.partner,"mode":mode,
            "candidates":self.candidates,"pick":self.best().map(Candidate::arm)})
    }
}

pub struct Inputs<'a> {
    pub limits: &'a BTreeMap<String, Limit>,
    pub now: i64,
    pub account: Option<&'a str>,
    pub stats: Option<&'a Statistics>,
}

impl<'a> Inputs<'a> {
    pub fn new(limits: &'a BTreeMap<String, Limit>, ctx: crate::decide::Ctx<'a>) -> Self {
        Self { limits, now: ctx.now, account: ctx.claude_account, stats: ctx.routing }
    }
}

impl Inputs<'_> {
    pub fn choose(&self, req: &Request<'_>, seat: &Seat, list: &[PickModel], need_spare: bool) -> Recommendation {
        let offers = pools::offers(self.limits, self.now, self.account, seat, list);
        let mean_index = mean_index(&offers);
        let empty = Statistics::default();
        let pop = Population { stats: self.stats.unwrap_or(&empty), role: req.role, mean_index };
        let mut rng = Draw::new(&format!("{}:{}:{:?}", req.agent, req.run, req.phase));
        let mut candidates: Vec<_> = offers.into_iter().map(|offer| candidate(offer, req, &pop, &mut rng)).collect();
        let mean_cost = candidates.iter().map(|c| c.cost * c.offer.scale).sum::<f64>() / candidates.len().max(1) as f64;
        let mut winner = None;
        let mut best = f64::NEG_INFINITY;
        for (i, c) in candidates.iter_mut().enumerate() {
            c.relative_cost = if mean_cost > 0.0 { c.cost * c.offer.scale / mean_cost } else { 1.0 };
            c.score = c.draw + c.synergy - c.price * c.relative_cost;
            if c.offer.open && (!need_spare || c.offer.spare > 0) && c.score > best {
                winner = Some(i);
                best = c.score;
            }
        }
        Recommendation { candidates, winner }
    }
}

pub(super) fn mean_index(offers: &[Offer]) -> f64 {
    if offers.is_empty() {
        return 1.0;
    }
    offers.iter().map(|o| o.index).sum::<f64>() / offers.len() as f64
}

fn candidate(offer: Offer, req: &Request<'_>, pop: &Population<'_>, rng: &mut Draw) -> Candidate {
    let arm = Arm { cli: offer.cli, model: offer.model.clone(), effort: offer.effort };
    let metric = metric(req.role);
    let outcome = pop.levels(metric, &arm, req.leaf)[1];
    let sample = |p: Posterior, rng: &mut Draw| rng.beta(p.mean, p.weight);
    let (mut mean, mut draw) = (outcome.mean, sample(outcome, rng));
    if metric == Metric::Catch {
        let precision = pop.levels(Metric::Precision, &arm, req.leaf)[1];
        mean *= precision.mean;
        draw *= sample(precision, rng);
    } else if metric == Metric::Reliability {
        mean *= offer.index / 100.0;
        draw *= offer.index / 100.0;
    }
    let cost = pop.levels(Metric::Cost, &arm, req.leaf)[1].mean;
    let synergy = pop.synergy(&arm.model, req.partner);
    let price = price(offer.spare);
    Candidate { offer, mean, draw, cost, synergy, price, relative_cost: 0.0, score: 0.0 }
}

pub(super) fn price(spare: u32) -> f64 {
    PRICE * (1.0 - f64::from(spare) / 100.0)
}
