//! Agents and their turns (1.4, 3.3): a seat's turns planned, prepared and settled, its replies checked, its session,
//! model and prompts, a foreman's children, its closing turn, and its DM, stop and resume (4.13.7).

pub mod cache;
pub mod children;
pub mod closing;
pub mod costs;
pub mod ends;
pub mod fresh;
pub mod held;
pub mod lesson;
pub mod models;
pub mod prompt;
pub mod recovery;
pub mod reply;
pub mod required;
pub mod seats;
pub mod stalled;
pub mod subagents;
pub mod turns;
pub mod usage;

use factory_types::Command;
use factory_types::reply::Role;
use factory_types::state::Sender;

use crate::decide::{Ctx, Decision};
use crate::state::State;

/// `dm`, `spawn`, `fresh` or `pick`, past the buzz.
pub fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::Dm { card, role } = cmd {
        seats::dm(state, card, *role, ctx)
    } else if matches!(cmd, Command::Spawn { .. }) {
        children::spawn(state, from, cmd, ctx)
    } else if let Command::Fresh { agent } = cmd {
        fresh::fresh(state, from, agent, ctx)
    } else if let Command::Pick { agent, .. } = cmd {
        match planner_of(state, from, agent) {
            Some(e) => Decision::refused(e),
            None => models::pick(state, from, cmd, ctx),
        }
    } else {
        Decision::refused(format!("`{}` isn't an agent's command", cmd.name()))
    }
}

// The planner is the foreman's child in name only: its model stays Andrei's choice (fac-633).
fn planner_of(state: &State, from: &Sender, agent: &str) -> Option<String> {
    let Sender::Agent(key) = from else { return None };
    let me = state.agents.get(key).filter(|a| a.role == Role::Foreman)?;
    let target = state.agents.get(&factory_types::AgentKey::new(agent.trim()))?;
    let card = crate::messages::places::card_of(state, &me.place)?;
    (target.role == Role::Planner && crate::messages::places::card_of(state, &target.place) == Some(card))
        .then(|| format!("{} is its card's planner, whose model only Andrei picks", target.key))
}
