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
}

/// The words shared by the command, card and Projects tab.
pub struct ReadSpec {
    pub name: &'static str,
    pub button: &'static str,
    pub detail: &'static str,
}

impl Read {
    /// The order of reads in the Projects tab.
    pub const ALL: [Self; 2] = [Self::Pulse, Self::DeepSweep];

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

/// A report's fixed read window; `through` is filled when its clone's commit is selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub read: Read,
    pub from: Option<Sha>,
    pub since_ms: Option<i64>,
    pub ended_since_ms: Option<i64>,
    pub through: Option<Sha>,
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
