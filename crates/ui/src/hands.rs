//! The open session's asks (5.9, fac-96) in the engine's order (red first, then oldest). A card's
//! room lists its DMs' and children's asks too, and main those with no row of their own.

use factory_types::view::{HandView, PlaceKind};
use iced::widget::{Column, column, container, responsive, row};
use iced::{Element, Length, Padding};

use crate::app::{App, Message};
use crate::icons::{self, Icon};
use crate::spans::{self, Tone};
use crate::widgets::{self, bold, boxed_button, clickable, txt};
use crate::{fonts, text, theme, tree};

/// A hand's card: its padding inside its edge, top and bottom, and left and right (fac-66).
pub const CARD_Y: f32 = 6.0;
pub const CARD_X: f32 = 12.0;
const HAND_GAP: f32 = 6.0;
const MARKER_GAP: f32 = 6.0;
const GAP_CHARS: usize = 2;
// Beside its buttons, its words take at least this many characters; a button's edges and padding
// take about three more than its words; stacked, its buttons sit this far under its words.
const MIN_WORDS: usize = 40;
const BUTTON_CHARS: usize = 3;
const BUTTONS_GAP: f32 = 4.0;
// What a given ask shows in its orchestrator button's place (fac-119).
const GIVEN: &str = "sent to orchestrator";
// A foreman's design question's tag, after its role (fac-233).
const DESIGN: &str = " design";
/// How long a newly raised hand's marker blinks: three times 250 ms on and 250 ms off.
pub(crate) const BLINK_MS: i64 = 1500;

/// Whether a newly raised hand's marker shows now: it blinks three times, 250 ms on and 250 ms
/// off, from when the window first saw it (5.11.2).
pub fn blink_on(seen_ms: i64, now_ms: i64, animations: bool) -> bool {
    let since = now_ms.saturating_sub(seen_ms);
    if !animations || !(0..BLINK_MS).contains(&since) {
        return true;
    }
    (since / 250) % 2 == 0
}

/// The asks the open place lists, in the engine's order.
pub fn of(app: &App) -> Vec<&HandView> {
    app.screen.hands.iter().filter(|h| tree::lists(&app.screen, &app.session.place, h)).collect()
}

/// The open place's asks as cards under `max_h`, when it lists any.
pub fn view(app: &App, max_h: f32) -> Option<Element<'_, Message>> {
    let asks = of(app);
    if asks.is_empty() {
        return None;
    }
    let list = Column::with_children(asks.into_iter().map(|h| hand(app, h))).spacing(HAND_GAP);
    // Inset so the first and last cards' edges stand clear of the scroll's.
    let list = widgets::scroll(container(list).padding([HAND_GAP, 0.0])).height(Length::Shrink);
    Some(container(list).id("asks").max_height(max_h).into())
}

// Its words beside its buttons where the card is wide enough, else its buttons under its words.
fn hand<'a>(app: &'a App, h: &'a HandView) -> Element<'a, Message> {
    let tone = spans::light(Some(h.color));
    let inside = move |size: iced::Size| {
        let avail = widgets::chars_in(size.width - icons::SIZE - MARKER_GAP);
        line(app, h, tone, avail)
    };
    let card = container(responsive(inside).height(Length::Shrink)).padding([CARD_Y, CARD_X]);
    clickable(format!("hand:{}", h.ask), card.width(Length::Fill).style(theme::card(Tone::Edge)), None)
}

