//! The state's records (section 2): what the store keeps, and `core` decides on.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{AgentKey, JobId};
use crate::job::{JobKind, JobSpec};

/// Who wrote a message, or sent a command.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sender {
    /// The operator: the window, or `mork ctl` outside a wall (`mork.sock`).
    Operator,
    /// An agent, known by the wall its connection came from (`agent.sock`).
    Agent(AgentKey),
    Engine,
    /// A project's outside orchestrator: `mork ctl --as orchestrator:<project>` on `mork.sock`,
    /// never counted as the operator (fac-244).
    Orchestrator(crate::ProjectId),
    /// The operator's outside PA, which the orchestrators replaced (fac-244): only on what it sent before, as
    /// nothing says which of them it was; nobody sends as it now.
    Pa,
}

/// The prefix of an orchestrator's identity, before its project: `orchestrator:example.com` (fac-244).
pub const ORCHESTRATOR: &str = "orchestrator:";

impl Sender {
    pub fn label(&self) -> String {
        match self {
            Self::Operator => "operator".into(),
            Self::Agent(a) => a.as_str().to_owned(),
            Self::Engine => "engine".into(),
            Self::Orchestrator(p) => format!("{ORCHESTRATOR}{p}"),
            Self::Pa => "pa".into(),
        }
    }

    /// An orchestrator, or the PA they replaced: an outside session on the operator's socket, never him.
    pub fn is_orchestrator(&self) -> bool {
        matches!(self, Self::Orchestrator(_) | Self::Pa)
    }
}

/// A message's type (3.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    /// It woke the agent it reached.
    Request,
    /// It woke nobody.
    Update,
    /// An agent's hand, posted as a message (3.5.4).
    Hand,
    /// A line from the engine (a marker when it is to an agent, 5.7.5).
    Engine,
    /// A reply's `feedback` entry, which builds before schema 5 posted: stored ones still load,
    /// none is shown or posted anymore.
    Feedback,
}

/// One message in a place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub seq: u64,
    pub place: crate::PlaceId,
    pub time: i64,
    pub from: Sender,
    pub to: Option<String>,
    pub kind: MessageKind,
    pub headline: String,
    pub text: String,
    /// The operator's messages in main are numbered (3.3.5).
    pub number: Option<u64>,
    /// A hand, when the message is one (3.5.4): its text, color and ask.
    pub hand: Option<Box<crate::reply::Hand>>,
    /// The images it carries (`say`'s `images`), as paths outside any wall.
    pub images: Vec<String>,
    /// A reply's findings it carries (`report-ready`'s, a review's), shown under its line and
    /// kept in the record (T4.7, 5.7.5).
    pub findings: Vec<crate::reply::Finding>,
    /// Its author's role, when an agent posted it: kept for its colour once the agent is gone (M5.26), filled in by
    /// version 4 (M5.29). A role since removed (the assistant's, an advisor's; fac-119) reads as none.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::reply::known_role")]
    pub role: Option<crate::reply::Role>,
    /// What its agent said it is (fac-18), kept apart from `kind`, which says whether it woke
    /// anyone. None on what an agent's reply didn't send, and on messages from before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<crate::reply::Intent>,
    /// The ask the operator gave an orchestrator that this message of its closed: his delegation, so its
    /// answer carries his authority (fac-160). Only the engine sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated: Option<crate::AskId>,
    /// Where each entry of a card's new notes lies in `text`, in bytes, so a seat's news can leave out its own (fac-625).
    /// None on every other message, and on updates from before; boxed to keep the message small.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Box<[std::ops::Range<usize>]>>,
    /// The option of a seat's open ask this answer chose, by its number from 1 (fac-696).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<u32>,
}

/// What an agent is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AgentStatus {
    Idle,
    /// A turn of it is launching or running.
    Working {
        job: JobId,
    },
    /// Its last turn failed; a resume or a message retries it (1.4.10).
    Error {
        words: String,
    },
    /// Cut twice in a row: a `cut` ask, and nothing more runs (1.4.6.4).
    Cut,
    /// Its turn was stopped (4.13.7).
    Stopped,
    /// Its turn failed on a reached usage limit: it runs again after `until` (1.4.10.2).
    Waiting {
        until: i64,
    },
}

/// The turn an agent owes, durable until it launches (2.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "kebab-case")]
pub enum Owed {
    /// News woke it.
    Wake,
    /// A cut turn's one retry: the cut note, then the cut prompt (1.4.6.4).
    Retry {
        prompt: String,
        resume: bool,
        /// The job it retries corrects this one: still the same correction (1.4.9.3).
        corrects: Option<JobId>,
        /// The head the retried turn was asked about, kept: a retry never reviews a newer one (M4.2).
        #[serde(skip_serializing_if = "Option::is_none")]
        head: Option<crate::Sha>,
        /// The seq the retried turn's news reached, kept: the retry is given what it was (fac-51).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seen: Option<u64>,
        /// The job whose prompt carries the request it runs again (fac-626): a session holding that prompt word
        /// for word is asked to continue it instead.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request: Option<JobId>,
    },
    /// A correction of a refused reply (1.4.9).
    Correction { of: JobId, reason: String },
    /// The sampled survey's turn, before the closing turn replaced it (fac-287): asked as `Lesson` is.
    Survey,
    /// Its seat ends: its closing turn, in its own session first (fac-176, fac-287).
    Lesson,
}

