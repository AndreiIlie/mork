//! The factory's upkeep of itself: its own updates and reloads, what they brought, the manuals they carry and the
//! material compared across them, the store's backups and pruning, and the outward actions held for Andrei.

pub mod changelog;
pub mod manuals;
pub mod material;
pub mod reload;
pub mod updates;
pub mod upkeep;

use factory_types::Command;

use crate::decide::{Ctx, Decision};
use crate::state::State;

/// `action.retry`, `action.drop` or `action.done`, past the buzz.
pub fn command(state: &State, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::ActionRetry { action } = cmd {
        crate::decide::actions::settle_held(state, action, "retry", None, ctx)
    } else if let Command::ActionDrop { action } = cmd {
        crate::decide::actions::settle_held(state, action, "drop", None, ctx)
    } else if let Command::ActionDone { action, note } = cmd {
        crate::decide::actions::settle_held(state, action, "done", Some(note), ctx)
    } else {
        Decision::refused(format!("`{}` isn't a held action's command", cmd.name()))
    }
}
