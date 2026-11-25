//! The session tree (fac-96, fac-168): DMs have no row of their own, and a folded card's
//! heading stands for everything under it.

use std::collections::{BTreeMap, BTreeSet};

use factory_types::view::{HandView, PlaceKind, RoomRow, Screen};

/// A line of the rooms column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line<'a> {
    /// A project's heading (fac-168).
    Project(&'a str),
    /// A card's heading, its fold, and the place it opens: its first row's (fac-168).
    Card {
        card: &'a str,
        place: &'a str,
        folded: bool,
        /// Its PR and DevOps tasks, shown beside the id (fac-362); empty when it has none.
        detail: &'a str,
    },
    Row(&'a RoomRow),
}

impl<'a> Line<'a> {
    /// The place the line stands for, which Ctrl+↑ and Ctrl+↓ walk to: a row's, a folded card's.
    pub fn stands(&self) -> Option<&'a str> {
        match *self {
            Line::Row(r) => Some(r.place.as_str()),
            Line::Card { place, folded: true, .. } => Some(place),
            Line::Project(_) | Line::Card { .. } => None,
        }
    }
}

/// The column's lines, in order: each project's and card's heading before its first row, and the
/// rows that show.
pub fn lines<'a>(screen: &'a Screen, collapsed: &BTreeSet<String>) -> Vec<Line<'a>> {
    let mut out = Vec::new();
    let mut last = None;
    for r in &screen.rooms {
        if let Some(u) = &r.under
            && last.is_none_or(|(p, c)| (p, c) != (u.project.as_str(), u.card.as_str()))
        {
            if last.is_none_or(|(p, _)| p != u.project.as_str()) {
                out.push(Line::Project(&u.project));
            }
            let folded = collapsed.contains(u.card.as_str());
            out.push(Line::Card { card: u.card.as_str(), place: r.place.as_str(), folded, detail: &u.detail });
        }
        last = r.under.as_ref().map(|u| (u.project.as_str(), u.card.as_str()));
        if shows(r, collapsed) {
            out.push(Line::Row(r));
        }
    }
    out
}

// A place's row: a DM's room's; a place with no row yet (a DM not yet opened) its card's when its
// words name a card with one.
fn own_row<'a>(screen: &'a Screen, place: &str) -> Option<&'a RoomRow> {
    let row = |p: &str| screen.rooms.iter().find(|r| r.place == p);
    match row(place) {
        Some(RoomRow { kind: PlaceKind::Dm { card, .. }, .. }) => row(card.as_str()),
        Some(r) => Some(r),
        None => row(place.split_once('/')?.0),
    }
}

/// The line standing for a place: its row, a DM's room's, or the heading of its folded card.
pub fn row_of(screen: &Screen, collapsed: &BTreeSet<String>, place: &str) -> Option<String> {
    let r = own_row(screen, place)?;
    match &r.under {
        Some(u) if collapsed.contains(u.card.as_str()) => heading(screen, u.card.as_str()).map(ToOwned::to_owned),
        _ => Some(r.place.to_string()),
    }
}

/// The room a place is part of: a DM's or a child's card's, else its own row, as `row_of`.
pub fn room_of(screen: &Screen, place: &str) -> Option<String> {
    let r = own_row(screen, place)?;
    match &r.kind {
        PlaceKind::Child { card, .. } => own_row(screen, card.as_str()).map(|r| r.place.to_string()),
        _ => Some(r.place.to_string()),
    }
}

/// The place a card's heading opens and stands for while folded: its first row's.
pub fn heading<'a>(screen: &'a Screen, card: &str) -> Option<&'a str> {
    let under = |r: &&RoomRow| r.under.as_ref().is_some_and(|u| u.card.as_str() == card);
    screen.rooms.iter().find(under).map(|r| r.place.as_str())
}

/// A DM's or a child's card.
pub fn card_of(kind: &PlaceKind) -> Option<&str> {
    match kind {
        PlaceKind::Dm { card, .. } | PlaceKind::Child { card, .. } => Some(card.as_str()),
        PlaceKind::Main
        | PlaceKind::Card { .. }
        | PlaceKind::Spec { .. }
        | PlaceKind::Challenge { .. }
        | PlaceKind::Other => None,
    }
}

/// Whether a row shows in the rooms column: a DM's never, a folded card's not.
pub fn shows(r: &RoomRow, collapsed: &BTreeSet<String>) -> bool {
    match (&r.kind, &r.under) {
        (PlaceKind::Dm { .. }, _) => false,
        (_, Some(u)) => !collapsed.contains(u.card.as_str()),
        (_, None) => true,
    }
}

// An ask whose place has no row: its card's room, else main.
fn fallback(screen: &Screen, h: &HandView) -> String {
    let card = h.card.as_ref().filter(|c| screen.rooms.iter().any(|r| r.place == c.as_str()));
    card.map_or_else(|| "main".to_owned(), ToString::to_string)
}

/// Each line's asks, in the engine's order (red first, then oldest): the asks of the place it
/// stands for and its DMs', and a folded card's heading everything under it.
pub fn asks_by_row<'a>(screen: &'a Screen, collapsed: &BTreeSet<String>) -> BTreeMap<String, Vec<&'a HandView>> {
    let mut out: BTreeMap<String, Vec<&HandView>> = BTreeMap::new();
    for h in &screen.hands {
        let row = row_of(screen, collapsed, h.place.as_str());
        let row = row.or_else(|| row_of(screen, collapsed, &fallback(screen, h))).unwrap_or_else(|| "main".into());
        out.entry(row).or_default().push(h);
    }
    out
}

/// Whether a place lists an ask with its actions: its own, and in a card's room its DMs' and
/// children's too; main lists those whose place and card have no row.
pub fn lists(screen: &Screen, place: &str, h: &HandView) -> bool {
    h.place == place || room_of(screen, h.place.as_str()).unwrap_or_else(|| fallback(screen, h)) == place
}
