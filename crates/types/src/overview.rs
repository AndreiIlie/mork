//! Per-card Overview stage spine carried on the card room's `PlaceView` (fac-684).

use serde::{Deserialize, Serialize};

use crate::view::MessageView;

/// Structured kind of an Overview stage row (fac-698).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverviewStageKind {
    Spec,
    Build,
    Verify,
    Land,
    Report,
    Review,
}

/// Where a stage milestone stands in the Overview spine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverviewStageAt {
    Passed,
    Failed,
    Current,
}

/// One human-facing message in the Overview, tagged with the room it came from (`Spec`, `Build`, `Review`, `Report`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewLine {
    pub room: String,
    pub message: MessageView,
    /// An answered ask's answer (fac-691): Andrei's or the orchestrator's line, folded into its ask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<MessageView>,
}

/// One milestone in the card's Overview spine: its kind, state, window, summary, count, lines, and round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewStage {
    pub kind: OverviewStageKind,
    pub label: String,
    pub at: OverviewStageAt,
    pub start_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<OverviewLine>,
    /// A round stage's verdict, commit, and named findings with proofs (fac-698).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<crate::review::RoundView>,
}

/// Per-card Overview stream of stages in run order, built in `core` for the web Card Room.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewView {
    pub stages: Vec<OverviewStage>,
    /// The current stage's live rows, after its lines (fac-691).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub live: Vec<LiveRow>,
}

/// One live row under the current stage (fac-691): a verification step or a working seat, its words and its time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRow {
    /// `Checks`, `Suite`, `Review R<n>`, or a seat's name.
    pub name: String,
    /// A reviewer's topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub words: String,
    /// It runs now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub working: bool,
    /// A reviewer under its round's row.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub under: bool,
    /// It names a seat.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub seat: bool,
    /// When it started, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<i64>,
    /// The running suite's counts so far, when its output gives them (fac-697).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<SuiteProgress>,
}

/// What a running suite's output has counted so far (fac-697): tests, or a Jest suite's test files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteProgress {
    pub passed: u32,
    pub failed: u32,
    /// How many it runs in all, when its runner said so up front.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// It counts test files rather than tests.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub files: bool,
    /// The test, or file, that ended last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
}
