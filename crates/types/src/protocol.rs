//! The socket protocol (1.6): one JSON object per line, UTF-8, at most 1 MiB a line.

use serde::{Deserialize, Serialize};

/// The protocol's version, sent in `hello`.
pub const PROTOCOL: u32 = 1;

/// The longest line either side reads.
pub const MAX_LINE: usize = 1 << 20;

/// The engine's first message (1.6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub build: String,
    pub dirty: bool,
    pub protocol: u32,
    pub state_version: Option<u32>,
    pub home: String,
}

/// `{"hello": {…}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloLine {
    pub hello: Hello,
}

/// Why a command was refused (1.6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Invalid,
    Refused,
    Buzz,
    Leak,
    Internals,
    Recovering,
    Reloading,
    Replaced,
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// An answer: `{"id":…,"ok":{…}}` or `{"id":…,"err":{…}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Answer {
    Ok { id: u64, ok: serde_json::Value },
    Err { id: u64, err: ErrorBody },
}

/// A request's first parse: its id and its raw command, checked later by the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    pub cmd: serde_json::Value,
    /// Who the command is sent as: `orchestrator:<project>` on the operator's socket alone (fac-244); none, the socket's.
    #[serde(default, rename = "as", skip_serializing_if = "Option::is_none")]
    pub sent_as: Option<String>,
}

/// A web composer submission keeps the original words until Rust resolves its command and fields.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerRequest {
    pub id: u64,
    pub composer: ComposerDraft,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComposerDraft {
    pub context: crate::input::Context,
    pub text: String,
    pub images: Vec<String>,
    pub now: bool,
}

/// Local actions returned by the bridge. Engine commands keep their usual answer.
#[derive(Debug, Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum ComposerAction {
    Help { commands: Vec<(String, String)> },
    Open { card: String },
    Usage,
}

/// An incoming web socket line, distinguished by its existing outer keys.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Frame {
    Hello(HelloLine),
    Watch { event: WatchEvent },
    Answer(Answer),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchEvent {
    Screen(Box<crate::view::Screen>),
    View(WatchedPlace),
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum WatchedPlace {
    Full(Box<crate::view::PlaceView>),
    Messages(crate::chat::MessagesView),
}
