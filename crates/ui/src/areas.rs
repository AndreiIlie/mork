//! A board's part of main's Board tab (fac-37); each group stays folded until clicked.

use iced::widget::{Column, Row, column, container, row};
use iced::{Element, Length};
use mork_types::ProjectId;
use mork_types::view::{BoardGroup, BoardRow, BoardView, MergeQueue};

use crate::app::{App, Message};
use crate::board::{self, GAP, TILE_X, TILE_Y};
use crate::spans::Tone;
use crate::widgets::{self, bold, clickable, small, txt};
use crate::{fonts, theme, time};

const LIST_GAP: f32 = 6.0;
// A group's row's inset.
const ROW_X: f32 = 4.0;

/// A group's key among the window's open ones: its board's name and its own.
pub fn key(project: &ProjectId, name: &str) -> String {
    format!("{project}/{name}")
}

/// A board's part: its name and counts, its merge queue while a card is in it, its groups.
pub fn board<'a>(app: &'a App, b: &'a BoardView, width: f32, picked: Option<&str>) -> Element<'a, Message> {
    let backlog: usize = b.areas.iter().map(|g| g.count).sum();
    let queue = merge_queue(app, &b.merge, width, picked);
    let counts = match (backlog + b.done.count, &queue) {
        (0, None) => "  no cards on its board".to_owned(),
        _ => format!("  {backlog} in the backlog, {} {}", b.done.count, b.done.name.to_lowercase()),
    };
    let head = widgets::centred(vec![bold(b.project.to_string(), Tone::Bright).into(), txt(counts, Tone::Dim).into()]);
    let mut col = column![widgets::hline(theme::seam), head].spacing(LIST_GAP);
    if let Some(q) = queue {
        col = col.push(container(q).padding(iced::Padding { bottom: LIST_GAP, ..iced::Padding::ZERO }));
    }
    if backlog + b.done.count > 0 {
        col = col.push(groups(app, b, width, picked));
    }
    col.into()
}

fn merge_queue<'a>(app: &'a App, q: &'a MergeQueue, width: f32, picked: Option<&str>) -> Option<Element<'a, Message>> {
    if q.slot.is_none() && q.in_line == 0 {
        return None;
    }
    let base_w = q.base.chars().count() as f32 * fonts::ADVANCE + 2.0 * (TILE_X + theme::BORDER);
    let arrow_w = 3.0 * fonts::ADVANCE;
    let slot_w = (width - base_w - arrow_w).max(0.0);
    let slot: Element<'a, Message> = match &q.slot {
        Some((r, step)) => {
            let id = format!("merge:{}", r.id);
            let tone = if picked == Some(id.as_str()) { Tone::White } else { Tone::Edge };
            let lines = vec![
                small("in the slot", Tone::Dim).into(),
                board::head(r, board::tile_chars(slot_w)),
                txt(step.clone(), Tone::Green).into(),
            ];
            board::tile(id, lines, tone, board::press(r))
        }
        None => container(txt("nobody in the slot", Tone::Dim)).padding([TILE_Y, TILE_X]).into(),
    };
    let base = container(bold(q.base.clone(), Tone::Bright))
        .padding([TILE_Y, TILE_X])
        .width(Length::Fixed(base_w))
        .style(theme::card(Tone::White));
    let arrow = container(txt("›", Tone::White)).width(Length::Fixed(arrow_w)).center_x(Length::Fixed(arrow_w));
    let lane =
        row![container(slot).width(Length::Fixed(slot_w)), arrow, base].align_y(iced::alignment::Vertical::Center);
    let head = widgets::centred(vec![
        txt("merge queue", Tone::Bright).into(),
        txt("  one card at a time, first come first served", Tone::Dim).into(),
    ]);
    let mut col = column![head, lane].spacing(LIST_GAP);
    if q.in_line > 0 {
        let n = q.in_line.to_string();
        col = col.push(widgets::centred(vec![
            txt("in line", Tone::Bright).into(),
            widgets::gap(fonts::ADVANCE),
            txt(n, Tone::Dim).into(),
        ]));
        let lines = q.waiting.iter().enumerate().map(|(i, (r, at))| in_line(app, i + 1, r, *at, width, picked));
        col = col.push(Column::with_children(lines));
        if q.in_line > q.waiting.len() {
            col = col.push(txt(format!("{} more not sent", q.in_line - q.waiting.len()), Tone::Dim));
        }
    }
    Some(col.into())
}

