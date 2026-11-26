//! `decide(&State, Event, ctx) -> Decision` (1.12.1): what happens next, as changes to store in one transaction,
//! intents for the engine to carry out after it commits, and the answer to a command.

use mork_types::Command;
use mork_types::config::Config;
use mork_types::protocol::{ErrorBody, ErrorCode};
use mork_types::state::{Job, Sender};
use mork_types::{AgentKey, CardId, JobId};
use serde_json::Value;

use crate::agents::{turns, usage};
use crate::maintenance::updates;
use crate::state::{Change, State};

pub(crate) mod actions;
pub mod buzz;
mod commands;
mod events;
pub mod jobs;
pub(crate) mod token;

pub use crate::agents::turns::SessionSeen;

#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    pub now: i64,
    pub config: &'a Config,
    /// A build with `faults`: the echo role and `debug.sleep` exist (1.10.4).
    pub faults: bool,
}

/// How a job ended, as the engine read it from its folder (1.4.6, 1.4.7).
#[derive(Debug, Clone, PartialEq)]
pub enum JobEnd {
    /// A complete result: a reply, or the CLI's error.
    Finished { reply: Result<Value, String>, session: SessionSeen },
    /// It ended with no complete result (no `result` event), not stopped: a failed turn.
    Failed { words: String },
    /// Its supervisor died with no end, and no complete result: cut (1.4.6).
    Cut { session: SessionSeen },
    /// A recorded stop (1.4.5).
    Stopped,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Command {
        from: Sender,
        cmd: Command,
    },
    /// The engine made the job of an owed turn; record its intent (1.4.3.1).
    TurnPrepared {
        job: Box<Job>,
        session: Option<String>,
    },
    /// The supervisor claimed it (1.4.3.7).
    JobRunning {
        id: JobId,
    },
    /// What a turn's stream showed of its session, for an end that carries none (a stop, a
    /// failure): a resumed turn then resumes that session (1.4.8).
    SessionSeen {
        id: JobId,
        session: SessionSeen,
    },
    /// The job ended (or was found ended at recovery).
    JobEnded {
        id: JobId,
        end: JobEnd,
    },
    /// No wall can start: the owed turn doesn't launch, and the agent shows why (1.4.11.6).
    WallMissing {
        agent: AgentKey,
        reason: String,
    },
    /// An owed turn couldn't be made (no clone, no allowed model, a folder not made): the agent shows
    /// why and waits, its turn owed, for a message or its `resume` (fac-146).
    TurnUnmade {
        agent: AgentKey,
        reason: String,
    },
    /// A build job ended: its build is in place, or it failed and left none (1.8.3).
    BuildEnded {
        id: JobId,
        commit: String,
        result: Result<(), String>,
        /// A build of a master commit past the pin, pinned or refused by the pin's checks (fac-19).
        pinned: Option<Result<(), String>>,
    },
    /// The engine says something in a place (`Reloaded to …`, 1.8.6.6).
    EngineSays {
        place: mork_types::PlaceId,
        text: String,
        /// What makes it once only (`reload:<id>`), journaled with it so a repeat is seen.
        key: Option<String>,
    },
    /// A job that belongs to no agent's turn (a build) is recorded, `launching` (1.4.3.1).
    JobRecorded {
        job: Box<Job>,
    },
    /// A CLI reported usage-limit windows: from a turn's stream, or Codex's limits read.
    LimitsSeen {
        limits: Vec<mork_types::usage::Limit>,
        /// From a turn's stream: the job, and how far into its `out.jsonl` these limits were
        /// read (1.4.7.2), stored with them.
        at: Option<(JobId, u64)>,
        /// A whole read of this CLI's limits (Codex's): a window it doesn't list is no longer
        /// reached (1.4.8, Codex 7).
        whole: Option<mork_types::config::Cli>,
    },
    /// The boards' cards, read again (an index, never stored).
    BoardsRead {
        cards: Vec<mork_types::card::BoardCard>,
        /// Each board's id prefix, from its `config.yml` (M5.27).
        prefixes: std::collections::BTreeMap<mork_types::ProjectId, String>,
    },
    /// The engine is about to carry out a pending action: `started` commits first (2.5.1).
    ActionStarted {
        id: mork_types::ActionId,
        /// What the engine read before the call, to check after a crash (a board file's
        /// content before and after, 2.5.2); kept from the first start.
        prepared: Option<Value>,
    },
    /// An outward action returned (2.5.1).
    ActionReturned {
        id: mork_types::ActionId,
        result: Result<Value, mork_types::work::ActionFailure>,
    },
    /// What a `started` action did, as found after a crash (2.5.2).
    ActionFound {
        id: mork_types::ActionId,
        found: mork_types::work::Found,
    },
    /// The poller, or the re-read before a merge, read a card's PR (4.11).
    PrRead {
        card: CardId,
        seen: mork_types::work::PrSeen,
        /// Read right before the merge call (4.11.2.4), not by the poller.
        before_merge: bool,
    },
    /// The poller read a project's open PRs (4.16.1).
    OpenPrsRead {
        project: mork_types::ProjectId,
        prs: Vec<mork_types::work::OpenPr>,
    },
    /// Who the Bitbucket token is (3.7.2): `None` when it may act, else why not.
    TokenRead {
        refused: Option<String>,
    },
    /// Bitbucket accepted the token without saying who it is (`GET /user` answered, not 401): a
    /// refusal for a 401 is over (3.7.2).
    TokenAccepted,
    /// Master's tip, read in the operator's checkout, isn't `current`: the pin, else the build running (fac-19).
    MasterSeen {
        tip: mork_types::Sha,
        current: String,
        /// Its build, already in place, tried for the pin: pinned, or refused by the pin's checks.
        pinned: Option<Result<(), String>>,
    },
    /// A project's checks folder was read at this commit (4.7.7).
    ChecksFolderSeen {
        project: mork_types::ProjectId,
        commit: String,
    },
    /// Time passed: leases to free, queues to start, fetches and polls due.
    Tick,
    /// A test-slot client's connection ended (4.9.3).
    LeaseClosed {
        lease: mork_types::LeaseId,
    },
    /// The engine started, or a reload took over: leases wait for their holders (4.9.4).
    Started,
    /// The operating material the build before this one gave, as a start journaled it, or `None`
    /// when unknown; `build` is this one's: what changed is announced (fac-14).
    MaterialRead {
        previous: Option<crate::maintenance::material::Recorded>,
        build: String,
    },
    /// A door request, resolved by the engine against its targets (fac-1): its call, and the allowlist's verdict.
    DoorAsked {
        call: Box<mork_types::door_call::DoorCall>,
        validator: mork_types::door_call::Verdict,
        /// The calls under way that are being stopped, their caller gone: none is matched.
        stopped: Vec<mork_types::ActionId>,
    },
    /// A door request no adapter may run, or no operation: journaled and answered.
    DoorRefused {
        from: Sender,
        target: String,
        op: String,
        status: String,
        why: String,
    },
    /// A door call's requester was told how it stands: by its answer, or (`answered` false) now by news.
    DoorDelivered {
        id: mork_types::ActionId,
        answered: bool,
    },
    /// A pending door call lost its last waiter: busy, or an unstarted read whose caller went.
    DoorUnstarted {
        id: mork_types::ActionId,
        busy: Option<String>,
    },
    /// The kill of a read's query its server didn't take, tried again as the same query was asked
    /// again (fac-184): `ended` when killed, or none was left to kill.
    DoorKilled {
        id: mork_types::ActionId,
        ended: bool,
    },
    /// A failed round's proofs were copied into its fixer's clone: the files copied, or why not (fac-377).
    ProofsCopied {
        review: CardId,
        round: u32,
        result: Result<u32, String>,
    },
    /// A project's orchestrator's hook opened, or closed (fac-119, fac-244): main says so.
    Hooked {
        project: mork_types::ProjectId,
        hooked: bool,
    },
}

