//! A card's pipeline strip, stage times, and header facts (5.7.2, fac-680).

use serde::{Deserialize, Serialize};

use crate::CardId;
use crate::card::Mode;

/// Structured step kind for a card's pipeline strip (fac-698).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageKind {
    Spec,
    Build,
    Verify,
    Land,
    Report,
    Review,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageMark {
    pub kind: StageKind,
    /// Step label: `Spec`, `Build`, `Verify`, `Land`, `Report`, `Review`, or `Done`.
    pub label: String,
    pub at: StageAt,
    /// How many times it ran, from 1.
    pub runs: u32,
    /// Step count (`11/12`, `R1`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<String>,
    /// Live inline sub-status (`Mock`, `checks`, `2 reviewers working · suite running`, `PR #1184 waits for your approval`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageAt {
    Passed,
    Current,
    ToCome,
}

/// The timeline's totals (4.19), in ms.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Times {
    /// Each step's time, by its label, in the pipeline's order (`Spec`, `Build`, `Verify`, `Land`).
    pub stages: Vec<(String, i64)>,
    pub waiting_on_you: i64,
    pub paused: i64,
}

/// A fact of the facts line (5.7.3), in its order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fact", rename_all = "kebab-case")]
pub enum Fact {
    Head { head: String },
    JoinOwed { base: String, tip: String, asked: bool },
    BaseAhead { base: String, commits: u32 },
    Overlap { card: CardId, files: usize },
    Mode { mode: Mode },
    Run { run: u32 },
    Carries { cards: Vec<CardId> },
}