// A card waiting its turn: its place in line, its id and title, how long it has waited.
fn in_line<'a>(
    app: &App,
    n: usize,
    r: &'a BoardRow,
    at: i64,
    width: f32,
    picked: Option<&str>,
) -> Element<'a, Message> {
    let id = format!("merge:{}", r.id);
    let waited = format!("waiting {}", time::duration(app.now_ms - at));
    let used = 4 + r.id.as_str().chars().count() + 2 + waited.chars().count() + 2;
    let title = board::title(r, widgets::chars_in(width).saturating_sub(used));
    let cells = vec![txt(format!("{n:>2}  "), Tone::Dim).into(), id_text(r, picked == Some(id.as_str())), title];
    clickable(id, filled(Row::with_children(cells), txt(waited, Tone::Dim)), board::press(r))
}

fn groups<'a>(app: &'a App, b: &'a BoardView, width: f32, picked: Option<&str>) -> Element<'a, Message> {
    let w = board::third(width);
    let all: Vec<&BoardGroup> = b.areas.iter().chain([&b.done]).filter(|g| g.count > 0).collect();
    let mut cols: [Vec<Element<'a, Message>>; 3] = Default::default();
    for (i, g) in all.into_iter().enumerate() {
        cols[i % 3].push(group(app, &b.project, g, w, picked));
    }
    let cols = cols.map(|c| Column::with_children(c).spacing(LIST_GAP).width(Length::Fixed(w)).into());
    Row::with_children(cols).spacing(GAP).into()
}

// A group: its heading, a click on which opens or folds it, and its cards while open.
fn group<'a>(app: &App, project: &ProjectId, g: &'a BoardGroup, w: f32, picked: Option<&str>) -> Element<'a, Message> {
    let key = key(project, &g.name);
    let open = app.board_open.contains(&key);
    let heading = widgets::centred(vec![
        txt(if open { "▾" } else { "▸" }, Tone::Dim).into(),
        widgets::gap(fonts::ADVANCE),
        txt(g.name.clone(), Tone::Bright).into(),
        widgets::gap(fonts::ADVANCE),
        txt(g.count.to_string(), Tone::Dim).into(),
    ]);
    let heading = clickable(format!("area:{key}"), heading.width(Length::Fill), Some(Message::BoardArea(key.clone())));
    let mut col = column![heading];
    if open {
        let chars = board::tile_chars(w);
        col = col.extend(g.cards.iter().map(|r| line(r, chars, picked)));
        if g.count > g.cards.len() {
            col = col.push(txt(format!("{} more not sent", g.count - g.cards.len()), Tone::Dim));
        }
    }
    let tone = if open { Tone::Edge } else { Tone::Rule };
    container(col).padding([TILE_Y, TILE_X]).width(Length::Fill).style(theme::card(tone)).into()
}

// A card in a group: its id, its title, and on the right its status when its group doesn't say it.
fn line<'a>(r: &'a BoardRow, chars: usize, picked: Option<&str>) -> Element<'a, Message> {
    let id = format!("board:{}", r.id);
    let status = r.status.clone().unwrap_or_default();
    let used = r.id.as_str().chars().count() + 3 + if status.is_empty() { 0 } else { status.chars().count() + 2 };
    let right = Row::new().push(txt(status, Tone::Dim)).align_y(iced::alignment::Vertical::Center);
    let title = board::title(r, chars.saturating_sub(used));
    let cells = Row::new().push(id_text(r, picked == Some(id.as_str()))).push(title);
    let row = container(filled(cells, right)).padding([0.0, ROW_X]);
    clickable(id, row, board::press(r))
}

fn id_text(r: &BoardRow, picked: bool) -> Element<'_, Message> {
    if picked { bold(r.id.to_string(), Tone::Bright).into() } else { txt(r.id.to_string(), Tone::Text).into() }
}

// One 16 px line across its width: `left`, then `right` against its right edge.
fn filled<'a>(left: Row<'a, Message>, right: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    left.push(iced::widget::space().width(Length::Fill))
        .push(right)
        .width(Length::Fill)
        .height(Length::Fixed(fonts::LINE))
        .align_y(iced::alignment::Vertical::Center)
        .into()
}
