//! The agents' memories (fac-159), managed through `mork ctl`, and the
//! projects' notes (fac-287), each with the card and seat it came from, apart from the board's own notes.

mod handoffs;
mod lessons;
mod notes;
mod search;
mod similar;

use mork_types::state::Sender;
use mork_types::{Command, ProjectId};

use crate::decide::{Ctx, Decision};
use crate::state::State;
use lessons::{Written, found, known};

pub(crate) use handoffs::handed_off;
pub use handoffs::{HANDOFF, handoff_dir};
pub(crate) use lessons::reviewed;
pub use lessons::view;
pub use notes::view as notes_view;
pub(crate) use notes::{origin, put, under};
pub(crate) use search::index;

pub(crate) fn lesson_note_help(mork: Option<&ProjectId>) -> String {
    match mork {
        Some(mork) => format!(
            "File a lesson about your card's project with `mork ctl notes.add lesson '<text>'`. File lessons about Mork or intended for `general` with `mork ctl notes.add lesson --project {mork} '<text>'`. The receiving orchestrator triages the note."
        ),
        None => "File a lesson about your card's project with `mork ctl notes.add lesson '<text>'`. The receiving orchestrator triages the note.".into(),
    }
}

/// One of the memory commands, or the projects' notes'.
pub fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if matches!(cmd, Command::NotesAdd { .. } | Command::Notes { .. } | Command::NotesDelete { .. }) {
        return notes::command(state, from, cmd, ctx);
    }
    let decided = if let Command::MemoryRead { memory, name } = cmd {
        found(state, memory, name, ctx).map(|f| lessons::read(state, from, f, ctx.now))
    } else if let Command::MemorySearch { words } = cmd {
        Ok(Decision::ok(search::search(state, from, words)))
    } else if let Command::MemoryWrite { memory, name, description, text, new } = cmd {
        lessons::write(state, from, Written { memory, name, description, text, new: new.unwrap_or(false) }, ctx)
    } else if let Command::MemoryDelete { memory, name } = cmd {
        found(state, memory, name, ctx).map(|f| lessons::delete(state, f))
    } else if let Command::MemoryStats { memory } = cmd {
        memory
            .as_deref()
            .map_or(Ok(()), |m| known(m, ctx))
            .map(|()| Decision::ok(lessons::stats(state, memory.as_deref(), ctx.config)))
    } else {
        Err(format!("`{}` isn't a memory command", cmd.name()))
    };
    decided.unwrap_or_else(Decision::refused)
}
