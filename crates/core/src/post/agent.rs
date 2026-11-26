use mork_types::state::{Agent, AgentStatus};

use crate::messages::places::Who;

/// A seat's record, new: no session, no turn owed, nothing seen.
pub fn new_agent(who: &Who) -> Agent {
    Agent {
        key: who.key.clone(),
        seat: who.seat.clone(),
        role: who.role,
        place: who.place.clone(),
        session: None,
        session_started: false,
        needs_specification: false,
        status: AgentStatus::Idle,
        owed: None,
        seen: 0,
        seen_before: 0,
        wake_after: false,
        report_round: None,
        completion_wake: false,
        build_wake: false,
        held_reports: None,
        cuts: 0,
        paused: false,
        last_prompt: None,
        not_before: None,
        costs: std::collections::BTreeMap::new(),
        token_totals: std::collections::BTreeMap::new(),
        cost_job: None,
        window: None,
        context: None,
        fresh: None,
        parent: None,
        topic: None,
        choice: None,
        pending: None,
        retiring: None,
        ran: Default::default(),
        replied: None,
        taken: Vec::new(),
        missed: Vec::new(),
        held: std::collections::BTreeMap::new(),
        io: None,
        last_turn_ms: None,
    }
}
