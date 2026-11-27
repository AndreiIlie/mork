use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Cli, Effort};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Arm {
    pub cli: Cli,
    pub model: String,
    pub effort: Effort,
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Estimate {
    pub sum: f64,
    pub weight: f64,
}

impl Estimate {
    pub fn add(&mut self, sum: f64, weight: f64) {
        self.sum += sum;
        self.weight += weight;
    }

    pub fn mean(self, fallback: f64) -> f64 {
        if self.weight > 0.0 { self.sum / self.weight } else { fallback }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Levels {
    pub global: Estimate,
    pub leaves: BTreeMap<String, Estimate>,
}

#[derive(Debug, Default)]
pub struct Statistics {
    pub outcomes: BTreeMap<(String, Metric), BTreeMap<Arm, Levels>>,
    pub partners: BTreeMap<(String, String, String), Estimate>,
    pub recent: Vec<(i64, Value)>,
    pub shadow: BTreeMap<String, [usize; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    Quality,
    Catch,
    Precision,
    Reliability,
    Cost,
}
