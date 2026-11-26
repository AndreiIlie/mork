use mork_types::CardId;
use mork_types::card::Card;
use mork_types::note::{Note, NoteWhat};

use crate::post::Poster;
use crate::state::State;

/// The folder in a card's clone a foreman leaves its handoff in, for the orchestrator.
pub const HANDOFF: &str = ".git/mork/handoff";

/// Where a card's run's handoff is kept, under its project's records.
pub fn handoff_dir(card: &CardId, run: u32) -> String {
    format!("handoffs/{card}-{run}")
}

/// The live cards across projects whose title, description or notes cite the handoff's card, or those of a card one of
/// them carries (fac-563).
pub(super) fn citing_cards(state: &State, handoff: &CardId) -> Vec<CardId> {
    state
        .cards
        .values()
        .filter(|card| {
            card.stage.is_live() && crate::lifecycle::carried::cards(card).any(|id| cites(state, card, id, handoff))
        })
        .map(|card| card.id.clone())
        .collect()
}

// One of a run's cards, its own or one it carries, read from its board, else as the run saved it.
fn cites(state: &State, card: &Card, id: &CardId, handoff: &CardId) -> bool {
    let on_board = state.board.get(id);
    let saved = card
        .carries
        .iter()
        .find(|k| k.id == *id)
        .map_or((&card.title, &card.description), |k| (&k.title, &k.description));
    let named = |text: &str| crate::internals::whole(text, handoff.as_str());
    if named(on_board.map_or(saved.0, |b| &b.title)) {
        return true;
    }
    let file = on_board
        .and_then(|b| crate::board::read(&b.text, &crate::board::writes::written(state, &b.project, &b.file)).ok());
    match file {
        Some(file) => named(&file.body) || file.notes.iter().any(|note| named(note)),
        None => named(saved.1),
    }
}

/// A card's clone went, its handoff copied first: what it held is filed as a note of its card.
pub(crate) fn handed_off(p: &mut Poster<'_>, card: &CardId, run: u32, v: &serde_json::Value) {
    let Some(h) = v.get("handoff").filter(|h| !h.is_null()) else { return };
    let filed =
        |n: &Note| n.card.as_ref() == Some(card) && matches!(n.what, NoteWhat::Handoff { run: r, .. } if r == run);
    // A removal run again after a crash finds the copy it recorded: it is filed once.
    if p.state.notes.values().any(filed) {
        return;
    }
    let files = serde_json::from_value(h["files"].clone()).unwrap_or_default();
    let skipped = serde_json::from_value(h["skipped"].clone()).unwrap_or_default();
    let dir = h["dir"].as_str().unwrap_or_default().to_owned();
    let project = p.card(card).map(|c| c.project);
    super::notes::put(p, project, Some(card.clone()), None, NoteWhat::Handoff { run, dir, files, skipped });
}
