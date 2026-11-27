pub mod choose;
pub mod observe;
mod posterior;
pub mod report;
mod sample;
pub(crate) mod seats;
#[cfg(test)]
mod tests;

use mork_types::routing::Metric;

const PRIOR: f64 = 7.0;
const HALF_LIFE: f64 = 30.0 * 86_400_000.0;
pub const WINDOW: i64 = 90 * 86_400_000;
const PRICE: f64 = 0.10;
const OVERRIDE: f64 = 2.0;
const PLANNER: [f64; 3] = [0.25, 0.25, 0.5];
const PAR: f64 = 0.5;
const RELIABLE: f64 = 0.9;
const CLAMP: (f64, f64) = (0.05, 0.95);
const LENSES: &[(&str, &[&str])] = &[
    ("correctness", &["correct", "test"]),
    ("design", &["design", "simpl"]),
    ("integrity", &["money", "permission", "integrity", "security"]),
    ("performance", &["perform"]),
];

fn lens(topic: &str) -> &str {
    let lower = topic.to_lowercase();
    LENSES.iter().find(|(_, words)| words.iter().any(|w| lower.contains(w))).map_or("other", |r| r.0)
}

fn metric(role: &str) -> Metric {
    match role {
        "foreman.work" | "planner" => Metric::Quality,
        "reviewer" => Metric::Catch,
        _ => Metric::Reliability,
    }
}
