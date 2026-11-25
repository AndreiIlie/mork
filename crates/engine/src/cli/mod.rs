//! The CLIs (1.4.8): how `claude`, `codex` and `agy` are called, and one parser per stream format
//! (1.12.3).

pub mod agy;
pub mod appserver;
pub mod claude;
pub mod codex;
pub mod history;
pub mod limits;
pub mod live;
pub mod said;
pub mod turn;
mod usage;

use serde_json::Value;

/// What a stream showed, read line by line. Reading it twice changes nothing (1.4.7.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamState {
    /// The Claude account captured before this job started, retained across engine restarts.
    pub account: Option<factory_types::usage::Account>,
    /// Claude's `system/init`, Codex's `thread.started` or agy's `init` was seen.
    pub session_started: bool,
    /// Codex's thread id, or agy's conversation id.
    pub thread: Option<String>,
    /// The complete result, once seen: the reply, or the turn's error. A reply the CLI gave
    /// that isn't JSON is its words, a string.
    pub result: Option<Result<Value, String>>,
    /// Claude's session cost so far (`total_cost_usd`), when reported.
    pub cost_usd: Option<f64>,
    /// Running usage by session or Codex thread, including child threads (fac-526).
    pub token_totals: std::collections::BTreeMap<String, factory_types::cost::Tokens>,
    /// A `rate_limit_event` with status `rejected` was seen: its reset, in epoch seconds, when
    /// known.
    pub limit_rejected: Option<Option<i64>>,
    /// The steps, in words, once each (1.4.8, Claude 8; Codex 4).
    pub steps: Vec<Step>,
    /// What the seat says it is doing until its next step (Codex's commentary).
    pub doing: Option<String>,
    /// What the context window holds now (Claude 9, fac-681); unknown for Codex.
    pub context: Option<ContextReport>,
    /// The model reported by agy's `init` event, attached to its context reports (fac-681).
    pub model: Option<String>,
    /// The usage-limit windows the stream reported, latest first per window.
    pub limits: Vec<limits::Window>,
    /// The seat's folder and the agent home, for shortening paths.
    pub folder: String,
    pub home: String,
    /// The job's agent: a foreman's or planner's word to the other shows as a step without its text (fac-633).
    pub seat: Option<String>,
    /// Claude's latest main-thread call: its message id, its count and its prompt part.
    pub last_call: Option<(String, u64, u64)>,
    /// Each model's context window, from a result's `modelUsage`.
    pub windows: std::collections::BTreeMap<String, u64>,
    /// Text and thinking waiting for the next step of their thread: the turn's own (none), or a
    /// subagent's, by the tool call that ran it (fac-96).
    pub pending: std::collections::BTreeMap<Option<String>, Vec<factory_types::seen::Detail>>,
    /// Each detail's identity in the stream, so a line read twice adds none.
    pub detail_keys: std::collections::BTreeSet<String>,
    /// Details were left out, to keep within `turn::DETAILS_MAX`.
    pub details_cut: bool,
    /// The agent's own step list, its steps in the order they were made (fac-128).
    pub list: Vec<factory_types::seen::ListStep>,
    /// The planner's calls waiting for their results, by tool call: its name and its input.
    pub planner_calls: std::collections::BTreeMap<String, (String, Value)>,
    /// When the engine read the line in hand, in epoch ms; none when no read of it was stamped
    /// (fac-206).
    pub at: Option<i64>,
    /// When the turn started (Claude's `system/init`, Codex's `thread.started`) and ended (its
    /// result), as read.
    pub started_ms: Option<i64>,
    pub ended_ms: Option<i64>,
    /// Messages delivered into the turn, by id, that reached the model and await its answer (Claude), or that
    /// reached it so far (Codex) (fac-389).
    pub staged: Vec<String>,
    /// The delivered messages the model read before its reply (fac-389).
    pub taken: Vec<String>,
    /// Delivered messages' first positions among the turn's own steps; only taken ones are published (fac-406).
    pub read_at: std::collections::BTreeMap<String, usize>,
    /// Codex's latest agent message, its reply once its turn completed (app-server).
    pub last_text: Option<String>,
    /// The model answered the turn's prompt: an output of its own on the main thread (fac-620).
    pub answered: bool,
    /// The session's context was compacted during the turn (fac-620).
    pub compacted: bool,
}

/// One step, in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub words: String,
    /// The text and thinking its thread gave since its step before (fac-96).
    pub details: Vec<factory_types::seen::Detail>,
    /// The subagent's tool call it ran under; none for the turn's own.
    pub thread: Option<String>,
    /// When it started and ended, as read (fac-206).
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextReport {
    pub used: u64,
    pub window: Option<u64>,
    pub model: Option<String>,
}

impl StreamState {
    /// A stream of a seat working in `folder`, with the agent home `home`.
    pub fn for_folder(folder: &str, home: &str) -> Self {
        Self { folder: folder.to_owned(), home: home.to_owned(), ..Self::default() }
    }
}

/// Whether a job's stream is Codex's app-server's: a live Codex turn's (fac-389).
pub fn app_server(spec: &factory_types::job::JobSpec) -> bool {
    matches!(spec.live, Some(factory_types::job::Live::Codex { .. }))
}

/// Reads one complete line of a turn's stream, in the format its command speaks.
pub fn read_line(cli: factory_types::config::Cli, app_server: bool, st: &mut StreamState, line: &str) {
    match cli {
        factory_types::config::Cli::Claude => claude::read_line(st, line),
        factory_types::config::Cli::Codex if app_server => appserver::read_line(st, line),
        factory_types::config::Cli::Codex => codex::read_line(st, line),
        factory_types::config::Cli::Agy => agy::read_line(st, line),
    }
}

/// Dollars as millionths of a dollar, as the state keeps a cost (1.4.8, Claude 7).
pub fn micros(usd: f64) -> u64 {
    // why: a cost is never negative, and far below u64's range in millionths.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let m = (usd.max(0.0) * 1_000_000.0).round() as u64;
    m
}

/// The context window the stream showed, with its model: the latest call's model, else the
/// one model the result named (Claude 9).
pub fn window_of(st: &StreamState) -> Option<(String, u64)> {
    let model = st.context.as_ref().and_then(|c| c.model.clone());
    match model {
        Some(m) => st.windows.get(&m).map(|w| (m, *w)),
        None => st.windows.iter().next().map(|(m, w)| (m.clone(), *w)),
    }
}
