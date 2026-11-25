//! Conversation activity derived from retained job streams (fac-138), never journaled.

use serde::{Deserialize, Serialize};

use crate::seen::TurnView;
use crate::view::MessageView;
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
