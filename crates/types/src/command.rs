//! The commands of 1.13, as a type. `catalog::CATALOG` declares the same commands with their
//! fields and deciding areas; a command it lacks fails parsing.

use serde::{Deserialize, Serialize};

use crate::card::Mode;
use crate::reply::Role;
use crate::{AgentKey, CardId};

/// How a `say` reaches an agent running a turn (fac-78).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    /// Read at its next turn, as every message is.
    Queued,
    /// Its running turn is cut, and its next one starts with the message.
    Now,
}

/// A deliverable or a task picked by its number, from 1, or by its name or text (fac-255).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Pick {
    N(u64),
    Name(String),
}

impl From<u64> for Pick {
    fn from(n: u64) -> Self {
        Self::N(n)
    }
}

impl Pick {
    /// The number it stands for: a number, or a name of digits alone.
    pub fn number(&self) -> Option<u64> {
        match self {
            Self::N(n) => Some(*n),
            Self::Name(s) => s.trim().parse().ok(),
        }
    }
}

/// A string or an array of strings. Plans also accept lines or an encoded array in a string (fac-417).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextList {
    Text(String),
    Items(Vec<String>),
}

/// One command, as sent over a socket: `{"<name>": {fields…}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "watch")]
    Watch {
        places: Vec<String>,
        /// Each place's view with only its latest `tail` messages (fac-42).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail: Option<u64>,
        /// The whole place view, including turn traces and the clone's work; without it, the
        /// conversation alone (fac-280).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<bool>,
    },
    /// The sending orchestrator's project's stream (fac-119, fac-244): what reaches the operator there, from
    /// where its last hook stopped.
    #[serde(rename = "hook")]
    Hook {},
    /// The sending orchestrator's next batch of wake entries (fac-663).
    #[serde(rename = "wakes")]
    Wakes {},
    #[serde(rename = "history")]
    History {
        place: crate::PlaceId,
        before: u64,
        count: u64,
        /// With it, the page also carries the turns between its messages (fac-280).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turns: Option<bool>,
    },
    /// The posted card-room messages and card records matching any of the words (fac-371).
    #[serde(rename = "history.search")]
    HistorySearch { words: String },
    #[serde(rename = "history.journal")]
    HistoryJournal {
        card: CardId,
        count: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kinds: Option<Vec<String>>,
    },
    #[serde(rename = "status")]
    Status {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        jobs: Option<bool>,
        /// With `jobs`, the settled jobs whose folders are still kept too, not the live ones alone (fac-280).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        all: Option<bool>,
        /// With `jobs`, only the jobs of this card (fac-280).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        card: Option<String>,
    },
    #[serde(rename = "usage")]
    Usage {},
    #[serde(rename = "config")]
    Config {},
    #[serde(rename = "config.change")]
    ConfigChange { change: crate::config::view::ConfigChange },
    #[serde(rename = "quality")]
    Quality {},
    #[serde(rename = "routing")]
    Routing {},
    #[serde(rename = "claude.account")]
    ClaudeAccount { name: String },
    #[serde(rename = "projects")]
    Projects {},
    #[serde(rename = "lookback")]
    Lookback {
        project: crate::ProjectId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        read: Option<String>,
    },
    #[serde(rename = "build")]
    Build { commit: String },
    #[serde(rename = "pin")]
    Pin { commit: String },
    #[serde(rename = "reload")]
    Reload {
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    #[serde(rename = "say")]
    Say {
        place: crate::PlaceId,
        #[serde(skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        images: Option<Vec<String>>,
        /// The message's kind (fac-18): an agent's must carry one, the operator's needs none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<crate::reply::Intent>,
        /// `now` cuts the recipient's running turn and continues its session with the message
        /// (fac-78); none, queued.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delivery: Option<Delivery>,
        /// The option of the addressed seat's open ask this answer chooses, by its number from 1 (fac-696).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        choice: Option<u32>,
    },
    #[serde(rename = "message.send-now")]
    MessageSendNow { place: crate::PlaceId, seq: u64 },
    #[serde(rename = "dm")]
    Dm { card: CardId, role: Role },
    /// A foreman's child, given its first task and chosen model and effort.
    #[serde(rename = "spawn")]
    Spawn {
        card: CardId,
        role: Role,
        task: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cli: Option<String>,
        /// What a reviewer reviews, in its foreman's words (fac-559): required of a reviewer, refused of others.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        topic: Option<String>,
    },
    #[serde(rename = "ack")]
    Ack { ask: crate::AskId },
    #[serde(rename = "start")]
    Start {
        card: CardId,
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<Mode>,
        /// Started in visual mode (fac-258): the operator's or an orchestrator's, on a full card.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        visual: Option<bool>,
        /// The run's foreman's model, as `pick` names one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// The run's foreman's effort, as `pick` names one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delegated: Option<bool>,
        #[serde(rename = "self-sustained", default, skip_serializing_if = "Option::is_none")]
        self_sustained: Option<bool>,
        /// A full run with no planner, its foreman holding the specification (fac-616): the operator's or an orchestrator's.
        #[serde(rename = "no-planner", default, skip_serializing_if = "Option::is_none")]
        no_planner: Option<bool>,
        /// The cards the run carries with it (fac-563), sharing its room, clone, branch and merge.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        carries: Vec<CardId>,
    },
    #[serde(rename = "approve")]
    Approve { card: CardId },
    /// The operator reopens a visual card's approved mock, at any stage until its final landing starts (fac-591).
    #[serde(rename = "mock")]
    Mock { card: CardId },
    /// The operator or the card's own foreman sends an approved card back to brief in the same run (fac-638).
    #[serde(rename = "brief")]
    Brief {
        card: CardId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        words: Option<String>,
    },
    #[serde(rename = "merge")]
    Merge { card: CardId },
    #[serde(rename = "stop")]
    Stop {
        #[serde(skip_serializing_if = "Option::is_none")]
        card: Option<CardId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent: Option<AgentKey>,
    },
    #[serde(rename = "resume")]
    Resume {
        #[serde(skip_serializing_if = "Option::is_none")]
        card: Option<CardId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent: Option<AgentKey>,
    },
    #[serde(rename = "fresh")]
    Fresh { agent: String },
    /// A session's own model and effort (fac-96), from its next turn.
    #[serde(rename = "pick")]
    Pick {
        agent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        /// The model's CLI, where seats run the same model text on both.
        #[serde(skip_serializing_if = "Option::is_none")]
        cli: Option<String>,
    },
    #[serde(rename = "close")]
    Close { card: CardId, why: String },
    #[serde(rename = "done")]
    Done { card: CardId },
    /// The operator's (fac-222): turns a full card's preview worker on, starting or restarting it, or
    /// `stop` turns it off; `start` and `restart` as well (fac-196).
    #[serde(rename = "preview")]
    Preview {
        card: CardId,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<String>,
    },
    /// The operator's (fac-222): `preview`'s restart, and the card's foreman asked for the changed page's link and a brief reminder of the change.
    #[serde(rename = "demo")]
    Demo { card: CardId },
    #[serde(rename = "prs")]
    Prs { project: crate::ProjectId },
    #[serde(rename = "review")]
    Review { project: crate::ProjectId, pr: u64 },
    #[serde(rename = "review.merge")]
    ReviewMerge { project: crate::ProjectId, pr: u64 },
    #[serde(rename = "review.branch")]
    ReviewBranch { project: crate::ProjectId, branch: String },
    #[serde(rename = "post")]
    Post { card: CardId },
    /// `declined`: the operator's decision on the card rules the finding out, said on its PR thread (fac-349).
    #[serde(rename = "resolve")]
    Resolve {
        card: CardId,
        n: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        declined: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dropped: Option<String>,
    },
    #[serde(rename = "answer")]
    Answer { card: CardId, n: u64, text: String },
    /// A new round of our card's PR on its current head, its last round failed (fac-349).
    #[serde(rename = "rereview")]
    Rereview { card: CardId },
    /// Triages a failed review round's findings: every finding chosen fix, decline or foreman (fac-680).
    #[serde(rename = "triage")]
    Triage {
        card: CardId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fix: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        decline: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        foreman: Vec<String>,
    },
    #[serde(rename = "deliverables")]
    Deliverables { card: CardId, names: TextList },
    #[serde(rename = "plan")]
    Plan { card: CardId, deliverable: Pick, tasks: TextList },
    #[serde(rename = "plan.task")]
    PlanTask { card: CardId, deliverable: Pick, task: Pick },
    #[serde(rename = "card.show")]
    CardShow { card: CardId },
    /// A review card's findings, named by it or by the card it reviews.
    #[serde(rename = "findings")]
    Findings { card: CardId },
    #[serde(rename = "card.create")]
    CardCreate {
        project: crate::ProjectId,
        title: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<crate::CardId>,
    },
    #[serde(rename = "card.change")]
    CardChange {
        card: CardId,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        decision: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        withdraw: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        priority: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        labels: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        acceptance: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    #[serde(rename = "card.close")]
    CardClose {
        card: CardId,
        #[serde(rename = "as")]
        close_as: String,
        note: String,
    },
    #[serde(rename = "actions")]
    Actions {},
    #[serde(rename = "action.retry")]
    ActionRetry { action: crate::ActionId },
    #[serde(rename = "action.drop")]
    ActionDrop { action: crate::ActionId },
    #[serde(rename = "action.done")]
    ActionDone { action: crate::ActionId, note: String },
    #[serde(rename = "test.lease")]
    TestLease { kind: String, command: Vec<String> },
    #[serde(rename = "test.resume")]
    TestResume { lease: crate::LeaseId },
    #[serde(rename = "test.done")]
    TestDone { lease: crate::LeaseId, exit: u64 },
    #[serde(rename = "door")]
    Door { target: String, op: String, params: serde_json::Value },
    #[serde(rename = "door.once")]
    DoorOnce { ask: crate::AskId },
    #[serde(rename = "door.no")]
    DoorNo { ask: crate::AskId },
    #[serde(rename = "door.help")]
    DoorHelp {
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    /// The operator gives an open ask to its project's orchestrator (fac-119, fac-244): a line on its stream,
    /// the ask marked as with it.
    #[serde(rename = "give")]
    Give { ask: crate::AskId },
    /// The operator asks the ask's orchestrator for advice in its own chat, leaving the ask his (fac-239).
    #[serde(rename = "opinion")]
    Opinion { ask: crate::AskId },
    /// The operator sets a card's delegation level, or ends it with `off` (fac-474).
    #[serde(rename = "delegate")]
    Delegate {
        card: crate::CardId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        off: Option<bool>,
        #[serde(rename = "self-sustained", default, skip_serializing_if = "Option::is_none")]
        self_sustained: Option<bool>,
    },
    /// The operator turns a live full card's visual mode on, or with `off` off (fac-258).
    #[serde(rename = "visual")]
    Visual {
        card: crate::CardId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        off: Option<bool>,
    },
    /// The operator turns a project's auto on, or with `off` ends it (fac-381): cards that start in the project
    /// from then on start with their own `delegate` toggle on.
    #[serde(rename = "auto")]
    Auto {
        project: crate::ProjectId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        off: Option<bool>,
    },
    /// A memory file's text, a seat's read counted (fac-159, fac-598).
    #[serde(rename = "memory.read")]
    MemoryRead { memory: String, name: String },
    /// The memory files matching any of the words, no read counted (fac-293).
    #[serde(rename = "memory.search")]
    MemorySearch { words: String },
    /// Creates or rewrites a memory file.
    #[serde(rename = "memory.write")]
    MemoryWrite {
        memory: String,
        name: String,
        description: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new: Option<bool>,
    },
    #[serde(rename = "memory.delete")]
    MemoryDelete { memory: String, name: String },
    /// The memories' counts and files, or one memory's.
    #[serde(rename = "memory.stats")]
    MemoryStats {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        memory: Option<String>,
    },
    /// Files a note to the project's orchestrator (fac-287), with source details for a smell or a proposal.
    #[serde(rename = "notes.add")]
    NotesAdd {
        kind: crate::note::NewNote,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<TextList>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<crate::ProjectId>,
    },
    #[serde(rename = "notes")]
    Notes { project: crate::ProjectId },
    #[serde(rename = "notes.delete")]
    NotesDelete { project: crate::ProjectId, ids: Vec<String> },
    /// Sends one project's part of a section to review (fac-230, fac-249). `target` is a configured
    /// project, or `general` for the Memory section's shared memory, which goes to Mork's.
    #[serde(rename = "orchestrator.review")]
    OrchestratorReview { section: crate::orchestrator::ReviewSection, target: String },
    #[serde(rename = "debug.sleep")]
    DebugSleep { ms: u64 },
}

impl Command {
    /// The command's name, as the catalog has it.
    pub fn name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_object().and_then(|o| o.keys().next().cloned()))
            .unwrap_or_default()
    }

    /// Parses a command, strictly: the catalog's check first, so every refusal names its
    /// field, then the type.
    pub fn parse(v: &serde_json::Value, faults: bool) -> Result<Self, crate::catalog::Invalid> {
        crate::catalog::validate(v, faults)?;
        serde_json::from_value(v.clone())
            .map_err(|e| crate::catalog::Invalid { field: "cmd".into(), message: e.to_string() })
    }

    /// The command's declaration in the catalog: its fields, the area that decides it, whether it is a read.
    pub fn spec(&self) -> Option<&'static crate::catalog::CommandSpec> {
        crate::catalog::find(&self.name(), true)
    }

    /// Whether the command acts outward, so a reload under way refuses it (1.8.6.2).
    pub fn acts_outward(&self) -> bool {
        !self.spec().is_some_and(|s| s.read)
    }
}
