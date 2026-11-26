//! Pages of a conversation (fac-42): a message's place in its history never changes, so
//! `earlier` counts tell whether two runs of messages meet.

use mork_types::view::{HistoryPage, MessageView, PlaceView};
use mork_types::{Command, PlaceId};

use crate::app::{App, Effect, PAGE};

/// The view the window keeps of a place: the engine's `new`, under what it kept of `old` that
/// comes before it and meets it; `new` alone when they don't meet.
pub fn merged(old: Option<PlaceView>, mut new: PlaceView) -> PlaceView {
    let Some(old) = old else { return new };
    let keep = new.earlier.checked_sub(old.earlier).filter(|k| *k > 0 && *k <= old.messages.len());
    let Some(keep) = keep else { return new };
    let mut kept: Vec<MessageView> = old.messages.into_iter().take(keep).collect();
    // An ask kept is open while the engine pins it: answered or replaced since, it isn't.
    for m in &mut kept {
        m.ask_open = new.pinned.iter().any(|p| p.seq == m.seq);
    }
    let first = kept.first().map_or(0, |m| m.seq);
    new.pinned.retain(|p| p.seq < first);
    let boundary = new.messages.first().map(|m| m.seq);
    kept.append(&mut new.messages);
    crate::activity::merge(old.turns, &mut new.turns, boundary);
    new.messages = kept;
    new.earlier = old.earlier;
    new
}

impl App {
    /// The conversation's top came into view: the page before its oldest message shown is asked
    /// for, once for each.
    pub(crate) fn older_page(&mut self, first: u64) -> Vec<Effect> {
        let place = self.session.place.clone();
        let Some(v) = self.places.get(&place) else { return Vec::new() };
        let asked = self.older == Some(first);
        if v.earlier == 0 || v.messages.first().map(|m| m.seq) != Some(first) || asked {
            return Vec::new();
        }
        self.older = Some(first);
        vec![Effect::Send(Command::History {
            place: PlaceId::new(place),
            before: first,
            count: PAGE,
            turns: Some(true),
        })]
    }

    /// A page `history` answered: above its place's view, when it ends where the view begins.
    pub(crate) fn page(&mut self, page: HistoryPage) {
        let Some(v) = self.places.get_mut(page.place.as_str()) else { return };
        let Some(first) = page.messages.first().map(|m| m.seq) else { return };
        if page.earlier + page.messages.len() != v.earlier {
            return;
        }
        v.pinned.retain(|p| p.seq < first);
        crate::activity::merge(page.turns, &mut v.turns, None);
        let mut messages = page.messages;
        messages.append(&mut v.messages);
        v.messages = messages;
        v.earlier = page.earlier;
    }
}