/// Work for the engine, after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// End a job (1.4.5).
    StopJob { id: JobId },
    /// Make and launch a job of no agent's turn (section 4); it is recorded through
    /// [`Event::JobRecorded`] with its purpose.
    StartJob { purpose: mork_types::work::Purpose },
    /// Carry out an action, recorded `pending` (2.5.1).
    RunAction { id: mork_types::ActionId },
    /// Send an ask's desktop notice, claimed in this transaction (2.6).
    Notify { ask: mork_types::AskId },
    /// Write a card's record again (2.9.1).
    WriteRecord { card: CardId },
    /// Read a card's PR again right before its merge call (4.11.2.4).
    ReadPr { card: CardId },
    /// A test slot freed: the engine asks again for the `test.lease` calls that wait (4.9.2).
    SlotFreed,
    /// Copy review card `review`'s proofs into `card`'s clone, for round `round`'s fix (fac-263).
    CopyProofs { review: CardId, card: CardId, round: u32 },
    /// Remove a handoff's copy, `dir` under `project`'s records, its note deleted (fac-287).
    RemoveHandoff { project: mork_types::ProjectId, dir: String },
    /// Remove a card's mock kept from an earlier clone, put into its new clone (fac-591).
    DropMock { card: CardId },
    /// Hand message `seq` to running turn `job`, its words and images, for its CLI to read while it works (fac-389):
    /// each image copied from its source to its name in the turn's `$MORK_IO`, as the job's `io_copies` record it.
    Deliver { job: JobId, seq: u64, text: String, images: Vec<(String, String)> },
}

/// A journal entry, appended in the same transaction (2.2.4).
/// See `docs/journal.md` for ordering, timestamps and history boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct Journal {
    pub kind: String,
    pub card: Option<String>,
    pub data: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decision {
    pub changes: Vec<Change>,
    pub intents: Vec<Intent>,
    pub journal: Vec<Journal>,
    pub answer: Option<Result<Value, ErrorBody>>,
}

