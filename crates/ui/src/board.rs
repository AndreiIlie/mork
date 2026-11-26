//! Main's Board tab (5.8.1, fac-37): the engine sorts the rows; the window only draws them
//! and folds the areas.

use std::collections::BTreeSet;

use iced::widget::{Column, column, container, responsive, row, tooltip};
use iced::{Element, Length};
use mork_types::CardId;
use mork_types::view::{BoardRow, BoardView, HandView, PlaceKind, Screen};

use crate::app::{App, Message};
use crate::icons::{self, Icon};
use crate::spans::{self, Tone};
use crate::widgets::{self, bold, clickable, txt};
use crate::{areas, fonts, hands, theme};

/// The gap between the three columns, and between the Board's parts.
pub const GAP: f32 = 14.0;
/// A tile's padding inside its edge, top and bottom, and left and right.
pub const TILE_Y: f32 = 6.0;
pub const TILE_X: f32 = 10.0;
const TILE_GAP: f32 = 8.0;
// The widest a whole title shows under the pointer before it wraps, in characters.
const WHOLE_CHARS: f32 = 80.0;
// The scrollbar's room on the right.
const BAR: f32 = 8.0;

/// Every card row drawn, in the order drawn, by its widget's id: what ↑, ↓ and Enter walk. An
/// area's rows only while it is open.
pub fn rows<'a>(screen: &'a Screen, open: &BTreeSet<String>) -> Vec<(String, &'a CardId)> {
    let on_board = |r: &'a BoardRow| (format!("board:{}", r.id), opens(r));
    let in_queue = |r: &'a BoardRow| (format!("merge:{}", r.id), &r.id);
    let mut out: Vec<(String, &CardId)> = screen.boards.iter().flat_map(|b| &b.live.cards).map(on_board).collect();
    for b in &screen.boards {
        out.extend(b.merge.slot.iter().map(|(r, _)| in_queue(r)));
        out.extend(b.merge.waiting.iter().map(|(r, _)| in_queue(r)));
        for g in b.areas.iter().chain([&b.done]) {
            if open.contains(&areas::key(&b.project, &g.name)) {
                out.extend(g.cards.iter().map(on_board));
            }
        }
    }
    out
}

/// The room a row's click opens: its card's, or the run's carrying it (fac-563).
pub fn opens(r: &BoardRow) -> &CardId {
    r.room.as_ref().unwrap_or(&r.id)
}

/// The room a run's click opens: a planner card's spec room while its plan is agreed, else its build room (fac-633).
pub fn room_of(screen: &Screen, card: &str) -> String {
    let spec = screen.rooms.iter().find(|r| matches!(&r.kind, PlaceKind::Spec { card: c } if c.as_str() == card));
    match spec {
        Some(s) if s.stage == "Spec" || s.stage.starts_with("Spec ") => s.place.to_string(),
        Some(_) | None => card.to_owned(),
    }
}

/// Whether Mork ran a card the Board shows: a click on its row opens its room.
pub fn ran(screen: &Screen, id: &str) -> bool {
    screen.boards.iter().flat_map(every_row).any(|r| r.id == id && r.ran)
}

pub(crate) fn every_row(b: &BoardView) -> impl Iterator<Item = &BoardRow> {
    let queue = b.merge.slot.iter().map(|(r, _)| r).chain(b.merge.waiting.iter().map(|(r, _)| r));
    let groups = b.areas.iter().chain([&b.done]).flat_map(|g| &g.cards);
    b.live.cards.iter().chain(queue).chain(groups)
}

pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let width = (size.width - BAR).max(0.0);
        let picked = app.board_row.and_then(|i| rows(&app.screen, &app.board_open).get(i).map(|r| r.0.clone()));
        let picked = picked.as_deref();
        let mut col = Column::new().spacing(GAP).push(widgets::gap_v(0.0)).push(top(app, width, picked));
        for b in &app.screen.boards {
            col = col.push(areas::board(app, b, width, picked));
        }
        widgets::scroll(container(col).width(Length::Fixed(width))).into()
    })
    .into()
}

/// One of three side by side, each `width` wide.
pub fn third(width: f32) -> f32 {
    ((width - 2.0 * GAP) / 3.0).floor().max(0.0)
}

/// The characters that fit in a tile `width` wide.
pub fn tile_chars(width: f32) -> usize {
    widgets::chars_in(width - 2.0 * (TILE_X + theme::BORDER))
}

fn top<'a>(app: &'a App, width: f32, picked: Option<&str>) -> Element<'a, Message> {
    let (w, boards) = (third(width), &app.screen.boards);
    let chars = tile_chars(w);
    let live: Vec<Element<'a, Message>> =
        boards.iter().flat_map(|b| &b.live.cards).map(|r| live_tile(r, chars, picked)).collect();
    let waits: Vec<&HandView> = app.screen.hands.iter().collect();
    let waits_tone = if waits.is_empty() { Tone::Dim } else { Tone::Orange };
    let waits: Vec<Element<'a, Message>> = waits.into_iter().map(|h| wait_tile(h, chars)).collect();
    let live_n = boards.iter().map(|b| b.live.count).sum();
    let waits_n = waits.len();
    row![
        tiles(("Live", live_n), live, Tone::Dim, "nothing live", w),
        tiles(("Waits on you", waits_n), waits, waits_tone, "nothing waits on you", w),
    ]
    .spacing(GAP)
    .into()
}

