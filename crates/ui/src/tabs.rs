//! A card's tabs (fac-96, fac-633): its spec and build rooms, then its DMs, which have no row and are reached from
//! here. A foreman with no DM yet has a tab that opens one, as `/dm` does.

use iced::Element;
use iced::widget::row;
use mork_types::reply::Role;
use mork_types::view::{HandView, PlaceKind, PlaceView};

use crate::app::{App, Message};
use crate::icons::{self, Icon};
use crate::spans::{self, Tone};
use crate::widgets::{self, bold, clickable, txt};

const SEP: &str = " · ";
// A marker's gap before it; the characters the gap and the marker take, rounded up.
const MARKER_GAP: f32 = 5.0;
const MARKER_CHARS: usize = 3;

/// A tab: its words, the place it opens, and how.
#[derive(Debug, Clone)]
pub struct Tab {
    pub name: String,
    pub place: String,
    pub open: Message,
}

/// The tabs of a card's room or DM: its spec and build rooms, or its one room, its DMs as the rooms list them, its
/// foreman's DM to open when it has none. None for a place of no card, or a room with no other tab.
pub fn of(app: &App, v: &PlaceView) -> Vec<Tab> {
    let card = match &v.kind {
        PlaceKind::Card { card } | PlaceKind::Spec { card } | PlaceKind::Dm { card, .. } => card.as_str(),
        PlaceKind::Challenge { .. } | PlaceKind::Child { .. } | PlaceKind::Main | PlaceKind::Other => {
            return Vec::new();
        }
    };
    let tab = |name: String, place: String| Tab { name, open: Message::Open(place.clone()), place };
    let header = v.card.as_ref();
    let mut tabs = match header.and_then(|h| h.spec.as_ref()) {
        Some(spec) => vec![tab("spec".into(), spec.to_string()), tab("build".into(), card.to_owned())],
        None => vec![tab("room".into(), card.to_owned())],
    };
    let mut foreman = false;
    for r in &app.screen.rooms {
        if let PlaceKind::Dm { card: c, role } = &r.kind
            && c.as_str() == card
        {
            foreman |= *role == Role::Foreman;
            tabs.push(tab(format!("dm {}", role.as_str()), r.place.to_string()));
        }
    }
    // The card's own sessions, whichever of its places is open, hold its room's foreman.
    let seated = v.sessions.iter().any(|a| a.role == Role::Foreman && a.agent.as_str() == format!("{card}/foreman"));
    if !foreman && seated {
        let place = format!("{card}/{}/dm", Role::Foreman.as_str());
        tabs.push(Tab { name: "dm foreman".into(), place, open: Message::OpenDm(card.to_owned()) });
    }
    if tabs.len() < 2 { Vec::new() } else { tabs }
}

/// The tabs' width, in characters.
pub fn chars(app: &App, tabs: &[Tab]) -> usize {
    let marked = tabs.iter().filter(|t| asks(app, &t.place).is_some()).count();
    tabs.iter().map(|t| t.name.chars().count() + SEP.len()).sum::<usize>() + marked * MARKER_CHARS
}

fn asks<'a>(app: &'a App, place: &str) -> Option<&'a HandView> {
    app.screen.hands.iter().find(|h| h.place == place)
}

/// The tabs as a line, wrapping to the right where it can't hold them: the open one bright, each with
/// asks marked in its first's colour.
pub fn view<'a>(app: &'a App, tabs: Vec<Tab>) -> Element<'a, Message> {
    let mut line = row![].align_y(iced::alignment::Vertical::Center);
    for (i, t) in tabs.into_iter().enumerate() {
        let on = t.place == app.session.place;
        let words = if on { bold(t.name, Tone::Bright) } else { txt(t.name, Tone::Dim) };
        let mut tab = row![words].align_y(iced::alignment::Vertical::Center);
        if let Some(h) = asks(app, &t.place) {
            tab = tab.push(widgets::gap(MARKER_GAP)).push(icons::icon(Icon::Hand, spans::light(Some(h.color))));
        }
        let mut cell = row![].align_y(iced::alignment::Vertical::Center);
        if i > 0 {
            cell = cell.push(txt(SEP, Tone::Rule));
        }
        line = line.push(cell.push(clickable(format!("roomtab:{}", t.place), tab, Some(t.open))));
    }
    line.wrap().align_x(iced::alignment::Horizontal::Right).into()
}
