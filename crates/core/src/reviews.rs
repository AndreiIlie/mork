//! Review cards, of a colleague's PR or branch, and our cards' PR reviews.

pub mod cards;
pub(crate) mod checks;
pub mod end;
pub mod findings;
pub mod gate;
pub(crate) mod history;
pub(crate) mod responses;
pub mod results;
pub mod rounds;
// ESLint core rules whose findings are house style in a colleague's review (fac-220).
mod style_rules;
mod threads;
pub mod tickets;
pub mod triage;
pub mod verdict;
pub mod watch;

use mork_types::Command;
use mork_types::state::Sender;

use crate::decide::{Ctx, Decision};
use crate::state::State;

/// One of the reviews' commands, past the buzz.
pub fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::Review { project, pr } = cmd {
        cards::review(state, project, *pr, false, ctx)
    } else if let Command::ReviewMerge { project, pr } = cmd {
        cards::review(state, project, *pr, true, ctx)
    } else if let Command::ReviewBranch { project, branch } = cmd {
        cards::branch(state, project, branch, ctx)
    } else if let Command::Post { card } = cmd {
        findings::post(state, &card.as_str().to_lowercase(), ctx)
    } else if let Command::Resolve { card, n, declined, dropped } = cmd {
        match (declined.unwrap_or(false), dropped.as_deref()) {
            (true, Some(_)) => Decision::refused("`declined` and `dropped` are different resolutions: choose one"),
            (false, Some(text)) => responses::respond(state, from, card, (*n, text, true), ctx),
            (declined, None) => findings::resolve(state, from, card, *n, declined, ctx),
        }
    } else if let Command::Answer { card, n, text } = cmd {
        responses::respond(state, from, card, (*n, text, false), ctx)
    } else if let Command::Rereview { card } = cmd {
        cards::rereview(state, card, ctx)
    } else if let Command::Triage { card, fix, decline, foreman } = cmd {
        triage::answer(state, from, card, fix, decline, foreman, ctx)
    } else {
        Decision::refused(format!("`{}` isn't a review's command", cmd.name()))
    }
}
