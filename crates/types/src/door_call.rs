//! Durable door calls bind the request, target resolution, approval and requester notifications.
//! Legacy checking phases and model metadata remain readable across upgrades (fac-1, fac-124).

use serde::{Deserialize, Serialize};

use crate::ids::{AskId, CardId, JobId};
use crate::state::Sender;

/// What was asked, and what it resolved to: fixed once the call is made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorCall {
    /// Who asked: the socket's sender.
    pub from: Sender,
    /// Its place: the ask is raised there, and its news posted there.
    pub place: crate::PlaceId,
    pub card: Option<CardId>,
    /// The job whose wall sent it.
    pub job: Option<JobId>,
    /// Its logical turn: the first of the job's corrections and retries. The same request in it
    /// is the same call.
    pub turn: Option<JobId>,
    pub target: String,
    pub op: String,
    /// The typed parameters, as canonical JSON.
    pub params: String,
    /// The resolved envelope (`door_plan::Envelope`), as canonical JSON: what runs, where, as
    /// whom, within which bounds.
    pub plan: String,
    /// The engine's nonce: the dispatcher keeps its receipt under it and the action's id.
    pub nonce: String,
}

impl DoorCall {
    /// The request, exactly: compared whole, never by a hash.
    pub fn request(&self) -> String {
        format!("{}\n{}\n{}", self.target, self.op, self.params)
    }

    /// Whose scope its deduplication is: the sender and its logical turn.
    pub fn scope(&self) -> String {
        format!("{}:{}", self.from.label(), self.turn.as_ref().map_or("-", |t| t.as_str()))
    }

    /// The call's id for the dispatcher.
    pub fn call_id(&self, action: &str) -> String {
        format!("{}-{action}", self.nonce)
    }
}

/// An allowlist verdict or historical model verdict, with its reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    /// The allowlist's: `allowed` or `outside`. The model's: `safe`, `unsafe` or `unsure`.
    pub verdict: String,
    pub reason: String,
}

impl Verdict {
    pub fn new(verdict: &str, reason: impl Into<String>) -> Self {
        Self { verdict: verdict.to_owned(), reason: reason.into() }
    }
}

/// Where a call stands, besides its action's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// A legacy call interrupted during its removed model check.
    Checking,
    /// The allowlist permits unattended dispatch.
    Cleared,
    /// It waits for the operator.
    Asked,
    /// The operator said once.
    Approved,
    /// The operator said no.
    Refused,
}

/// The operator's `once`: bound to the plan it was given on, used by one dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub at_ms: i64,
    /// The envelope approved, whole.
    pub plan: String,
    /// When a dispatch took it, in the transaction that started it.
    pub consumed_ms: Option<i64>,
}

/// What changes as a call goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorState {
    pub phase: Phase,
    pub validator: Verdict,
    /// Historical model metadata, retained without affecting authorization.
    pub checker: Option<Verdict>,
    pub ask: Option<AskId>,
    pub approval: Option<Approval>,
    /// Historical model cost in millionths of a dollar; new calls record none.
    pub check_cost: Option<u64>,
    /// The outcome its requester was last told (`ask raised`, `done`, `held`…), by an answer
    /// or by news.
    pub told: Option<String>,
    /// A read cut whose server didn't take the kill of its query (fac-184): the session it may still
    /// run on, until its kill is taken or the session's limit has passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<crate::door_plan::Session>,
}
