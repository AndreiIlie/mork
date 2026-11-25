// A failed train run's whole log is copied into the clone of each card its news reaches (fac-164), where that
// card's seats can read it and git sees nothing: `<clone>/.git/factory/suite-logs/<job>.out`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use factory_types::CardId;
use factory_types::card::Card;

use super::Engine;

impl Engine {
    // The copies of the run's log, by card: its own card's, and our card's that holds the PR it ran.
    pub(super) fn suite_logs(&self, c: &Card, log: &Path, job: &str) -> BTreeMap<CardId, String> {
        let ours = c.pr.and_then(|pr| factory_core::queue::approval::owner(&self.state, c.project.as_str(), pr));
        let cards = std::iter::once(&c.id).chain(ours.map(|o| &o.id));
        cards.filter_map(|id| Some((id.clone(), copied(&self.clone_dir(id.as_str()), log, job)?))).collect()
    }
}

// The log copied under the clone's `.git/`; none when the card has no clone or the copy fails.
// Every step stays beneath the clone through no link an agent planted there (R9).
fn copied(clone: &Path, log: &Path, job: &str) -> Option<String> {
    let dir = Path::new(".git/factory/suite-logs");
    for folder in [Path::new(".git/factory"), dir] {
        factory_sys::mkdir_beneath(clone, folder).ok()?;
    }
    let file: PathBuf = dir.join(format!("{job}.out"));
    factory_sys::write_beneath(clone, &file, &std::fs::read(log).ok()?).ok()?;
    Some(clone.join(file).display().to_string())
}
