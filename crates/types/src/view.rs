//! The views the engine publishes and every client draws (1.5.2, section 5): what is true, worked
//! out by `core`. Layout, folding, drafts, icons and animation are the window's.

use serde::{Deserialize, Serialize};

pub use crate::chat::{Author, HistoryPage, ImageView, Line, MessageView};
pub use crate::overview::{LiveRow, OverviewStage, OverviewStageAt, OverviewStageKind, OverviewView, SuiteProgress};
pub use crate::pipeline::{Fact, StageAt, StageKind, StageMark, Times};
pub use crate::rooms::{PlaceKind, RoomRow, Under};

use crate::ask::{AskKind, Color};
use crate::card::{Mode, Stage};
use crate::config::Effort;
use crate::reply::Role;
use crate::{AgentKey, CardId, Command, PlaceId};

/// The whole screen but the place shown: the top line, the rooms, the hands, the boards.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screen {
    pub revision: u64,
    /// The engine's time when it was made, and the local time's offset from UTC (5.12).
    pub now_ms: i64,
    pub tz_offset_min: i32,
    pub top: TopLine,
    /// `main`, then each live card in the order it started, each followed by its DMs (5.6.1).
    pub rooms: Vec<RoomRow>,
    /// Every open ask: red first, then oldest first (5.9.1).
    pub hands: Vec<HandView>,
    /// Main's Board tab: every configured board, in the config's order (5.8.1, fac-37).
    pub boards: Vec<BoardView>,
    /// Memory summaries in order: `general`, then each project's (fac-159).
    #[serde(default)]
    pub memories: Vec<crate::memory::MemoryView>,
    /// Project notes: each project with notes, by name (fac-287).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<crate::note::ProjectNotes>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<crate::lookback::ProjectView>,
}

/// The top line (5.5).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopLine {
    pub pin: PinLine,
    pub usage: Vec<Share>,
    pub phase: Phase,
    /// The machine's load, once the engine has sampled it; transient, never stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<Resources>,
}

/// The whole machine's CPU and memory now, each `None` when it couldn't be read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resources {
    /// Busy share of every core since the last sample, 0 to 100.
    pub cpu_percent: Option<u8>,
    pub ram: Option<Ram>,
}

/// Memory in MiB: what is in use, reclaimable cache not counted, of the total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ram {
    pub used_mib: u64,
    pub total_mib: u64,
}

/// The pin against Mork repo's master (1.8.9), and the update (fac-19).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinLine {
    /// The pinned build's short commit, if one is pinned.
    pub pin: Option<String>,
    pub master: Master,
    /// The pin isn't the build running: a reload, or a restart, runs it. Its button sends
    /// `reload` with no target.
    #[serde(default)]
    pub ready: bool,
    /// Master's tip, when it isn't the pin: its build under way, or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<Update>,
}

/// Master's tip on its way to the pin (fac-19), by its short commit; a failed one with the
/// command its `retry` button sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum Update {
    /// Its build is owed or under way.
    Preparing { short: String },
    /// Its build failed, or couldn't start: `retry` builds it again.
    BuildFailed { short: String, retry: Command },
    /// Its build is in place, and the pin's checks refused it: `retry` pins it.
    NotPinned { short: String, retry: Command },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum Master {
    /// Git can't say: never shown as equal.
    #[default]
    Unknown,
    Equal,
    Ahead {
        commits: u32,
    },
}

/// One account pool's usage, pace and reset countdown, once it has a share or is reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    pub cli: String,
    /// `5h`, `week` or `fable`.
    pub window: String,
    /// Its share, when known.
    pub percent: Option<u32>,
    /// The CLI said the limit is reached.
    pub reached: bool,
    /// Its reset time passed with nothing newer read.
    pub stale: bool,
    /// When it resets, in epoch ms, when known.
    #[serde(default)]
    pub reset_ms: Option<i64>,
    #[serde(default)]
    pub ends_percent: Option<u64>,
    #[serde(default)]
    pub signed_out: bool,
    /// The saved Claude account this signed-out pool's header switches to, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub switch_account: Option<String>,
    /// The age of the signed-out account's latest reading, for example `2d`.
    #[serde(default)]
    pub read_ago: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum Phase {
    #[default]
    Ready,
    /// Recovery under way (2.7), and what it is doing.
    Recovering { doing: String },
    /// A reload under way (1.8), to this short commit.
    Reloading { short: String },
}

