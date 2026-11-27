//! Conversation activity derived from retained job streams (fac-138), never journaled.

use serde::{Deserialize, Serialize};

use crate::reply::{Finding, Hand as ReplyHand, Role};
use crate::seen::TurnView;
use crate::{AgentKey, PlaceId};

/// One turn's activity, after the message preceding its start in this place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub agent: AgentKey,
    pub after_seq: u64,
    pub running: bool,
    pub step_count: usize,
    pub turn: TurnView,
}

/// A page of a place's messages before one, oldest first, with its retained turn activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub place: PlaceId,
    pub messages: Vec<MessageView>,
    pub earlier: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<ChatTurn>,
}

/// A place's conversation alone, what a `watch` without `full` sends (fac-280): the place, its
/// revision, its latest messages, the earlier ones whose ask is open, and how many came before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessagesView {
    pub place: PlaceId,
    pub revision: u64,
    pub messages: Vec<MessageView>,
    pub earlier: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<MessageView>,
}

/// A dense activity line, with whitespace between words rather than newlines or indentation.
pub fn line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How long a call took, as the chat says it (fac-206): `<1s` under a second, `12s`, then
/// `1m 05s` past a minute.
pub fn took(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    match secs {
        0 => "<1s".to_owned(),
        1..60 => format!("{secs}s"),
        _ => format!("{}m {:02}s", secs / 60, secs % 60),
    }
}

/// One message, as the conversation draws it (5.7.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageView {
    /// Present while unread. True when the recipient can be interrupted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<bool>,
    pub seq: u64,
    pub time_ms: i64,
    pub from: Author,
    pub to: Option<String>,
    pub kind: Line,
    pub headline: String,
    /// The text: a marker's shows only unfolded under its divider (5.7.5, fac-384).
    pub text: String,
    /// The text was cut to keep the view within its line (M1.16).
    pub cut: bool,
    /// The operator's messages in main are numbered.
    pub number: Option<u64>,
    pub hand: Option<Box<ReplyHand>>,
    pub findings: Vec<Finding>,
    /// The images it carries, drawn as thumbnails (fac-31). A view made before them has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageView>,
    /// Its hand's ask is still open: not answered, nor replaced by a newer one (3.5.5, fac-39).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_ask: Option<crate::AskId>,
    /// The option of a seat's ask this answer chose, by its number from 1 (fac-696).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<u32>,
}

/// An image a message carries (fac-696): its path outside any wall, and where the web bridge serves it, for a job's
/// published copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageView {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Who wrote it, for its colour (5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum Author {
    Operator,
    Engine,
    /// A project's outside orchestrator (fac-244).
    Orchestrator {
        project: String,
    },
    /// The operator's outside PA, on what it sent before the orchestrators replaced it (fac-119, fac-244).
    Pa,
    Agent {
        label: String,
        role: Role,
    },
}

/// How a message draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Line {
    /// An engine message to an agent: a thin divider, its label and time, its text unfolding under it.
    Marker,
    /// An engine message to the operator or to everyone in a place: whole, or one line that unfolds
    /// when long (fac-39).
    Engine,
    /// The operator's words: whole, bright.
    Operator,
    /// An agent's message: its words to the operator whole, to another agent one line that unfolds.
    Agent,
}

#[derive(Debug, Serialize)]
pub struct HistoryAnswer {
    pub history: HistoryPage,
}
