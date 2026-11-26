// A command's decision: its authority first (the buzz), then the area its catalog declaration names.

use mork_types::Command;
use mork_types::catalog::Area;
use mork_types::state::Sender;

use super::{Ctx, Decision, buzz};
use crate::state::State;

/// Every command, the engine's own included, is buzzed before any area decides it (3.6.3, M3.20).
pub fn decide(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Some(e) = buzz::buzz(state, from, cmd, ctx) {
        return buzz::buzzed(from, cmd, e);
    }
    let Some(spec) = cmd.spec() else { return elsewhere(cmd) };
    match spec.area {
        Area::Engine => elsewhere(cmd),
        Area::Queue => crate::queue::command(state, from, cmd, ctx),
        Area::Door => crate::door::command(state, cmd, ctx),
        Area::Lifecycle => crate::lifecycle::command(state, from, cmd, ctx),
        Area::Reviews => crate::reviews::command(state, from, cmd, ctx),
        Area::Messages => crate::messages::command(state, from, cmd, ctx),
        Area::Agents => crate::agents::command(state, from, cmd, ctx),
        Area::Board => crate::board::command(state, from, cmd, ctx),
        Area::Views => crate::views::command(state, cmd, ctx),
        Area::Memory => crate::memory::command(state, from, cmd, ctx),
        Area::Maintenance => crate::maintenance::command(state, cmd, ctx),
    }
}

// A command the engine answers itself, never sent here past its own handling.
fn elsewhere(cmd: &Command) -> Decision {
    Decision::refused(format!("`{}` isn't decided here", cmd.name()))
}
