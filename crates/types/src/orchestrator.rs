//! The orchestrators' streams (fac-119, fac-244): what reaches Andrei, as lines each project's
//! orchestrator's hook reads, kept until written to it.

use serde::{Deserialize, Serialize};

use crate::ask::AskKind;
use crate::ids::{AskId, CardId, PlaceId};

/// One line of a project's stream, numbered in the order it happened, across every project's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorEvent {
    pub seq: u64,
    pub time: i64,
    /// Whose stream it is on: its card's project, or with none the factory's own (fac-244).
    pub project: crate::ProjectId,
    #[serde(flatten)]
    pub what: OrchestratorWhat,
}

/// What happened: a message to Andrei or the orchestrator, an ask opened, closed or given to it, or a
/// section Andrei sent it to review or an ask he wants advice on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum OrchestratorWhat {
    Message {
        place: PlaceId,
        card: Option<CardId>,
        from: String,
        text: String,
        /// A seat's or the engine's request to an orchestrator (fac-474): wakes it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        request: bool,
    },
    AskOpened(OrchestratorAsk),
    AskClosed(OrchestratorAsk),
    /// Andrei gave the orchestrator this ask (fac-119's button).
    Given(OrchestratorAsk),
    /// Andrei wants advice in the orchestrator's own chat; the ask stays his (fac-239).
    Opinion {
        ask: OrchestratorAsk,
        text: String,
    },
    /// Andrei sent one project's part of the Memory or Notes tab to review (fac-230, fac-249's
    /// buttons). A line kept from before them has no target: the whole section.
    Review {
        section: ReviewSection,
        /// The project, or `general`, whose part it is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        text: String,
    },
}

/// A section of main's tabs Andrei sends to review (fac-230).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewSection {
    /// The Memory tab's memories.
    Memories,
    /// The Notes tab: a project's notes. A line kept from before it says `surveys` or `smells`.
    #[serde(alias = "surveys", alias = "smells")]
    Notes,
}

/// An ask as the stream shows it: where, whose, what kind, its words and its buttons' answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorAsk {
    pub ask: AskId,
    pub place: PlaceId,
    pub card: Option<CardId>,
    pub from: Option<String>,
    pub kind: AskKind,
    pub text: String,
    /// Its buttons: each one's word, and the command it sends.
    pub answers: Vec<crate::view::Offer>,
}

/// A project's auto switch (fac-381): while on, each card that starts in the project starts delegated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auto {
    pub project: crate::ProjectId,
    pub on: bool,
}
