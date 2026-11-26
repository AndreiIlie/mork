//! The only place the time is read (1.12.2).

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The system time in UTC milliseconds since the epoch.
pub fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// A monotonic instant, for timeouts that keep real time (1.10.6).
pub fn instant() -> Instant {
    Instant::now()
}

/// Sleeps for real time.
pub fn sleep(d: Duration) {
    std::thread::sleep(d);
}

/// Real time passed since an instant (process-bounding waits keep real time, 1.10.6).
pub fn elapsed(since: Instant) -> Duration {
    since.elapsed()
}
