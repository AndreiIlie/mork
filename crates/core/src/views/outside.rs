//! What the engine read of the world for the views, none of it in the state.

use std::collections::BTreeMap;

use factory_types::lookback::Read;
use factory_types::view::{Phase, PinLine, Resources, Share, SuiteProgress};
use factory_types::{AgentKey, CardId, JobId, ProjectId, Sha};

use super::timeline::Point;

/// The cursor commit and its count, by project and look-back read.
pub type LookbackCounts = BTreeMap<(ProjectId, Read), (Sha, u32)>;

/// What the engine read of the world for the views: none of it is in the state.
#[derive(Debug, Clone, Default)]
pub struct Outside {
    pub tz_offset_min: i32,
    pub pin: PinLine,
    pub phase: Phase,
    pub usage: Vec<Share>,
    pub resources: Option<Resources>,
    /// Each live card run's timeline marks, from its journal (4.19).
    pub timelines: BTreeMap<CardId, Vec<Point>>,
    /// How many commits the base is ahead of each card's head, when known.
    pub base_ahead: BTreeMap<CardId, u32>,
    pub lookback_counts: LookbackCounts,
    /// What each agent's latest turn showed: its steps, what it does now, its context.
    pub agents: BTreeMap<AgentKey, AgentSeen>,
    /// Each card's current run's cost so far, in cents, and the cards with a turn's cost in it
    /// unknown.
    pub card_costs: BTreeMap<CardId, u64>,
    pub card_costs_unknown: std::collections::BTreeSet<CardId>,
    /// Each current run's journal sums by seat, including seats already gone (fac-526).
    pub seat_costs: BTreeMap<CardId, Vec<factory_types::about::SeatCost>>,
    /// Each card's clone folder, where a finding's `file:line` opens (5.7.5).
    pub clones: BTreeMap<CardId, String>,
    /// The agents whose `factory test` waits for a slot now (4.9).
    pub slot_waits: std::collections::BTreeSet<AgentKey>,
    /// The files the card's agents' turns produced, newest first (fac-105).
    pub outputs: BTreeMap<CardId, Vec<factory_types::seen::OutputFile>>,
    /// Each live card's suite runs' counts so far, by job (fac-697).
    pub suites: BTreeMap<JobId, SuiteProgress>,
}

/// An agent's latest turn, as the engine read its stream (1.4.8).
#[derive(Debug, Clone, Default)]
pub struct AgentSeen {
    pub steps: Vec<String>,
    pub doing: Option<String>,
    pub context: Option<(u64, u64)>,
    /// Its latest turn's steps with their text and thinking (fac-96).
    pub turn: Option<std::sync::Arc<factory_types::seen::TurnView>>,
    /// Its own step list, as its latest turn's stream left it (fac-128).
    pub list: Option<factory_types::seen::StepList>,
}
