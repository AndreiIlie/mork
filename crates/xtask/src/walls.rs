//! fac-65: the merge train runs the suite in two phases. Walled, no inner wall can start: each
//! wall test skips and its binary records its name; unwalled, those tests alone run again.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::suite::Binary;

/// The train's phase, set by the engine on its run of Mork's own suite.
pub const PHASE: &str = "MORK_TRAIN_PHASE";
/// The file a test binary of the walled phase records its skipped wall tests in.
pub const RECORD: &str = "MORK_WALL_TESTS";

/// A phase of the train's run of the suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Inside the train's wall: the whole suite, its wall tests recorded as they skip.
    Walled,
    /// Outside every wall, after the walled phase passed: the recorded wall tests alone.
    Unwalled,
}

/// The phase `MORK_TRAIN_PHASE` names, an error for a name it can't be. None is an older engine's
/// train, or no train: the suite runs once as before, until fac-123 refuses it after fac-65's reload.
pub fn phase() -> Result<Option<Phase>, String> {
    let Some(val) = std::env::var_os(PHASE) else {
        return Ok(None);
    };
    match val.to_str() {
        Some("") => Ok(None),
        Some("walled") => Ok(Some(Phase::Walled)),
        Some("unwalled") => Ok(Some(Phase::Unwalled)),
        Some(other) => Err(format!("{PHASE}={other}: the train's phase is walled or unwalled")),
        None => Err(format!("{PHASE} is not valid UTF-8: the train's phase is walled or unwalled")),
    }
}

/// Where the walled phase's records are: in the candidate's target folder, which git ignores.
pub fn records(root: &Path) -> PathBuf {
    root.join("target/wall-tests")
}

/// The records' folder, emptied as the walled phase starts: a record is this run's alone.
pub fn fresh(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

// One file per test binary: the two passes' builds of a test have different file names.
fn record_of(dir: &Path, b: &Binary) -> PathBuf {
    dir.join(b.program.file_name().unwrap_or(b.program.as_os_str()))
}

/// The walled phase's binaries, each told the file it records its skipped wall tests in.
pub fn record(binaries: &mut [Binary], dir: &Path) {
    for b in binaries {
        let file = record_of(dir, b).display().to_string();
        b.env.push((RECORD.to_owned(), file));
    }
}

/// Selects recorded tests by exact name; missing folders, unreadable records or absent tests fail.
/// Otherwise an empty libtest selection could certify assertions that never ran (fac-65).
pub fn rerun(binaries: Vec<Binary>, dir: &Path) -> Result<Vec<Binary>, String> {
    if !dir.is_dir() {
        return Err(format!("{} is missing: the walled phase recorded nothing", dir.display()));
    }
    let mut out = Vec::new();
    for mut b in binaries {
        let file = record_of(dir, &b);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}: its wall tests' record can't be read", file.display())),
        };
        let names: BTreeSet<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if names.is_empty() {
            continue;
        }
        // An `--exact` name that matches no test runs nothing, and its binary still passes.
        let listed = crate::suite::listed(&b)?;
        if let Some(gone) = names.iter().find(|n| !listed.contains(**n)) {
            return Err(format!("{}: {gone} was recorded, and the rebuilt binary has no such test", b.name));
        }
        b.args = std::iter::once("--exact").chain(names).map(str::to_owned).collect();
        // MORK_UNWALLED=1 from the project's test command would skip these tests again.
        b.env.push(("MORK_UNWALLED".to_owned(), "0".to_owned()));
        out.push(b);
    }
    Ok(out)
}

/// How many wall tests the unwalled phase's binaries run, each by its exact name.
pub fn named(binaries: &[Binary]) -> usize {
    binaries.iter().map(|b| b.args.iter().filter(|a| *a != "--exact").count()).sum()
}