/// An agent's state, as its square and icon draw it (5.6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentLight {
    Working,
    /// Its own ask is open.
    Asking,
    Error,
    /// Waiting for a turn or a test slot.
    Waiting,
    Idle,
}

/// One open ask (5.9): the window marks its place's row and lists it where it was raised (fac-96).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandView {
    pub seq: u64,
    pub message_place: PlaceId,
    pub headline: String,
    pub ask: crate::AskId,
    pub place: PlaceId,
    pub card: Option<CardId>,
    /// The role that raised it, when an agent did.
    pub role: Option<Role>,
    pub kind: AskKind,
    pub color: Color,
    pub words: String,
    pub raised_ms: i64,
    /// The commands the engine would take for it now (1.5.2).
    pub offers: Vec<Offer>,
    /// The operator gave it to an orchestrator (fac-119): it shows so in its orchestrator button's place.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub given: bool,
    /// A design question (fac-233): it shows a `design` tag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub design: bool,
    /// When the waiting seat's prompt cache expires, in epoch ms, while idle (fac-670).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_at_ms: Option<i64>,
    /// An `approve-brief` hand's standing decisions on its card, oldest first (fac-691).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,
    /// Its seat's options and pick, when its hand gave them (fac-696); `words` holds them composed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<crate::ask::Choices>,
    /// An `approve-brief` hand's typed sketch and a visual card's mock link (fac-696).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<BriefView>,
}

/// What an `approve-brief` hand shows beside its decisions (fac-696).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefView {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_shape: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mock: Option<String>,
}

/// A command the engine would accept now, and the word its button shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub label: String,
    pub command: Command,
}

/// One board in main's Board tab (5.8.1, fac-37).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardView {
    #[serde(default)]
    pub auto: bool,
    pub project: crate::ProjectId,
    /// Its live cards, in the order they started.
    pub live: BoardGroup,
    pub merge: MergeQueue,
    /// Its other cards not done, by area (one `area:<name>` label each), the areas by name and
    /// the cards with none last, under `Unsorted`.
    pub areas: Vec<BoardGroup>,
    /// Its done cards, under the board's done status.
    pub done: BoardGroup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardGroup {
    pub name: String,
    /// How many cards it has, whether or not their rows were all sent (a line's bound, M4.9).
    pub count: usize,
    /// Its cards, ids sorted as numbers.
    pub cards: Vec<BoardRow>,
}

/// A project's merge queue (fac-10): the card holding its slot, then those admitted after it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeQueue {
    /// The branch its cards land on.
    pub base: String,
    /// The card holding the slot, and where its landing stands, in words.
    pub slot: Option<(BoardRow, String)>,
    /// The cards waiting, first come first served, each with when `/merge` admitted it.
    pub waiting: Vec<(BoardRow, i64)>,
    /// How many cards wait, whether or not their rows were all sent (a line's bound, M4.9).
    pub in_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardRow {
    pub id: CardId,
    pub title: String,
    /// Mork ran it: live, done or closed (Enter opens its room).
    pub ran: bool,
    /// A live card's marks.
    pub live: Option<LiveMarks>,
    /// Its board status, when a row's place doesn't say it: a card not done that isn't in the
    /// Backlog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The room a click opens when it isn't the card's own: the run carrying it (fac-563).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<CardId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveMarks {
    pub light: Option<Color>,
    pub stage: String,
    /// Its agents at work.
    pub working: Vec<Role>,
    pub progress: u8,
}

