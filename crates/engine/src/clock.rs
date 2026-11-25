//! The only place the time is read (1.12.2). With the `faults` feature, `FACTORY_TEST_CLOCK`
//! names a file holding the time the engine reads as now (1.10.6).

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Now, in UTC milliseconds since the epoch: the test clock's when one is set.
pub fn now_ms() -> i64 {
    if let Some(ms) = test_clock() {
        return ms;
    }
    real_now_ms()
}

/// The system clock, ignoring any test clock: for waits that bound a process.
pub fn real_now_ms() -> i64 {
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

// The test clock's last good read: a read that meets the file being rewritten gives this
// rather than the real time, so a test clock never jumps to now.
static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

fn test_clock() -> Option<i64> {
    let path = crate::faults::test_env("FACTORY_TEST_CLOCK")?;
    for _ in 0..20 {
        if let Some(ms) = std::fs::read_to_string(&path).ok().and_then(|t| t.trim().parse::<i64>().ok()) {
            LAST.store(ms, std::sync::atomic::Ordering::SeqCst);
            return Some(ms);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let last = LAST.load(std::sync::atomic::Ordering::SeqCst);
    (last != 0).then_some(last)
}

/// Real time passed since an instant (process-bounding waits keep real time, 1.10.6).
pub fn elapsed(since: Instant) -> Duration {
    since.elapsed()
}
