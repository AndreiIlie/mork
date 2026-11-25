// Keeping up: daily backups (2.8), pruning (2.9.2), and
// `logs/app.log`, rotated at 10 MB with five kept (1.3).

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use factory_core::maintenance::upkeep;
use factory_core::messages::places;
use factory_types::state::Agent;

use super::Engine;
use crate::paths::Home;
use crate::{clock, files};

const LOG_MAX: u64 = 10 * 1024 * 1024;

/// Appends a line to `logs/app.log`, rotating it.
pub fn log(home: &Home, line: &str) {
    let path = home.logs().join("app.log");
    files::rotate(&path, LOG_MAX, 5);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {line}", clock::now_ms());
    }
}

impl Engine {
    /// At most once a real second.
    pub(super) fn upkeep(&mut self) {
        if self.last_upkeep.is_some_and(|t| clock::elapsed(t) < Duration::from_secs(1)) {
            return;
        }
        self.last_upkeep = Some(clock::instant());
        let now = clock::now_ms();
        self.daily_backup(now);
        self.prune(now);
    }

    fn meta_ms(&self, key: &str) -> Option<i64> {
        self.store.meta(key).ok().flatten().and_then(|v| v.parse().ok())
    }

    fn daily_backup(&mut self, now: i64) {
        if !upkeep::daily_backup_due(self.meta_ms("last_daily_backup"), now) {
            return;
        }
        if let Err(e) = self.store.backup(&self.opts.home.backups(), now, "daily") {
            self.log(&format!("the daily backup failed: {e}"));
            return;
        }
        let _ = self.store.set_meta("last_daily_backup", &now.to_string());
        let dailies: Vec<(i64, std::path::PathBuf)> = std::fs::read_dir(self.opts.home.backups())
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let t = name.strip_suffix("-daily.db")?.rsplit('-').next()?.parse().ok()?;
                        Some((t, e.path()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let times: Vec<i64> = dailies.iter().map(|(t, _)| *t).collect();
        let gone = upkeep::dailies_to_remove(&times);
        for (t, p) in dailies {
            if gone.contains(&t) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    fn prune(&mut self, now: i64) {
        match upkeep::prune_step(self.meta_ms("last_prune"), now) {
            upkeep::Prune::Wait => return,
            upkeep::Prune::Start => {
                let _ = self.store.set_meta("last_prune", &now.to_string());
                return;
            }
            upkeep::Prune::Run => {}
        }
        let _ = self.store.set_meta("last_prune", &now.to_string());
        for id in self.store.settled_before(now - upkeep::KEEP_JOBS).unwrap_or_default() {
            let _ = std::fs::remove_dir_all(self.opts.home.job(&id));
        }
        self.prune_seat_ios(now);
        let Ok(files) = std::fs::read_dir(self.opts.home.door().join("files")) else { return };
        for entry in files.flatten() {
            let modified = entry.metadata().ok().filter(std::fs::Metadata::is_file).and_then(|m| modified_ms(&m));
            if modified.is_some_and(|t| t < now - upkeep::KEEP_JOBS) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    // A seat's `io/` folder goes once no live seat names it (an ended card's stay loaded till a restart), no unsettled
    // job writes it, and nothing in it changed for as long as a settled job's folder is kept (fac-654).
    fn prune_seat_ios(&self, now: i64) {
        let busy: BTreeSet<PathBuf> = (self.state.jobs.values().filter(|j| j.settled.is_none()))
            .filter_map(|j| j.spec.env.get("FACTORY_IO").map(PathBuf::from))
            .collect();
        let live = |a: &&Agent| a.retiring.is_some() || !places::ended(&self.state, a.place.as_str());
        let owned = |name: &OsStr| {
            self.state.agents.values().filter(live).any(|a| a.io.as_deref().is_some_and(|io| name == io))
        };
        let Ok(folders) = std::fs::read_dir(self.opts.home.seat_ios()) else { return };
        for entry in folders.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            if owned(&entry.file_name()) || busy.contains(&entry.path()) {
                continue;
            }
            if newest_ms(&self.opts.home.seat_ios(), &entry.file_name()) < now - upkeep::KEEP_JOBS {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

// The latest change in a seat's folder, its own or any entry's at any depth (fac-654), read through the folders the
// walk opened: no link is followed, whatever the seat swaps in, and a pending folder holds only its parent open.
fn newest_ms(seats: &Path, name: &OsStr) -> i64 {
    let mut newest = 0;
    let mut pending: Vec<(Rc<File>, OsString)> = Vec::new();
    let mut next = factory_sys::open_beneath(seats, Path::new(name)).ok();
    loop {
        if let Some(dir) = next.take().map(Rc::new) {
            newest = newest.max(dir.metadata().ok().and_then(|m| modified_ms(&m)).unwrap_or(0));
            for entry in factory_sys::read_dir_of(&dir).into_iter().flatten().flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                newest = newest.max(modified_ms(&meta).unwrap_or(0));
                if meta.is_dir() {
                    pending.push((Rc::clone(&dir), entry.file_name()));
                }
            }
        }
        let Some((parent, name)) = pending.pop() else { return newest };
        next = factory_sys::open_dir_beneath(&parent, Path::new(&name)).ok();
    }
}

fn modified_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs::File;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::newest_ms;

    // fac-654 review: a file far below a seat's folder keeps it as one at its top does, whatever its folders' times.
    #[test]
    fn fac654_a_seats_newest_change_is_read_at_any_depth() {
        let seats = std::env::temp_dir().join(format!("factory-test-seat-ios-{}", std::process::id()));
        let seat = seats.join("20200101-seat");
        let deep: PathBuf = (0..8).fold(seat.clone(), |p, n| p.join(format!("d{n}")));
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("shot.png"), "SHOT").unwrap();
        std::fs::write(seat.join("top.png"), "TOP").unwrap();
        let at = |p: &Path, secs: u64| {
            let t: SystemTime = UNIX_EPOCH + Duration::from_secs(secs);
            File::open(p).unwrap().set_modified(t).unwrap();
        };
        at(&deep.join("shot.png"), 1_500_000_000);
        at(&seat.join("top.png"), 1_000_000_000);
        deep.ancestors().take(9).for_each(|dir| at(dir, 1_000_000_000));
        let newest = || newest_ms(&seats, OsStr::new("20200101-seat"));
        assert_eq!(newest(), 1_500_000_000_000, "the file eight folders down is the newest change");
        at(&deep.join("shot.png"), 1_200_000_000);
        assert_eq!(newest(), 1_200_000_000_000);
        std::fs::remove_dir_all(&seats).unwrap();
    }
}
