//! Cold-start estimates from the Artificial Analysis Intelligence Index v5, November 2026.

use super::seats::{ASTRA, NEON, SONNET};
use super::{Cli, Effort, OPUS, SOL, agy_bucket};

/// Reasoning index and USD per task at the given effort, shared across providers of the same model.
pub fn benchmark(model: &str, effort: Effort) -> Option<(f64, f64)> {
    let ladder: &[(f64, f64)] = match model {
        "claude-fable-5-2" => &[(53.89, 3.10), (56.32, 3.80), (61.87, 9.08), (62.22, 14.83), (64.77, 26.54)],
        OPUS => &[(34.75, 0.98), (48.26, 2.45), (51.79, 3.46), (57.52, 6.05), (59.12, 10.04)],
        SONNET => &[(25.60, 0.51), (30.87, 0.78), (41.48, 1.64), (49.87, 4.09), (54.73, 10.02)],
        ASTRA => &[(45.65, 0.66), (57.05, 1.62), (60.29, 2.07), (61.76, 2.52), (62.19, 4.00)],
        SOL => &[(40.55, 0.18), (52.43, 0.41), (54.84, 0.56), (55.63, 0.65), (57.26, 1.01)],
        NEON => &[(44.08, 0.88), (54.91, 1.90), (59.82, 3.74), (61.55, 6.44)],
        _ => return None,
    };
    ladder.get(Effort::ALL.iter().position(|e| *e == effort)?).copied()
}

/// Converts benchmark dollars to spare percentage points for the quota pool a model spends.
pub fn cost_scale(cli: Cli, model: &str) -> f64 {
    match cli {
        Cli::Codex => 13.0,
        Cli::Agy if agy_bucket(model) == "account" => 2.0,
        Cli::Claude | Cli::Agy => 3.5,
    }
}
