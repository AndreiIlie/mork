//! The merge train at the engine's start (fac-68): an interrupted fetch is asked again, and owed imports recover.

use mork_types::card::{Card, Landing, Stage};

use crate::post::Poster;

/// At the start, in order: the trains' cards, then the joins.
pub fn recover(p: &mut Poster<'_>) {
    trains(p);
    super::joins::recover(p);
}

// A fetch the train waited for is asked again.
fn trains(p: &mut Poster<'_>) {
    let cards: Vec<Card> = p.state.cards.values().filter(|c| c.stage.is_live()).cloned().collect();
    for mut c in cards {
        if let (Some(Landing::Fetch { at }), Stage::Merging) = (&c.landing, c.stage) {
            c.landing = Some(Landing::Queued { at: *at });
            p.put_card(c);
        }
    }
}
