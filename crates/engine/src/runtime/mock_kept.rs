// A card's mock outlives its clone (fac-591): kept in Mork's home between runs, the way a handoff is kept, and
// put into the next run's clone.

use std::path::Path;

use mork_core::lifecycle::mock::FOLDER;
use mork_types::CardId;

use super::Engine;
use super::handoff::{preserve_folder, put_back, recorded, unrecord};
use crate::paths::Home;

/// The clone's mock copied to `mocks/<card>/` before the clone goes, replacing an older run's copy: its file count,
/// none when the clone has none. A failure keeps the clone, for the removal's retry, which reuses this run's whole copy.
pub(super) fn keep(home: &Home, card: &CardId, run: u32, clone: &Path) -> Result<Option<usize>, String> {
    let to = home.mocks().join(card.as_str());
    let failed = |e: std::io::Error| format!("{card}'s mock wasn't kept: {e}");
    // A retry's first try may have removed part of the clone's mock after recording it whole, and a clone the copy
    // never went into whole holds at most part of it: either way the copy is the card's whole mock.
    let reused = run_of(&to, RUN).map_err(failed)? == Some(run) || run_of(&to, RESTORED).map_err(failed)? != Some(run);
    if reused && let Some(kept) = recorded(&to).map_err(failed)? {
        return Ok(Some(kept.files.len()));
    }
    let kept = preserve_folder(clone, FOLDER, &to).map_err(failed)?;
    if kept.is_some() {
        crate::files::write_whole(&marker(&to, RUN), run.to_string().as_bytes()).map_err(failed)?;
    }
    Ok(kept.map(|k| k.files.len()))
}

// Beside the copy: `<card>.run`, the run whose removal recorded it, and `<card>.restored`, the run whose clone it
// went into whole.
const RUN: &str = "run";
const RESTORED: &str = "restored";

fn marker(to: &Path, which: &str) -> std::path::PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{which}"));
    to.with_file_name(name)
}

fn run_of(to: &Path, which: &str) -> std::io::Result<Option<u32>> {
    match std::fs::read_to_string(marker(to, which)) {
        Ok(run) => run.trim().parse().map(Some).map_err(std::io::Error::other),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn remove_marker(to: &Path, which: &str) -> std::io::Result<()> {
    std::fs::remove_file(marker(to, which))
        .or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
}

/// Invalidates the previous clone's recorded restore before a retry replaces that clone.
pub(super) fn forget_restore(home: &Home, card: &CardId) -> Result<(), String> {
    let aside = home.mocks().join(card.as_str());
    remove_marker(&aside, RESTORED).map_err(|e| format!("{card}'s previous mock restore wasn't cleared: {e}"))
}

/// The kept mock written into run `run`'s new clone: its file count, none when none is kept. The copy stays until
/// the put-back is recorded.
pub(super) fn put_into(home: &Home, card: &CardId, run: u32, clone: &Path) -> Result<Option<usize>, String> {
    let aside = home.mocks().join(card.as_str());
    let failed = |e: std::io::Error| format!("{card}'s kept mock wasn't put back: {e}");
    let Some(kept) = recorded(&aside).map_err(failed)? else { return Ok(None) };
    // Recovery may repeat a put-back whose clone job was never settled.
    remove_marker(&aside, RESTORED).map_err(failed)?;
    put_back(clone, FOLDER, &aside, &kept).map_err(failed)?;
    crate::files::write_whole(&marker(&aside, RESTORED), run.to_string().as_bytes()).map_err(failed)?;
    Ok(Some(kept.files.len()))
}

impl Engine {
    /// The kept copy dropped, its put-back recorded.
    pub(super) fn drop_kept_mock(&self, card: &CardId) {
        let aside = self.opts.home.mocks().join(card.as_str());
        // The copy goes first: one left without its markers would outlive the clone's later edits.
        let removed =
            unrecord(&aside).and_then(|()| remove_marker(&aside, RUN)).and_then(|()| remove_marker(&aside, RESTORED));
        if let Err(e) = removed {
            self.log(&format!("{}: the kept mock wasn't removed: {e}", aside.display()));
        }
    }
}
