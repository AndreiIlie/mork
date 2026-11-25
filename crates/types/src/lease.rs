//! Test slots (4.9): leases, kept in the state across a reload or a restart.

use serde::{Deserialize, Serialize};

use crate::ids::{AgentKey, CardId};

/// What a lease runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseKind {
    Suite,
    Checks,
    Run,
    File,
    Walls,
}

/// A granted lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub id: crate::LeaseId,
    pub project: crate::ProjectId,
    pub card: Option<CardId>,
    /// The agent holding it, or `engine` for the engine's own runs.
    pub holder: AgentKey,
    pub kind: LeaseKind,
    /// After a restart or a reload: freed unless reclaimed by then (4.9.4).
    pub reclaim_by: Option<i64>,
}

/// A suite run remembered as passed (4.9.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pass {
    pub project: crate::ProjectId,
    pub tree: String,
    pub command: Vec<String>,
}
