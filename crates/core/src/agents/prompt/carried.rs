// The run's cards in a seat's prompt (fac-563): each card it carries after its own, with its acceptance.

use std::fmt::Write as _;

use factory_types::card::Card;

use crate::state::State;

pub(super) fn accepts(acceptance: &[String], out: &mut String) {
    if !acceptance.is_empty() {
        out.push_str("\nAcceptance:\n");
        for line in acceptance {
            let _ = writeln!(out, "- {line}");
        }
    }
}

// The cards the run carries, after its own, as their boards hold them; a blind seat reads their decisions apart.
pub(super) fn carried(state: &State, c: &Card, blind: bool, out: &mut String) {
    for k in &c.carries {
        let board = state.board.get(&k.id);
        let (title, description, acceptance) =
            board.map_or((&k.title, &k.description, &k.acceptance), |b| (&b.title, &b.description, &b.acceptance));
        let (notes, text) = if blind {
            crate::board::notes::apart(state, board, description)
        } else {
            (Vec::new(), description.clone())
        };
        let _ = write!(out, "\n## A card this run carries\n\n{}: {title}\n\n{}\n", k.id, text.trim());
        accepts(acceptance, out);
        out.push_str(&super::standing_decisions(
            &notes,
            "Its later decisions, part of the specification, newest first:",
        ));
    }
}
