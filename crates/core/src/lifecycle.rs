//! A card's run, from its start to its end: its brief, the replies and heads that move it through its gates,
//! its fixes, plan, mock step, stops and close.

pub mod baseline;
pub mod branches;
pub mod brief;
pub mod carried;
pub mod facts;
pub mod fixes;
pub mod gates;
pub mod lookback;
pub mod mock;
pub mod overlaps;
pub mod plan;
pub mod preview;
pub mod rebrief;
pub mod reopen;
pub mod replies;
pub mod retired;
pub mod start;
pub mod stops;
pub mod visual;

use mork_types::card::Card;
use mork_types::state::Sender;
use mork_types::{CardId, Command};

use crate::decide::{Ctx, Decision};
use crate::state::State;

/// One of a card's run's commands, past the buzz.
pub fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::Start { card, mode, visual, model, effort, delegated, self_sustained, no_planner, carries } = cmd {
        let foreman = (model.as_deref(), effort.as_deref());
        let options = start::Options {
            visual: visual.unwrap_or(false),
            delegated: delegated.unwrap_or(false),
            self_sustained: self_sustained.unwrap_or(false),
            no_planner: no_planner.unwrap_or(false),
            carries: carries.iter().map(|k| CardId::new(k.as_str().to_lowercase())).collect(),
        };
        let card = card.as_str().to_lowercase();
        start::start(state, from, &card, *mode, options, foreman, ctx)
    } else if let Command::Lookback { project, read } = cmd {
        lookback::start(state, project, read.as_deref(), ctx)
    } else if let Command::Approve { card } = cmd {
        brief::approve(state, from, card.as_str(), ctx)
    } else if let Command::Mock { card } = cmd {
        reopen::reopen(state, card, ctx)
    } else if let Command::Brief { card, words } = cmd {
        rebrief::back(state, from, card, words.as_deref(), ctx)
    } else if let Command::Stop { card, agent } = cmd {
        stops::stop_or_resume(state, true, card.as_ref(), agent.as_ref(), ctx)
    } else if let Command::Resume { card, agent } = cmd {
        stops::stop_or_resume(state, false, card.as_ref(), agent.as_ref(), ctx)
    } else if let Command::Close { card, why } = cmd {
        stops::close(state, &card.as_str().to_lowercase(), why, ctx)
    } else if let Command::Done { card } = cmd {
        stops::done(state, from, &card.as_str().to_lowercase(), ctx)
    } else if let Command::Preview { card, action } = cmd {
        preview::command(state, from, card, action.as_deref(), false, ctx)
    } else if let Command::Demo { card } = cmd {
        preview::command(state, from, card, None, true, ctx)
    } else if let Command::Visual { card, off } = cmd {
        visual::toggle(state, card, !off.unwrap_or(false), ctx)
    } else if matches!(cmd, Command::Deliverables { .. } | Command::Plan { .. } | Command::PlanTask { .. }) {
        plan::command(state, from, cmd, ctx)
    } else {
        Decision::refused(format!("`{}` isn't a card's run's command", cmd.name()))
    }
}

fn card(state: &State, id: &str) -> Result<Card, Decision> {
    state
        .cards
        .get(&CardId::new(id.to_lowercase()))
        .cloned()
        .ok_or_else(|| Decision::refused(format!("no card `{id}`")))
}