fn line<'a>(app: &'a App, h: &'a HandView, tone: Tone, avail: usize) -> Element<'a, Message> {
    let ask = h.ask.to_string();
    let seen = app.raised.get(&ask).copied().unwrap_or(i64::MIN);
    let marker = if blink_on(seen, app.now_ms, app.settings.animations) {
        icons::icon(Icon::Hand, tone)
    } else {
        widgets::gap(icons::SIZE)
    };
    let marker = container(marker).height(Length::Fixed(fonts::LINE)).align_y(iced::alignment::Vertical::Center);
    let role = h.role.map(|r| r.as_str()).unwrap_or_default();
    let cold = h
        .cold_at_ms
        .and_then(|at| spans::hand_cold(at, app.now_ms))
        .map(|words| if role.is_empty() && !h.design { words } else { format!(" {words}") });
    let mut who = row![txt(role, Tone::Dim)];
    if h.design {
        who = who.push(txt(DESIGN, Tone::Orange));
    }
    if let Some(cold) = &cold {
        who = who.push(txt(cold.clone(), Tone::Orange));
    }
    let place = where_raised(app, h);
    let role_chars = role.chars().count()
        + if h.design { DESIGN.chars().count() } else { 0 }
        + cold.as_ref().map_or(0, |c| c.chars().count());
    let where_chars = place.chars().count().max(role_chars);
    let head = column![bold(place, Tone::Bright), who];
    let offers = offers(app, h);
    let given = h.given.then_some(GIVEN);
    let labels = offers.iter().map(|(_, label, _)| label.as_str()).chain(given);
    let used = labels.map(|l| l.chars().count() + BUTTON_CHARS + GAP_CHARS).sum::<usize>();
    let beside = avail.saturating_sub(where_chars + 2 * GAP_CHARS + used);
    let stacked = beside < MIN_WORDS && (!offers.is_empty() || h.given);
    let width = if stacked { avail.saturating_sub(where_chars + GAP_CHARS) } else { beside };
    let lines = words(&text::clean(&h.words), width);
    let words = Column::with_children(lines.into_iter().map(|l| txt(l, Tone::Text).into()));
    let open = Message::OpenAsk { place: h.place.to_string(), ask: ask.clone() };
    let mut body = column![clickable(format!("handwords:{ask}"), words, Some(open))];
    let gap = || widgets::gap(GAP_CHARS as f32 * fonts::ADVANCE);
    let mut line = row![marker, widgets::gap(MARKER_GAP), container(head).width(chars(where_chars)), gap()];
    if stacked {
        body = body.push(container(buttons(offers, given)).padding(Padding::ZERO.top(BUTTONS_GAP)));
        line = line.push(container(body).width(chars(width)));
    } else {
        line = line.push(container(body).width(chars(width))).push(gap()).push(buttons(offers, given));
    }
    line.into()
}

fn chars(n: usize) -> Length {
    Length::Fixed(n as f32 * fonts::ADVANCE)
}

// The place an ask was raised in, as its card's room calls it: a DM, a child by its name.
fn where_raised(app: &App, h: &HandView) -> String {
    let kind = app.screen.rooms.iter().find(|r| r.place == h.place).map(|r| &r.kind);
    match kind {
        Some(PlaceKind::Dm { role, .. }) => format!("dm {}", role.as_str()),
        Some(PlaceKind::Spec { .. }) => "spec".to_owned(),
        Some(PlaceKind::Child { agent, .. }) => agent.as_str().rsplit('/').next().unwrap_or_default().to_owned(),
        _ => h.place.to_string(),
    }
}

// Its buttons, by id, words and press. One raised elsewhere opens its place.
fn offers(app: &App, h: &HandView) -> Vec<(String, String, Message)> {
    let ask = h.ask.as_str();
    let mut out = Vec::new();
    for (i, o) in h.offers.iter().enumerate() {
        let press = Message::Press {
            ask: Some(ask.to_owned()),
            place: h.place.to_string(),
            label: o.label.clone(),
            command: Box::new(o.command.clone()),
        };
        out.push((format!("hand:{ask}:{i}"), o.label.clone(), press));
    }
    if h.place != app.session.place.as_str() {
        let open = Message::OpenAsk { place: h.place.to_string(), ask: ask.to_owned() };
        out.push((format!("hand:{ask}:open"), "open".to_owned(), open));
    }
    out
}

// Its buttons, and a given ask's mark after them: boxed as a button is, in grey, and no button.
fn buttons<'a>(offers: Vec<(String, String, Message)>, given: Option<&'static str>) -> Element<'a, Message> {
    let mut r = row![].spacing(GAP_CHARS as f32 * fonts::ADVANCE);
    for (id, label, press) in offers {
        r = r.push(boxed_button(id, label, Tone::Green, press));
    }
    if let Some(words) = given {
        r = r.push(container(txt(words, Tone::Dim)).padding([1.0, 8.0]).style(theme::mark(Tone::Dim, false)));
    }
    r.wrap().into()
}

/// Each source line wrapped at `width` characters, splitting wide words without cutting text.
pub fn words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut current = String::new();
        for word in line.split_whitespace() {
            if !current.is_empty() {
                if current.chars().count() + 1 + word.chars().count() <= width {
                    current.push(' ');
                } else {
                    lines.push(std::mem::take(&mut current));
                }
            }
            for c in word.chars() {
                if current.chars().count() == width {
                    lines.push(std::mem::take(&mut current));
                }
                current.push(c);
            }
        }
        lines.push(current);
    }
    lines
}