/// One place, as the window shows it (5.7, 5.8.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceView {
    /// Unread message sequences and whether each can interrupt its recipient, including messages outside the tail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unread: Vec<(u64, bool)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<crate::about::StartDefaults>,
    pub place: PlaceId,
    pub kind: PlaceKind,
    pub revision: u64,
    /// A card's header, pipeline, times and facts.
    pub card: Option<CardHeader>,
    pub agents: Vec<AgentRow>,
    /// The latest messages, oldest first, within the line's bound; `earlier` more came before.
    pub messages: Vec<MessageView>,
    pub earlier: usize,
    /// Each retained turn in this conversation, between its messages (fac-138).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<crate::chat::ChatTurn>,
    /// The earlier messages whose ask is still open, oldest first: shown whole above `messages`
    /// until answered or replaced (fac-42).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<MessageView>,
    /// The commands the engine would take here now (1.5.2).
    pub offers: Vec<Offer>,
    /// Who the input's plain text goes to (3.4.5).
    pub input_to: String,
    /// The card's plan, in its room and its DMs (fac-36).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<crate::card::Plan>,
    /// The card's dated notes, newest first, in its room and its DMs: the operator's decisions among
    /// them, the card being its agents' memory (fac-94).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Every session of the card, in its room and its DMs (fac-105): its room's, its DMs', then
    /// its children's as spawned. Their steps and turns are `agents`' alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<AgentRow>,
    /// The files its agents' turns produced, in its room and its DMs, newest first (fac-105).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<crate::seen::OutputFile>,
    /// The step lists of its agents whose turn runs, shown above its input (fac-128): a card's
    /// room's agents' and its children's, a DM's own agent's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lists: Vec<crate::seen::AgentList>,
    /// A card's room's summaries (fac-594): its card's, then each card its run carries, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summaries: Vec<crate::about::Summary>,
    /// Folded internal review card's seats and current round for its parent's room (fac-680).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<crate::review::FoldedReview>,
    /// Open failed-round triage data for this card's room (fac-680).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage: Option<crate::review::TriageView>,
    /// Per-card Overview stage spine for the card's own room (fac-684).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<OverviewView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardHeader {
    pub id: CardId,
    pub title: String,
    pub mode: Mode,
    pub stage: Stage,
    pub paused: bool,
    /// The card's own stages, in order (5.7.2).
    pub pipeline: Vec<StageMark>,
    pub times: Times,
    pub facts: Vec<Fact>,
    /// The card's cost once it ended, in cents.
    pub cost_cents: Option<u64>,
    /// Its clone's folder, where a finding's `file:line` opens (5.7.5).
    pub clone: Option<String>,
    /// Where it works and what its run has cost so far (fac-105).
    #[serde(default)]
    pub about: crate::about::CardAbout,
    /// The run's spec room, if it has one, live or ended (fac-633).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<PlaceId>,
    /// Parent card id when this card is a folded internal review card (fac-680).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<CardId>,
}

/// One agent's row (5.7.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<crate::AgentKey>,
    pub agent: AgentKey,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Its seat's label and effort (`sol`, `xhigh`).
    pub label: String,
    pub effort: String,
    pub light: AgentLight,
    pub doing: Doing,
    /// Its context, used and window, when known.
    pub context: Option<(u64, u64)>,
    /// Its latest turn's steps (1.4.8), oldest first.
    pub steps: Vec<String>,
    /// Its session's model and effort, including its spawn selection or later pick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting: Option<Setting>,
    /// What its next turn launches with, where its session runs another: a pick, or its seat's
    /// own for a model `mork.toml` dropped. A running turn keeps what it launched with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Setting>,
    /// The models its session may pick, from the child list or the configured seats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<ModelChoice>,
    /// The efforts its session may pick; older views offer every effort.
    #[serde(default = "all_efforts")]
    pub efforts: Vec<Effort>,
    /// Its latest turn's steps with their text and thinking, of its own job only (fac-96).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<crate::seen::TurnView>,
    /// Its session's prompt-cache state; none before a first stamped turn (fac-670).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheState>,
}

/// A seat's prompt-cache state for Context (fac-670).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum CacheState {
    Running,
    Idle { cold_at_ms: i64 },
}

fn all_efforts() -> Vec<Effort> {
    Effort::OFFERED.to_vec()
}

/// A model offered to a session, with its CLI and configured label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    pub cli: crate::config::Cli,
    pub model: String,
    pub label: String,
}

/// A session's model and effort (fac-96).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setting {
    pub model: ModelChoice,
    pub effort: crate::config::Effort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum Doing {
    /// Working, with its current step's words.
    Working {
        step: String,
    },
    WaitingForSlot,
    WaitingOnYou,
    Error {
        words: String,
    },
    Idle,
}
