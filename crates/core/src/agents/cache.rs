//! Assumed prompt-cache lifetimes per CLI and seat warmth checks (fac-670).

use factory_types::config::{Cli, Config};
use factory_types::state::Agent;

const MINUTE_MS: i64 = 60_000;
/// Assumed Claude prompt-cache lifetime in ms.
pub const CLAUDE_MS: i64 = 20 * MINUTE_MS;
/// Assumed Codex prompt-cache lifetime in ms.
pub const CODEX_MS: i64 = 60 * MINUTE_MS;
/// Assumed agy prompt-cache lifetime in ms.
pub const AGY_MS: i64 = 20 * MINUTE_MS;

/// Assumed prompt-cache lifetime in ms for `cli`.
pub const fn lifetime_ms(cli: Cli) -> i64 {
    match cli {
        Cli::Claude => CLAUDE_MS,
        Cli::Codex => CODEX_MS,
        Cli::Agy => AGY_MS,
    }
}

/// Epoch ms when `a`'s prompt cache expires, from its last stamped turn and its session's CLI.
pub fn cold_at(config: &Config, a: &Agent) -> Option<i64> {
    let last = a.last_turn_ms?;
    let seat = super::models::seat(config, a)?;
    Some(last.saturating_add(lifetime_ms(seat.cli)))
}

/// Whether `a`'s prompt cache is warm: a turn of its own is running, or `now` is before expiry.
pub fn warm(config: &Config, a: &Agent, running: bool, now: i64) -> bool {
    running || cold_at(config, a).is_some_and(|at| now < at)
}