impl Decision {
    pub fn refused(message: impl Into<String>) -> Self {
        Self { answer: Some(Err(ErrorBody::new(ErrorCode::Refused, message))), ..Self::default() }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self { answer: Some(Err(ErrorBody::new(ErrorCode::Invalid, message))), ..Self::default() }
    }
    pub fn ok(value: Value) -> Self {
        Self { answer: Some(Ok(value)), ..Self::default() }
    }
    pub fn journal(&mut self, kind: &str, card: Option<&str>, data: Value) {
        self.journal.push(Journal { kind: kind.to_owned(), card: card.map(str::to_owned), data });
    }
}

/// Records each project's stream, auto-gives delegated cards' seat and plan asks, and requests help for engine asks.
pub fn decide(state: &State, event: &Event, ctx: Ctx<'_>) -> Decision {
    let mut d = route(state, event, ctx);
    crate::messages::streams::record(state, &mut d, ctx.config.mork.project.as_ref(), ctx.now);
    crate::messages::delegate::give_opened(state, &mut d, ctx.now);
    crate::messages::delegate::request_opened(state, &mut d, ctx.now);
    d
}

fn route(state: &State, event: &Event, ctx: Ctx<'_>) -> Decision {
    match event {
        Event::Command { from, cmd } => commands::decide(state, from, cmd, ctx),
        Event::TurnPrepared { job, session } => turns::prepared(state, job, session.as_deref(), ctx),
        Event::JobRunning { id } => turns::running(state, id),
        Event::SessionSeen { id, session } => turns::session_seen(state, id, session, ctx.now),
        Event::JobEnded { id, end } => crate::agents::ends::ended(state, id, end, ctx),
        Event::WallMissing { agent, reason } => crate::agents::seats::held(state, agent, reason, "wall.missing", ctx),
        Event::TurnUnmade { agent, reason } => crate::agents::seats::held(state, agent, reason, "turn.unmade", ctx),
        Event::BuildEnded { id, commit, result, pinned } => {
            updates::build_ended(state, id, commit, result, pinned, ctx)
        }
        Event::MasterSeen { tip, current, pinned } => updates::seen(state, tip, current, pinned.as_ref(), ctx),
        Event::EngineSays { place, text, key } => crate::messages::engine_says(state, place, text, key.as_deref(), ctx),
        Event::JobRecorded { job } => turns::recorded(state, job),
        Event::LimitsSeen { limits, at, whole } => usage::seen(state, limits, at.as_ref(), *whole, ctx),
        Event::DoorAsked { call, validator, stopped } => {
            crate::door::asking::asked(state, call, validator, stopped, ctx)
        }
        Event::DoorRefused { from, target, op, status, why } => {
            crate::door::asking::refused(from, target, op, status, why)
        }
        Event::DoorDelivered { id, answered } => crate::door::delivered(state, id, *answered, ctx),
        Event::DoorUnstarted { id, busy } => crate::door::queue::dropped(state, id, busy.as_deref(), ctx),
        Event::DoorKilled { id, ended } => crate::door::reads::killed(state, id, *ended, ctx),
        Event::MaterialRead { previous, build } => {
            crate::maintenance::material::material(state, previous.as_ref(), build, ctx)
        }
        Event::ProofsCopied { review, round, result } => {
            let mut p = crate::post::Poster::for_ctx(state, ctx);
            crate::reviews::verdict::copied(&mut p, review, *round, result, ctx);
            let mut d = Decision::default();
            p.finish(&mut d);
            d
        }
        Event::Hooked { project, hooked } => crate::messages::streams::hooked(state, project, *hooked, ctx.now),
        Event::BoardsRead { cards, prefixes } => events::boards_read(state, cards, prefixes, ctx),
        Event::ActionStarted { id, prepared } => actions::started(state, id, prepared.as_ref(), ctx.now),
        Event::ActionReturned { id, result } => actions::returned(state, id, result, ctx),
        Event::ActionFound { id, found } => actions::found(state, id, found, ctx),
        Event::PrRead { card, seen, before_merge } => crate::queue::pr::pr_read(state, card, seen, *before_merge, ctx),
        Event::OpenPrsRead { project, prs } => crate::reviews::watch::open_prs(state, project, prs, ctx),
        Event::TokenRead { refused } => token::token(state, refused.as_deref(), ctx),
        Event::TokenAccepted => token::token(state, None, ctx),
        Event::ChecksFolderSeen { project, commit } => Decision {
            changes: vec![Change::SetChecksFolder { project: project.clone(), commit: commit.clone() }],
            ..Decision::default()
        },
        Event::Tick => events::tick(state, ctx),
        Event::LeaseClosed { lease } => crate::queue::leases::closed(state, lease, ctx),
        Event::Started => events::started(state, ctx),
    }
}
