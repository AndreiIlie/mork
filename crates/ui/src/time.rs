//! Times and durations as the screen shows them (5.12).

const MINUTE: i64 = 60_000;
const DAY: i64 = 24 * 60 * MINUTE;

/// A time, local, as `HH:MM`, 24-hour: epoch ms plus the local offset in minutes.
pub fn hhmm(ms: i64, offset_min: i32) -> String {
    let local = ms.saturating_add(i64::from(offset_min) * MINUTE);
    let minutes = local.rem_euclid(DAY) / MINUTE;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// A duration: `1m12s` under ten minutes, else `41m`, or `3h12m`. Whole minutes drop their
/// seconds (`3m`), and under a minute it is seconds (`45s`).
pub fn duration(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match (h, m, s) {
        (0, 0, 0) => "0m".to_owned(),
        (0, 0, s) => format!("{s}s"),
        (0, m, 0) => format!("{m}m"),
        (0, m, s) if m < 10 => format!("{m}m{s}s"),
        (0, m, _) => format!("{m}m"),
        (h, 0, _) => format!("{h}h"),
        (h, m, _) => format!("{h}h{m}m"),
    }
}

/// The time left until a window resets: `2h30m12s`, `25m12s`, `12s` to the second, or `3d4h25m`,
/// `4h25m`, `25m` to the minute. Every unit under the first one shows, zero or not.
pub fn countdown(ms: i64, to_the_minute: bool) -> String {
    let secs = ms.max(0) / 1000;
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if to_the_minute {
        return match (d, h) {
            (0, 0) => format!("{m}m"),
            (0, h) => format!("{h}h{m}m"),
            (d, h) => format!("{d}d{h}h{m}m"),
        };
    }
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, m) => format!("{m}m{s}s"),
        (0, h, m) => format!("{h}h{m}m{s}s"),
        (d, h, m) => format!("{d}d{h}h{m}m{s}s"),
    }
}
