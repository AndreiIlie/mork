//! T1.45: after the suite and its production-only tests, no `t-*` folder and no
//! process naming one remains in the workspace's short namespace beneath HOME (fac-471).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// The short directory beneath HOME used by this workspace's test fixtures.
pub fn homes_dir(workspace: &Path) -> Result<PathBuf, String> {
    // Kept with fakes/src/home.rs::homes_dir: the crate graph forbids xtask depending on fakes (fac-471).
    let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
    let mut hash = DefaultHasher::new();
    workspace.hash(&mut hash);
    let home = std::env::var_os("HOME").ok_or("HOME is missing: no test fixture directory")?;
    Ok(PathBuf::from(home).join(format!(".t{:06x}", hash.finish() & 0x00ff_ffff)))
}

/// Test homes left in `tmp`.
pub fn leftover_homes(tmp: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(tmp) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("t-")))
        .collect();
    out.sort();
    out
}

/// Processes whose environment, working folder or command line names a test home in `tmp`.
pub fn leftover_processes(tmp: &Path) -> Vec<i32> {
    let needle = tmp.join("t-").display().to_string().into_bytes();
    let resolved = std::fs::canonicalize(tmp).ok().map(|p| p.join("t-").display().to_string().into_bytes());
    let me = std::process::id().cast_signed();
    let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&p| p != me && !zombie(p))
        .filter(|&p| {
            let has = |b: &[u8]| {
                std::iter::once(needle.as_slice())
                    .chain(resolved.as_deref())
                    .any(|n| b.windows(n.len()).any(|w| w == n))
            };
            std::fs::read(format!("/proc/{p}/environ")).is_ok_and(|b| has(&b))
                || std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|b| has(&b))
                || std::fs::read_link(format!("/proc/{p}/cwd")).is_ok_and(|c| has(c.as_os_str().as_encoded_bytes()))
        })
        .collect()
}

fn zombie(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s.rsplit_once(')').map(|(_, r)| r.trim_start().starts_with('Z')))
        .unwrap_or(true)
}

/// Every problem: each leftover home and process. A process still exiting gets up to 10 s.
pub fn check(tmp: &Path) -> Vec<String> {
    let mut out: Vec<String> =
        leftover_homes(tmp).into_iter().map(|p| format!("test home left: {}", p.display())).collect();
    let start = std::time::Instant::now();
    let mut left = leftover_processes(tmp);
    while !left.is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(200));
        left = leftover_processes(tmp);
    }
    out.extend(left.into_iter().map(|p| {
        let cmd = std::fs::read(format!("/proc/{p}/cmdline"))
            .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
            .unwrap_or_default();
        format!("process left naming a test home: pid {p}: {cmd}")
    }));
    out
}
