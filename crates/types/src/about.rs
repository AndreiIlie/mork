//! What a card's header says of where it works and what its run has cost so far (fac-105).

use serde::{Deserialize, Serialize};

/// Its project, branch and base, its pull request, its current run's cost and delegation level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardAbout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    pub project: crate::ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub base: String,
    /// The base's commit it stands on, short: its clone's or latest join's, else its head's fork
    /// point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork: Option<String>,
    /// Its pull request, and its page when its project is on Bitbucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    /// Its current run's cost so far as its journal holds it, live or ended, in cents, when it
    /// was read; and whether a turn's cost in it is unknown (`+ ?`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_cost_cents: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub run_cost_unknown: bool,
    /// Current-run sums from the journal, including seats whose sessions have ended (fac-526).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seat_costs: Vec<SeatCost>,
    /// The foreman settles its specification and approves its plan (fac-474).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delegated: bool,
    /// The foreman also decides departures from the card and earlier decisions (fac-474).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub self_sustained: bool,
    /// The Card section's switches (fac-258), in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub switches: Vec<Switch>,
}

/// One seat's recorded dollars and disjoint tokens in the current card run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatCost {
    pub agent: crate::AgentKey,
    pub cost_cents: u64,
    pub unknown: bool,
    pub tokens: crate::cost::Tokens,
}

/// One card control, with its current position and each position's command (fac-474).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switch {
    pub name: String,
    pub position: usize,
    pub positions: Vec<SwitchPosition>,
}

/// A control's labelled position, selectable only when the engine would accept its command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchPosition {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<crate::Command>,
}

/// A card's summary as a room shows it (fac-594): its board description, its dated notes left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub card: crate::CardId,
    pub title: String,
    pub text: String,
}

/// The preview's current state and the availability of its opposite position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewControl {
    pub on: bool,
    pub url: String,
    pub enabled: bool,
}

/// Defaults and mode availability for a board card's next run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartDefaults {
    pub level: u8,
    pub from: String,
    pub visual: bool,
    pub carries: Vec<crate::CardId>,
    pub modes: StartModes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartModes {
    pub full: bool,
    pub report: bool,
}