/// A seat ending with its closing turn owed or under way (fac-287): it goes, or stays as its card's
/// seats do, once that turn settles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retiring {
    /// Removed once its closing turn settles; else kept, as a landed or done card's seats are.
    pub remove: bool,
    /// Its closing turn was launched: the turn that settles next is that one.
    pub asked: bool,
    /// Its card's next round began while its closing turn ran: that turn's reply is discarded, and a
    /// request the round owes follows it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resumed: bool,
    /// Runs while its card is still live, as the planner's approval closing turn does (fac-670).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub live: bool,
}

/// A foreman's held completion reports (fac-528): their one wake is a completion's, carrying a review round while
/// every one held was a reviewer's of it (fac-277).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldReports {
    /// The review round every held report was of; none for any other mix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    /// Their wake is owed, merged with any already owed: a verdict or `report-ready` dropping it drops them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub waking: bool,
}

/// An agent: a seat in a role at a place, with its own session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub key: AgentKey,
    pub seat: crate::SeatName,
    pub role: crate::reply::Role,
    pub place: crate::PlaceId,
    /// Claude's session id (made before the first turn), or Codex's thread id once read.
    pub session: Option<crate::SessionId>,
    /// Whether the session has started (Claude's `system/init`, Codex's `thread.started`).
    pub session_started: bool,
    /// Compaction requires the next prompt to restore the card specification (fac-737).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_specification: bool,
    pub status: AgentStatus,
    pub owed: Option<Owed>,
    /// The last message seq the agent has seen, for its news.
    pub seen: u64,
    /// `seen` before its latest turn: a failed turn gives its news back, so the next message
    /// retries it (1.4.10.1).
    pub seen_before: u64,
    /// A request reached it while a turn of it was owed for another reason (a correction, a
    /// retry): it is woken once that turn is done (3.4.1).
    pub wake_after: bool,
    /// The review round whose reviewers' reports alone woke it (fac-277): its wake, owed or after, is dropped
    /// once that round has its verdict. None once anything else woke it, or two rounds' reports did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_round: Option<u32>,
    /// Whether only its children's completion reports woke it (fac-314): a report card's accepted `report-ready`
    /// drops that wake. False once anything else woke it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub completion_wake: bool,
    /// Whether only the engine's "Build it" request woke it (fac-328): the card leaving build, or its accepted
    /// `built`, drops that wake. False once anything else woke it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub build_wake: bool,
    /// Its children's completion reports held while another of them was at work (fac-528), until a turn launched
    /// with its news reads them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_reports: Option<HeldReports>,
    /// Cuts in a row.
    pub cuts: u32,
    pub paused: bool,
    /// The last prompt it was given, for a retry.
    pub last_prompt: Option<String>,
    /// A turn owed after a reached usage limit waits until this time (1.4.10.2).
    pub not_before: Option<i64>,
    /// Each session's latest cost total, in millionths of a dollar (1.4.8, Claude 7): its
    /// sessions' sum is its cost.
    pub costs: BTreeMap<String, u64>,
    /// Each session or Codex thread's latest running token counts (fac-526).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub token_totals: BTreeMap<String, crate::cost::Tokens>,
    /// The job already charged, so session capture and settlement share one cost line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_job: Option<JobId>,
    /// The context window its CLI last reported, with that window's model (1.4.8, Claude 9).
    pub window: Option<(String, u64)>,
    /// The tokens its context held at its last turn's latest call, when its CLI reports them
    /// (fac-25); none for a session not yet reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// The session `fresh` replaced, said at the start of the one after it (fac-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fresh: Option<Fresh>,
    /// The foreman that spawned it (fac-94): its completion reports there, never to the card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<crate::AgentKey>,
    /// What a reviewer reviews (fac-559), shown beside its name; none for other seats and reviewers saved before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Its session's own model and effort (fac-96), over its seat's: none, its seat's role defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<Choice>,
    /// A pick its next turn takes (fac-96): a turn running finishes with what it launched with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Choice>,
    /// Its seat is ending with its closing turn owed or under way (fac-287): no longer its card's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retiring: Option<Retiring>,
    /// The turns its session ran, and whether one made a tool call (fac-176); none for a new session.
    #[serde(default, skip_serializing_if = "Ran::is_none")]
    pub ran: Ran,
    /// Its latest accepted reply's outcome (fac-379, fac-380): a card whose foreman last said `working`, its children
    /// `working`, finished or blocked, with nothing running or awaited, is stalled. None: not replied, or saved before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replied: Option<crate::reply::Outcome>,
    /// Messages a turn of its own read while it worked (fac-389): no later news repeats them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taken: Vec<u64>,
    /// Messages delivered into a turn that ended before reading them (fac-389): its next news says so.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missed: Vec<u64>,
    /// Each prompt section's text its current session was last sent, by the section's name (fac-620): a later
    /// turn sends a section again only when it changed. Empty for a new or compacted session.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub held: BTreeMap<String, String>,
    /// Its `$MORK_IO` folder's id under the home's `io/` (fac-654): its first turn's job id, kept for the seat's
    /// lifetime; none before its first turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io: Option<String>,
    /// When its latest turn that reached the model ended, for prompt-cache expiry (fac-670).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_ms: Option<i64>,
}

