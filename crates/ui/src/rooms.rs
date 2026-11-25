//! The rooms column (5.6, fac-96, fac-168): DMs are reached from their room, and a line's hand
//! marks the open asks of every session it stands for.

use std::collections::{BTreeMap, BTreeSet};

use factory_types::card::Mode;
use factory_types::view::{AgentLight, HandView, PlaceKind, RoomRow, Screen};
use iced::widget::{Column, column, container, responsive};
use iced::{Element, Length};

use crate::app::{App, Message, ScrollOp};
use crate::icons::{self, Icon};
use crate::spans::{self, Tone};
use crate::tree::{self, Line};
use crate::widgets::{self, bold, clickable, txt};
use crate::{fonts, hands, theme, window, wrappers};

/// The id of the rooms column's list, which its wrapper scrolls.
pub const ROOMS: &str = "rooms";
/// A row's padding left and right, inside its band (fac-66).
pub const ROW_PAD: f32 = 6.0;
/// The fold's slot, left of a row's door, and the hand's, left of its squares (fac-96).
pub const FOLD_W: f32 = 10.0;
pub const HAND_W: f32 = 16.0;
// A child's `↳` stands under its room's name.
const CHILD_INDENT: f32 = FOLD_W + 14.0;
// The door's slot and the gap after it.
const DOOR_W: f32 = icons::SIZE + 2.0;
const SQUARES_PER_LINE: usize = 6;
// The column's padding, on each line.
const PAD: iced::Padding = iced::Padding { top: 0.0, right: window::PAD, bottom: 0.0, left: window::PAD - ROW_PAD };
const SQUARE_LINE_GAP: f32 = 2.0;
// Each light square's pitch: 4 px and a 4 px gap.
const SQUARE_PITCH: f32 = 8.0;
// The space above a project's heading, the first's too: a section's rule is the middle of it, a pixel between two
// equal halves.
const SECTION_GAP: f32 = 9.0;

/// The places of the lines Ctrl+↑ and Ctrl+↓ walk, in the column's order (5.6.3): the rows shown
/// and the folded cards' headings. No DM has a row, nor a folded card's room or child.
pub fn order<'a>(screen: &'a Screen, collapsed: &BTreeSet<String>) -> Vec<&'a str> {
    tree::lines(screen, collapsed).iter().filter_map(Line::stands).collect()
}

/// Where the line standing for a place is in the walk.
pub fn slot(screen: &Screen, collapsed: &BTreeSet<String>, place: &str) -> Option<usize> {
    let place = tree::row_of(screen, collapsed, place)?;
    order(screen, collapsed).iter().position(|p| *p == place)
}

/// The column: the list. When it is more than its space holds it scrolls, the open or picked room
/// kept in view (5.6.1).
pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| column_of(app, size)).into()
}

// What every line reads: the id column's width, a card heading's, the squares', the open place's line, each
// line's asks.
struct Look<'a> {
    id_chars: usize,
    card_chars: usize,
    squares: usize,
    open: Option<String>,
    asks: BTreeMap<String, Vec<&'a HandView>>,
}

fn column_of(app: &App, size: iced::Size) -> Element<'_, Message> {
    // Each line takes the panel's padding, so a rule between project groups spans it edge to edge.
    let (height, width) = (size.height, size.width - PAD.left - PAD.right);
    let collapsed = &app.session.collapsed;
    let list = tree::lines(&app.screen, collapsed);
    let squares = app.screen.rooms.iter().map(|r| r.squares.len()).max().unwrap_or(0).min(SQUARES_PER_LINE);
    let look = Look {
        id_chars: id_chars(&app.screen, width, squares),
        card_chars: widgets::chars_in(width - 2.0 * ROW_PAD - FOLD_W - HAND_W - squares as f32 * SQUARE_PITCH),
        squares,
        open: tree::row_of(&app.screen, collapsed, &app.session.place),
        asks: tree::asks_by_row(&app.screen, collapsed),
    };
    let list_h = top_of(&list, list.len());
    let first = list.iter().position(|l| matches!(l, Line::Project(_)));
    let rows = list.iter().enumerate().map(|(i, l)| {
        let padded = container(line(app, &look, *l)).padding(PAD);
        if !matches!(l, Line::Project(_)) {
            return padded.into();
        }
        // The gap above the heading, with the rule across its middle but above the first section.
        let gap: Element<'_, Message> = if first.is_some_and(|f| f < i) {
            let half = (SECTION_GAP - 1.0) / 2.0;
            column![widgets::gap_v(half), widgets::hline(theme::divider), widgets::gap_v(half)].into()
        } else {
            widgets::gap_v(SECTION_GAP)
        };
        column![gap, padded].into()
    });
    let rows = Column::with_children(rows).width(Length::Fill);
    if list_h > height {
        let scroll = widgets::scroll(rows).id(ROOMS);
        container(wrappers::scrolled(scroll, ROOMS, asked(app, &list))).height(Length::Fill).into()
    } else {
        column![rows, iced::widget::space().height(Length::Fill)].into()
    }
}

