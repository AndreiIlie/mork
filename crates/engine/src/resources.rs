//! The machine's own load, read from `/proc` (1.12.2): every process on it counts, Mork's or not.

use std::fs;
use std::time::Duration;

use mork_types::view::{Ram, Resources};

use crate::clock;

const EVERY: Duration = Duration::from_secs(2);
const FIRST: Duration = Duration::from_millis(500);

/// Samples the machine each couple of seconds, handing each reading to `send` until it says no.
/// Meant for a thread of its own: the CPU's share is the change between two reads.
pub fn watch(mut send: impl FnMut(Resources) -> bool) {
    let mut before = jiffies();
    let mut wait = FIRST;
    loop {
        clock::sleep(wait);
        wait = EVERY;
        let now = jiffies();
        let cpu_percent = before.zip(now).and_then(|(a, b)| share(a, b));
        before = now;
        if !send(Resources { cpu_percent, ram: ram() }) {
            return;
        }
    }
}

// Busy and total clock ticks of every core since boot (`/proc/stat`'s first line).
fn jiffies() -> Option<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let fields: Vec<u64> =
        stat.lines().next()?.strip_prefix("cpu ")?.split_whitespace().map_while(|f| f.parse().ok()).collect();
    // user nice system idle iowait irq softirq steal: guest time is already inside user's.
    let all: u64 = fields.get(..8)?.iter().sum();
    let idle = fields[3] + fields[4];
    Some((all - idle, all))
}

fn share((busy_a, all_a): (u64, u64), (busy_b, all_b): (u64, u64)) -> Option<u8> {
    let all = all_b.checked_sub(all_a).filter(|a| *a > 0)?;
    let busy = busy_b.saturating_sub(busy_a).min(all);
    u8::try_from((busy * 100 + all / 2) / all).ok()
}

fn ram() -> Option<Ram> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let kib = |name: &str| -> Option<u64> {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':')?.trim().strip_suffix("kB")?.trim().parse().ok())
    };
    let (total, available) = (kib("MemTotal")?, kib("MemAvailable")?);
    Some(Ram { used_mib: total.saturating_sub(available) / 1024, total_mib: total / 1024 })
}