/// What a session did, so a short one ends without a lesson turn (fac-176).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ran {
    pub turns: u32,
    pub tools: bool,
    /// `turns` when its latest closing turn was answered (fac-670).
    #[serde(default)]
    pub closed: u32,
}

impl Ran {
    /// Nothing ran yet.
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }
}

/// A model and effort selected for one session, under its CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub cli: crate::config::Cli,
    pub model: String,
    pub effort: crate::config::Effort,
    /// `operator`, the picking agent's key, or `mork.toml` for the roster's.
    pub by: String,
    /// The roster phase it was made in (fac-399): it holds within that phase alone, until its seat leaves it; none is
    /// an older build's, or one its seat's phase change expired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<crate::config::SeatPhase>,
}

/// A `fresh` (fac-25): the session it replaces, who asked and when, and whether the old
/// session's running turn is still to end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fresh {
    pub replaces: Option<crate::SessionId>,
    /// `operator`, or the asking agent's key.
    pub by: String,
    pub at: i64,
    /// A turn of the old session runs: the session is replaced once it ends.
    pub pending: bool,
    /// The destination CLI when a selection switch replaced the old session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<crate::config::Cli>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobStatus {
    Launching,
    Running,
    Settled,
}

/// How a job settled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Settlement {
    Applied,
    Failed { words: String },
    Cut,
    Stopped,
}

/// A job's record in the state: its intent, everything its folder holds (1.4.2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub kind: JobKind,
    pub owner: AgentKey,
    pub spec: JobSpec,
    pub stdin: String,
    /// The current specification prefixed to this request. A retry gets its own current prefix.
    #[serde(default)]
    pub specification_bytes: usize,
    pub io_files: BTreeMap<String, String>,
    pub status: JobStatus,
    pub settled: Option<Settlement>,
    pub settled_ms: Option<i64>,
    pub stop_requested: bool,
    /// A retired brief turn may settle its job without touching the seat's session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
    /// Its stop was a `say --delivery now`'s (fac-78): its agent's session continues with the message.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interrupted: bool,
    /// The job this one corrects (1.4.9.5).
    pub corrects: Option<JobId>,
    /// A cut job's retry of it.
    pub retry_of: Option<JobId>,
    /// A retry of its session's opening request (fac-626): the job that first ran it, whose prompt this one sent
    /// again whole or asked its session to go on with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continues: Option<JobId>,
    /// It ran its session's opening request (fac-626): the turn that began that session, or a retry of it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub opening: bool,
    /// Files copied into `io/` when its folder is written: the path inside `io/`, from where.
    pub io_copies: BTreeMap<String, String>,
    /// Why a job of no agent's turn runs (section 4).
    pub purpose: Option<crate::work::Purpose>,
    /// How far into `out.jsonl` its usage limits were applied (1.4.7.2): a stream read again
    /// after a restart applies only what lies past it.
    pub limits_at: u64,
    /// The head a review turn was asked about (4.4.3): its pass counts for that head only.
    pub head: Option<crate::ids::Sha>,
    /// The seq its news reached: it was given the messages below it, a retry's or a correction's
    /// those of the turn it runs again (fac-51). None on a job an older build made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<u64>,
    /// A check run's checks folder, at the commit read when it was launched (4.7.7).
    pub checks_at: Option<String>,
    /// The messages delivered into it while it ran (fac-389), by seq.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delivered: Vec<u64>,
    /// Delivered messages its settled turn read; none on jobs made before fac-406.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<Vec<u64>>,
    /// The prompt sections this turn sent, by name (fac-620): its agent holds them once its model answered.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub staged: BTreeMap<String, String>,
}

impl Job {
    /// The request to replay, apart from any specification restored for this particular turn.
    pub fn request_text(&self) -> &str {
        self.stdin.get(self.specification_bytes..).unwrap_or(&self.stdin)
    }
}

/// A place: main, a card's room, a DM, or a test place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub id: crate::PlaceId,
    pub next_number: u64,
}
