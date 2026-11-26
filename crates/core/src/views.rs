//! What is true, shown: the screen, a place's view, the commands offered, and the read commands, each worked out from the
//! state and the engine's reads, changing nothing.

mod boards;
pub mod burn;
pub mod conversation;
pub(crate) mod findings;
mod header;
pub mod journal;
mod live;
pub mod offers;
mod order;
pub mod outside;
mod overview;
mod pipeline;
pub mod place;
mod projects;
mod reads;
pub mod screen;
pub mod search;
pub mod status;
pub mod timeline;

use mork_types::Command;

use crate::decide::{Ctx, Decision};
use crate::state::State;

/// One of the reads, by its command, past the buzz.
pub fn command(state: &State, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::CardShow { card: id } = cmd {
        reads::card(state, id.as_str())
    } else if let Command::Findings { card: id } = cmd {
        findings::findings(state, id.as_str())
    } else if matches!(cmd, Command::Projects {}) {
        reads::projects(state, ctx)
    } else if matches!(cmd, Command::Actions {}) {
        reads::actions(state)
    } else if let Command::Prs { project } = cmd {
        reads::prs(state, project, ctx)
    } else {
        Decision::refused(format!("`{}` isn't a read", cmd.name()))
    }
}
