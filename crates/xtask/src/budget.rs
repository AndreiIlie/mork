//! T6.8's time: the suite's pass, built first, runs in under 15 minutes on the cloud VM's 4
//! cores (6.3).

use std::time::Duration;

/// The pass's run, its build left out.
pub const SUITE: Duration = Duration::from_secs(15 * 60);

/// The unwalled phase's two passes together, their builds left out (fac-170).
pub const WALLS: Duration = Duration::from_secs(3 * 60);

/// Whether the pass's run took less than [`SUITE`]; why not, when it didn't.
pub fn within(elapsed: Duration) -> Result<(), String> {
    if elapsed < SUITE {
        Ok(())
    } else {
        Err(format!(
            "the suite took {}m{}s, over the 15 minutes of T6.8",
            elapsed.as_secs() / 60,
            elapsed.as_secs() % 60
        ))
    }
}

/// Whether the unwalled phase's passes together took less than [`WALLS`]; why not, when they didn't.
pub fn walls_within(elapsed: Duration) -> Result<(), String> {
    if elapsed < WALLS {
        Ok(())
    } else {
        Err(format!(
            "the unwalled phase's passes took {}m{}s, over its 3 minutes (fac-170)",
            elapsed.as_secs() / 60,
            elapsed.as_secs() % 60
        ))
    }
}
