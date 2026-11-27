//! A project's look-back: the read a report owns and the cursor its completion leaves.

use serde::{Deserialize, Serialize};

use crate::{CardId, ProjectId, Sha};

/// The engine's fixed inputs in a look-back clone.
pub const INPUT_FILE: &str = ".git/mork/lookback.json";

/// The scope of one look-back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Read {
    Pulse,
    DeepSweep,
    Dream,
}

/// The words shared by the command, card and Projects tab.
pub struct ReadSpec {
    pub name: &'static str,
    pub button: &'static str,
    pub detail: &'static str,
}

impl Read {
    /// The order of reads in the Projects tab.
    pub const ALL: [Self; 3] = [Self::Pulse, Self::DeepSweep, Self::Dream];

    /// Finds a read by its command name.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|read| read.spec().name == name)
    }

    /// The name and Projects copy of this read.
    pub const fn spec(self) -> ReadSpec {
        let (name, button, detail) = match self {
            Self::Pulse => (
                "pulse",
                "Look back: pulse",
                "Reads what landed and the cards that ended since its last run, with the docs and open cards they touch. Run it weekly.",
            ),
            Self::DeepSweep => (
                "deep-sweep",
                "Look back: deep sweep",
                "Reads the whole code for dead code, dead tests, risks and wording. Run it monthly.",
            ),
            Self::Dream => (
                "dream",
                "Look back: dream",
                "Consolidates project memory against the current code and folds lesson notes.",
            ),
        };
        ReadSpec { name, button, detail }
    }
}

/// A project's last completed run of one read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub project: ProjectId,
    pub commit: Option<Sha>,
    #[serde(default)]
    pub started_ms: Option<i64>,
    pub ended_ms: i64,
    pub read: Read,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    #[default]
    Manual,
    Scheduled,
    Auto,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PassConfig {
    pub mode: Trigger,
    pub landings: Option<u64>,
    pub edits: Option<u64>,
    pub notes: Option<u64>,
    pub days: Option<u64>,
    pub budget: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub landings: u64,
    pub memory_edits: u64,
    pub note: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    pub project: ProjectId,
    pub read: Read,
    pub at_ms: i64,
    pub counts: Counts,
}

/// A report's fixed read window; `through` is filled when its clone's commit is selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub read: Read,
    pub from: Option<Sha>,
    pub since_ms: Option<i64>,
    pub ended_since_ms: Option<i64>,
    pub through: Option<Sha>,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default)]
    pub at_start: Option<Counts>,
    #[serde(default)]
    pub own_edits: u64,
    #[serde(default)]
    pub changed: std::collections::BTreeSet<String>,
}

/// One configured project's look-back in main, whether or not it has run before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectView {
    pub project: ProjectId,
    pub rows: Vec<ReadView>,
}

/// A row's last completion and, for the active read, its live card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadView {
    pub mode: Trigger,
    pub read: Read,
    pub last: Option<LastView>,
    pub running: Option<CardId>,
}

/// A completed run's local date and time, with its instant for the window's elapsed duration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastView {
    pub date: String,
    pub ended_ms: i64,
    pub commits: Option<u32>,
}