// A column: its name and how many it has, its tiles, and a word for the rows the line's bound cut.
fn tiles<'a>(
    (name, count): (&'a str, usize),
    tiles: Vec<Element<'a, Message>>,
    tone: Tone,
    none: &'a str,
    w: f32,
) -> Element<'a, Message> {
    let head = widgets::centred(vec![
        txt(name, Tone::Bright).into(),
        widgets::gap(fonts::ADVANCE),
        txt(count.to_string(), tone).into(),
    ]);
    let mut col = column![head].spacing(TILE_GAP).width(Length::Fixed(w));
    if tiles.is_empty() && count == 0 {
        col = col.push(txt(none, Tone::Dim));
    }
    let cut = count.saturating_sub(tiles.len());
    col = col.extend(tiles);
    if cut > 0 {
        col = col.push(txt(format!("{cut} more not sent"), Tone::Dim));
    }
    col.into()
}

fn live_tile<'a>(r: &'a BoardRow, chars: usize, picked: Option<&str>) -> Element<'a, Message> {
    let id = format!("board:{}", r.id);
    let mut lines = vec![head(r, chars)];
    if let Some(l) = &r.live {
        let mut marks: Vec<Element<'a, Message>> = vec![txt(l.stage.clone(), Tone::Green).into()];
        let working: Vec<&str> = l.working.iter().map(|r| r.as_str()).collect();
        if !working.is_empty() {
            marks.extend([widgets::gap(fonts::ADVANCE * 2.0), txt(working.join(", "), Tone::Green).into()]);
        }
        lines.push(widgets::centred(marks).into());
    }
    let tone = if picked == Some(id.as_str()) { Tone::White } else { Tone::Edge };
    tile(id, lines, tone, press(r))
}

// An ask waiting on the operator, as its hand says it, its colour on the hand by its head, as the
// tile's edge is every card's (fac-103): a click opens its place.
fn wait_tile(h: &HandView, chars: usize) -> Element<'_, Message> {
    let who = h.card.as_ref().map_or_else(|| h.place.to_string(), ToString::to_string);
    let role = h.role.map(|r| format!("  {}", r.as_str())).unwrap_or_default();
    let hand = icons::icon(Icon::Hand, spans::light(Some(h.color)));
    let head = vec![hand, widgets::gap(fonts::ADVANCE), bold(who, Tone::Bright).into(), txt(role, Tone::Dim).into()];
    let mut lines: Vec<Element<'_, Message>> = vec![widgets::centred(head).into()];
    for l in hands::words(&crate::text::clean(&h.words), chars) {
        lines.push(txt(l, Tone::Text).into());
    }
    let open = Message::OpenAsk { place: h.place.to_string(), ask: h.ask.to_string() };
    tile(format!("wait:{}", h.ask), lines, Tone::Edge, Some(open))
}

/// A card's tile: its lines on a card edged in a tone, under its widget's id.
pub fn tile<'a>(
    id: String,
    lines: Vec<Element<'a, Message>>,
    tone: Tone,
    press: Option<Message>,
) -> Element<'a, Message> {
    let card =
        container(Column::with_children(lines)).padding([TILE_Y, TILE_X]).width(Length::Fill).style(theme::card(tone));
    clickable(id, card, press)
}

/// A card's first line in `chars` characters: a live card's light, its id, its title.
pub fn head(r: &BoardRow, chars: usize) -> Element<'_, Message> {
    let mut cells: Vec<Element<'_, Message>> = Vec::new();
    let mut used = r.id.as_str().chars().count() + 2;
    if let Some(l) = &r.live {
        cells.extend([icons::light(spans::light(l.light)), widgets::gap(fonts::ADVANCE)]);
        used += 2;
    }
    cells.push(bold(r.id.to_string(), Tone::Bright).into());
    cells.push(title(r, chars.saturating_sub(used)));
    widgets::centred(cells).into()
}

/// A card's title after two spaces, cut to `chars` characters; cut, the pointer over it shows it
/// whole beneath (fac-84).
pub fn title(r: &BoardRow, chars: usize) -> Element<'_, Message> {
    let whole = crate::text::clean(&r.title);
    let shown = txt(format!("  {}", widgets::cut(&whole, chars)), Tone::Text);
    if whole.chars().count() <= chars {
        return shown.into();
    }
    let tip = container(widgets::wrapped(whole, Tone::Text)).max_width(WHOLE_CHARS * fonts::ADVANCE);
    tooltip(shown, tip, tooltip::Position::Bottom).gap(2.0).padding(TILE_Y).style(theme::tooltip).into()
}

/// What a click on a card's row does: a card Mork ran opens its room; any other, nothing.
pub fn press(r: &BoardRow) -> Option<Message> {
    r.ran.then(|| Message::BoardRow(opens(r).to_string()))
}
