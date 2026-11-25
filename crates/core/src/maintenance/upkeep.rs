//! The state's upkeep rules (section 2), as pure functions: daily backups, pruning.

// A day, in milliseconds.
const DAY: i64 = 86_400_000;
// Daily backups kept (2.8.1).
const DAILIES_KEPT: usize = 7;
/// Settled job folders are kept this long (2.9.2).
pub const KEEP_JOBS: i64 = 14 * DAY;

/// Whether a daily backup is due: none yet, or the last a day ago (2.8.1).
pub fn daily_backup_due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|t| now - t >= DAY)
}

/// Which daily backups go, by their times: all but the seven newest.
pub fn dailies_to_remove(times: &[i64]) -> Vec<i64> {
    let mut t = times.to_vec();
    t.sort_unstable();
    let n = t.len().saturating_sub(DAILIES_KEPT);
    t.truncate(n);
    t
}

/// What pruning does now (2.9.2): start counting, wait, or run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prune {
    /// The first look on this home: remember now, prune a day later.
    Start,
    Wait,
    Run,
}

/// Whether pruning runs, once a day.
pub fn prune_step(last: Option<i64>, now: i64) -> Prune {
    match last {
        None => Prune::Start,
        Some(t) if now - t < DAY => Prune::Wait,
        Some(_) => Prune::Run,
    }
}
