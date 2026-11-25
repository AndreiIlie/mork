//! What the views show of work as it happens is never kept in the state (fac-96).

use serde::{Deserialize, Serialize};

use crate::JobId;

/// An agent's latest turn as its stream told it: each step with the text and thinking the stream
/// gave before it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnView {
    /// The turn's job: what the steps and their details are of.
    pub job: JobId,
    pub steps: Vec<StepView>,
    /// Messages read during the turn, before the step at each position; its length means after the last step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<ReadPoint>,
    /// What came after its last step: its closing text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<Detail>,
    /// Earlier steps or details were left out, to keep within bounds.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// How long the model worked after its last step with no tool running, when 1s or more
    /// (fac-206).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_ms: Option<i64>,
    /// Since when, in epoch ms, the model has worked with no tool running, while the turn runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_since: Option<i64>,
}

/// A delivered message's position among the turn's steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadPoint {
    pub seq: u64,
    pub step: usize,
}

/// A step and what the agent wrote and thought before it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepView {
    /// The stream's id for the step, unique in its turn.
    pub id: String,
    pub words: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Detail>,
    /// When it started and ended, in epoch ms, as the engine read them (fac-206): none from a job
    /// before then, and no end while it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<i64>,
    /// How long the model worked before it with no tool running, when 1s or more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_ms: Option<i64>,
}

/// An agent's own step list as its turn's stream left it (fac-128): its CLI's planner, Claude's
/// TaskCreate and TaskUpdate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepList {
    /// The turn's job: the list shows while it runs.
    pub job: JobId,
    pub steps: Vec<ListStep>,
}

/// An agent's step list, as its running turn keeps it, for a place's view (fac-128).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentList {
    pub agent: crate::AgentKey,
    pub steps: Vec<ListStep>,
}

/// A step of an agent's own list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListStep {
    /// Its planner's id for it.
    pub id: String,
    pub words: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

/// Text or thinking a turn's stream gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    /// Which it is in its turn, from where the stream gave it: the same however much before it is
    /// left out, its text cut, or its stream read again.
    pub id: String,
    pub kind: DetailKind,
    pub text: String,
    /// The text went on past what is kept.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// A subagent's, which its CLI ran inside the turn.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sub: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetailKind {
    Text,
    Thinking,
}

/// A file a card's agent turn produced in its seat's `io/` folder (fac-105, fac-654): neither an input the engine
/// supplied it nor a file its CLI's protocol writes there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFile {
    /// The folder's id under the home's `io/`, its seat's first turn's job id.
    pub job: JobId,
    pub agent: crate::AgentKey,
    /// Its path inside `io/`, with `/` between its folders.
    pub name: String,
    pub size: u64,
    pub modified_ms: i64,
}