// The band of the line asked into view: the line standing for its place.
fn asked(app: &App, lines: &[Line<'_>]) -> Option<(u64, ScrollOp)> {
    let (n, place) = app.rooms_scroll.as_ref()?;
    let place = tree::row_of(&app.screen, &app.session.collapsed, place)?;
    let i = lines.iter().position(|l| l.stands() == Some(place.as_str()))?;
    Some((*n, ScrollOp::Show { top: top_of(lines, i), bottom: top_of(lines, i + 1) }))
}

// Where the line at an index starts: each is a line tall, a project's heading with its gap above.
fn top_of(lines: &[Line<'_>], i: usize) -> f32 {
    let gaps = lines[..i].iter().filter(|l| matches!(l, Line::Project(_))).count();
    i as f32 * fonts::LINE + gaps as f32 * SECTION_GAP
}

// The id column fits the longest name but a DM's or a child's, whose row has its own words, within
// what the row leaves it beside the longest stage and a character's gap (fac-122).
fn id_chars(screen: &Screen, width: f32, squares: usize) -> usize {
    let ids: Vec<_> = screen.rooms.iter().filter(|r| tree::card_of(&r.kind).is_none()).collect();
    let longest = ids.iter().map(|r| name(screen, r).chars().count()).max().unwrap_or(4);
    let stage = ids.iter().map(|r| stage(r).chars().count()).max().unwrap_or(0);
    let fixed = 2.0 * ROW_PAD + FOLD_W + DOOR_W + HAND_W + squares as f32 * SQUARE_PITCH;
    longest.min(widgets::chars_in(width - fixed).saturating_sub(stage + 1))
}

fn line<'a>(app: &'a App, look: &Look<'a>, l: Line<'a>) -> Element<'a, Message> {
    match l {
        Line::Project(project) => {
            let name = txt(project, Tone::Dim).width(Length::Fill);
            container(widgets::centred(vec![name.into()])).padding([0.0, ROW_PAD]).into()
        }
        Line::Card { card, place, folded, detail } => heading(app, look, (card, detail), place, folded),
        Line::Row(r) => room(app, look, r),
    }
}

// A click on a card's heading folds or unfolds it, opening nothing (fac-217); Enter on it opens its first room.
// Folded, it stands for everything under it: lit for the open place, with their asks' hand (fac-168).
fn heading<'a>(
    app: &'a App,
    look: &Look<'a>,
    (card, detail): (&'a str, &str),
    place: &'a str,
    folded: bool,
) -> Element<'a, Message> {
    let open = folded && look.open.as_deref() == Some(place);
    let picked = folded && app.selected.as_deref() == Some(place);
    let tone = if open || picked { Tone::Bright } else { Tone::Text };
    let id = if detail.is_empty() {
        widgets::cut_middle(card, look.card_chars)
    } else {
        widgets::cut(&format!("{card} ({detail})"), look.card_chars)
    };
    let name = if open { bold(id, tone) } else { txt(id, tone) };
    let asks = look.asks.get(place).filter(|_| folded);
    let cells = vec![
        fold(app, card),
        name.width(Length::Fill).into(),
        hand(app, place, asks),
        widgets::gap(look.squares as f32 * SQUARE_PITCH),
    ];
    clickable(format!("card:{card}"), band(open, picked, cells), Some(Message::FoldRoom(card.to_owned())))
}

// The row standing for the open place is lit, a DM's for it.
fn room<'a>(app: &'a App, look: &Look<'a>, r: &'a RoomRow) -> Element<'a, Message> {
    let open = look.open.as_deref() == Some(r.place.as_str());
    let picked = app.selected.as_deref() == Some(r.place.as_str());
    let door_tone = if open { Tone::White } else { spans::light(r.light) };
    let name_tone = if open || picked { Tone::Bright } else { Tone::Text };
    let id_w = look.id_chars as f32 * fonts::ADVANCE + fonts::ADVANCE;
    let mut cells: Vec<Element<'a, Message>> = Vec::new();
    match nested(&r.kind) {
        // A foreman's subagent sits under its card's id (fac-94).
        Some(name) => {
            cells.push(widgets::gap(CHILD_INDENT));
            cells.push(child(format!("↳ {name}"), r.topic.as_deref(), name_tone));
        }
        None => {
            cells.push(widgets::gap(FOLD_W));
            cells.push(icons::icon(Icon::Door, door_tone));
            cells.push(widgets::gap(2.0));
            // A longer name is cut in its middle, its kind and its card kept (fac-122).
            let place = widgets::cut_middle(name(&app.screen, r), look.id_chars);
            let name = if open { bold(place, name_tone) } else { txt(place, name_tone) };
            cells.push(name.width(Length::Fixed(id_w)).into());
            cells.push(txt(stage(r), Tone::Dim).width(Length::Fill).into());
        }
    }
    cells.push(hand(app, r.place.as_str(), look.asks.get(r.place.as_str())));
    cells.push(lights(&r.squares, look.squares));
    clickable(format!("room:{}", r.place), band(open, picked, cells), Some(Message::Open(r.place.to_string())))
}

