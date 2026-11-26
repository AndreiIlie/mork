// fac-312 removed the standalone researchers: an older state's transcripts stay, its research
// records are rows no build reads, and its runtime records are skipped as it loads, never resumed.

use mork_core::state::State;
use mork_types::action::{Action, ActionWhat};
use mork_types::card::{Card, Stage};
use mork_types::state::Sender;
use rusqlite::{Transaction, params};
use serde_json::Value;

// A new run must not make its archived builder seats eligible for live loading.
pub(super) fn drop_archived_builders(tx: &Transaction<'_>, card: &Card) -> super::Result<()> {
    if card.stage == Stage::Starting && card.run > 1 {
        tx.execute(
            "DELETE FROM entities
             WHERE kind = 'agent' AND id > ?1 || '/' AND id < ?1 || '0'
             AND json_extract(data, '$.role') = 'builder'
             AND EXISTS (SELECT 1 FROM entities AS prior WHERE prior.kind = 'card' AND prior.id = ?1
                 AND json_extract(prior.data, '$.stage') IN ('done', 'closed')
                 AND json_extract(prior.data, '$.run') < ?2)",
            params![card.id.as_str(), card.run],
        )?;
    }
    Ok(())
}

const PURPOSES: [&str; 2] = ["research", "research-install"];

// Its purposes are no variant this build decodes: their records would stop the state loading.
pub(super) fn research_purpose(purpose: &Value) -> bool {
    purpose.get("purpose").and_then(Value::as_str).is_some_and(|p| PURPOSES.contains(&p))
}

// Retired steps' asks are of no kind this build decodes; startup recovery resumes their cards
// without those steps (fac-329, fac-411).
pub(super) fn step_ask(ask: &Value) -> bool {
    matches!(ask.get("kind").and_then(Value::as_str), Some("architecture" | "preview"))
}

// fac-591 retired the mocker seat: its role is no variant this build decodes, and the start hands its mock to the
// planner.
pub(super) fn mocker(agent: &Value) -> bool {
    agent.get("role").and_then(Value::as_str) == Some("mocker")
}

// A research's place is `r-<n>`, of no card; its agents' keys and its jobs' owners begin with it.
fn research_place(s: &State, key: &str) -> bool {
    let place = key.split('/').next().unwrap_or_default();
    let n = place.strip_prefix("r-").unwrap_or_default();
    let card = mork_types::CardId::new(place);
    !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
        && !s.cards.contains_key(&card)
        && !s.unloaded.cards.contains(&card)
}

// Nothing of a research may wake, hold a slot or wait on an answer: no researcher would take it.
pub(super) fn drop_research(s: &mut State) {
    let mut agents = std::mem::take(&mut s.agents);
    agents.retain(|k, _| !research_place(s, k.as_str()));
    s.agents = agents;
    let mut asks = std::mem::take(&mut s.asks);
    asks.retain(|_, a| !research_place(s, a.place.as_str()));
    s.asks = asks;
    let mut leases = std::mem::take(&mut s.leases);
    leases.retain(|_, l| !research_place(s, l.holder.as_str()));
    s.leases = leases;
    let mut jobs = std::mem::take(&mut s.jobs);
    jobs.retain(|_, j| !research_place(s, j.owner.as_str()));
    s.jobs = jobs;
    let mut actions = std::mem::take(&mut s.actions);
    actions.retain(|_, a| !research_door(s, a));
    s.actions = actions;
}

// A door call of a research would run on startup and drop its news: its requester is gone.
fn research_door(s: &State, a: &Action) -> bool {
    let ActionWhat::Door(call) = &a.what else { return false };
    research_place(s, call.place.as_str()) || matches!(&call.from, Sender::Agent(k) if research_place(s, k.as_str()))
}
