// Ended cards' records, read from the store when an event names them (fac-336): `core` decides on
// the state alone, so what it reads of them is taken in first.

use std::collections::BTreeSet;

use mork_core::decide::Event;
use mork_core::state::unloaded::{Wanted, companions, wanted};
use mork_types::CardId;

use super::Engine;

impl Engine {
    /// Takes in what deciding `event` reads that the state doesn't hold.
    pub(super) fn load_wanted(&mut self, event: &Event) {
        let w = wanted(&self.state, event);
        self.load(&w);
    }

    /// Takes in these ended cards' records and these actions from the store.
    pub(super) fn load(&mut self, w: &Wanted) {
        if !w.cards.is_empty() {
            self.absorb(w.cards.clone());
        }
        if !w.actions.is_empty() {
            match self.store.actions_of(&w.actions) {
                Ok(actions) => self.state.take_actions(actions),
                Err(e) => self.log(&format!("stored actions couldn't be read: {e}")),
            }
        }
    }

    // The cards, then the cards deciding on them reads (a review card's, its subject).
    fn absorb(&mut self, mut cards: BTreeSet<CardId>) {
        while !cards.is_empty() {
            let records = match self.store.cards_records(&cards) {
                Ok(r) => r,
                Err(e) => return self.log(&format!("ended cards couldn't be read: {e}")),
            };
            for (id, r) in records {
                self.state.absorb(&id, r);
            }
            let unloaded = &self.state.unloaded.cards;
            let read = cards.iter().filter_map(|c| self.state.cards.get(c));
            cards = read.flat_map(companions).filter(|c| unloaded.contains(c)).collect();
        }
    }
}