fn child(head: String, topic: Option<&str>, tone: Tone) -> Element<'_, Message> {
    let mut cells: Vec<Element<'_, Message>> = vec![txt(head, tone).into()];
    if let Some(topic) = topic {
        cells.extend([widgets::gap(fonts::ADVANCE), txt(topic, Tone::Dim).into()]);
    }
    // The hand and lights reserve their space before this row is laid out.
    widgets::centred(cells).width(Length::Fill).clip(true).into()
}

fn band<'a>(open: bool, picked: bool, cells: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let style = match (open, picked) {
        (true, _) => theme::room_open,
        (false, true) => theme::room_picked,
        (false, false) => |_: &iced::Theme| container::Style::default(),
    };
    container(widgets::centred(cells)).padding([0.0, ROW_PAD]).style(style).into()
}

// A room's name: under a card's heading its side's, a planner card's `spec` and `build` (fac-633), else its place's.
fn name<'a>(screen: &Screen, r: &'a RoomRow) -> &'a str {
    let spec =
        |card: &str| screen.rooms.iter().any(|s| matches!(&s.kind, PlaceKind::Spec { card: c } if c.as_str() == card));
    match (&r.under, &r.kind) {
        (Some(_), PlaceKind::Spec { .. }) => "spec",
        (Some(_), PlaceKind::Card { card }) if spec(card.as_str()) => "build",
        _ => side(r),
    }
}

fn side(r: &RoomRow) -> &str {
    match &r.under {
        Some(u) if u.review => "review",
        Some(u) => match u.mode {
            Mode::Report => "report",
            Mode::Full | Mode::Light | Mode::Review => "implementation",
        },
        None => r.place.as_str(),
    }
}

// A nested row's words: a child's name; a DM's, were one ever shown.
fn nested(kind: &PlaceKind) -> Option<String> {
    match kind {
        PlaceKind::Child { agent, .. } => Some(agent.as_str().rsplit('/').next().unwrap_or_default().to_owned()),
        PlaceKind::Dm { role, .. } => Some(format!("dm {}", role.as_str())),
        PlaceKind::Main
        | PlaceKind::Card { .. }
        | PlaceKind::Spec { .. }
        | PlaceKind::Challenge { .. }
        | PlaceKind::Other => None,
    }
}

// A card's heading folds everything under it into the heading, or unfolds it; the glyph alone does the same.
fn fold<'a>(app: &'a App, card: &'a str) -> Element<'a, Message> {
    let glyph = if app.session.collapsed.contains(card) { "▸" } else { "▾" };
    let slot = container(txt(glyph, Tone::Dim)).width(Length::Fixed(FOLD_W));
    clickable(format!("fold:{card}"), slot, Some(Message::FoldRoom(card.to_owned())))
}

// The row's asks as one hand, in the colour of the first (the engine's order: red first), blinking
// while the newest was just raised (5.11.2).
fn hand<'a>(app: &App, place: &str, asks: Option<&Vec<&HandView>>) -> Element<'a, Message> {
    let Some(first) = asks.and_then(|a| a.first()) else { return widgets::gap(HAND_W) };
    let seen = asks.into_iter().flatten().filter_map(|h| app.raised.get(&h.ask.to_string()).copied()).max();
    if !hands::blink_on(seen.unwrap_or(i64::MIN), app.now_ms, app.settings.animations) {
        return widgets::gap(HAND_W);
    }
    let marker = icons::icon(Icon::Hand, spans::light(Some(first.color)));
    container(marker).id(format!("roomhand:{place}")).width(Length::Fixed(HAND_W)).into()
}

// The stage's word, with `paused` after it.
fn stage(r: &RoomRow) -> String {
    let paused = r.paused.then_some("paused");
    let words = std::iter::once(r.stage.as_str()).chain(paused);
    words.filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

// Past `slots` a line, the squares wrap 2 px below, so three lines still fit the row's 16 px (5.6.1).
fn lights<'a>(lights: &[AgentLight], slots: usize) -> Element<'a, Message> {
    let line = |agents: &[AgentLight]| {
        let squares = agents.iter().map(|l| icons::light(spans::agent_light(*l)));
        Element::from(iced::widget::Row::with_children(squares).spacing(SQUARE_PITCH - 4.0))
    };
    let lines = Column::with_children(lights.chunks(SQUARES_PER_LINE).map(line))
        .spacing(SQUARE_LINE_GAP)
        .align_x(iced::alignment::Horizontal::Right);
    let w = slots as f32 * SQUARE_PITCH;
    container(lines).width(Length::Fixed(w)).align_x(iced::alignment::Horizontal::Right).into()
}
