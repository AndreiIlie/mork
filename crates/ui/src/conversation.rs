//! A place's conversation keeps asks visible while details fold (fac-39); shared message
//! layouts keep copy actions and card titles consistent across shapes (fac-33).

use crate::activity::Entry;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::Scrollbar;
use iced::widget::{Column, column, container, responsive, row};
use iced::{Element, Length, Padding};
use mork_types::chat::ChatTurn;
use mork_types::reply::Finding;
use mork_types::view::{MessageView, PlaceView};

use crate::app::{App, Message};
use crate::shape::{self, Shape};
use crate::spans::{Span, Tone};
use crate::text::clean;
use crate::titles::{self, Titles};
use crate::widgets::{self, bold, clickable, gap, gap_v, txt};
use crate::{fonts, prose, theme, window, wrappers};

/// The column's words at their widest: 120 characters.
pub const COLUMN_W: f32 = 120.0 * fonts::ADVANCE;
pub(crate) const COLUMN_PAD: f32 = 12.0;
// The theme mock's spacing: before a block or a bubble, a preview, a divider, and
// a divider after a divider.
const BEFORE_BLOCK: f32 = 18.0;
const BEFORE_PREVIEW: f32 = 12.0;
const BEFORE_DIVIDER: f32 = 14.0;
const DIVIDERS: f32 = 6.0;
const UNDER_HEADER: f32 = 4.0;
const ABOVE_ASK: f32 = 8.0;
const DOT: f32 = 7.0;
const COPY: &str = "copy";
// A divider's characters past its label, time and copy: the gaps between them and the rules'.
const DIVIDER_KEEP: usize = 12;
// A bubble's share of the column, and its padding.
const BUBBLE_SHARE: u16 = 78;
const PANEL_PAD: f32 = 10.0;
const BUBBLE_PAD: Padding = Padding { top: 6.0, right: 12.0, bottom: 8.0, left: 12.0 };
// The scroller's width, and the grab zone either side of it (fac-76): together the column's
// padding, so no words sit under the zone, which takes the cursor from them.
const SCROLLER: f32 = 6.0;
const GRAB: f32 = (COLUMN_PAD - SCROLLER) / 2.0;

/// The conversation, scrolled to its latest message: the earlier messages whose ask is open
/// above the count of those not loaded, which loads more as it comes into view (fac-42).
pub fn view<'a>(app: &'a App, v: &'a PlaceView) -> Element<'a, Message> {
    let t = titles::of(&app.screen);
    let mut col = messages(app, &t, &v.pinned, &[], Column::new());
    if v.earlier > 0 {
        let top = if v.pinned.is_empty() { 0.0 } else { BEFORE_BLOCK };
        col = col.push(container(earlier(v)).padding(Padding::ZERO.top(top)));
    }
    col = messages(app, &t, &v.messages, &v.turns, col);
    let pad = Padding { top: 0.0, right: COLUMN_PAD, bottom: 8.0, left: COLUMN_PAD };
    let centred = container(container(col).max_width(COLUMN_W + 2.0 * COLUMN_PAD).padding(pad)).center_x(Length::Fill);
    let bar = Scrollbar::new().width(SCROLLER).scroller_width(SCROLLER).margin(GRAB);
    let list = widgets::scroll_with(centred, bar).id(window::CONVERSATION).anchor_bottom().on_scroll(|vp| {
        let share = vp.relative_offset().y;
        Message::Scrolled(if share.is_finite() { share } else { 0.0 })
    });
    wrappers::scrolled(list, window::CONVERSATION, app.scroll)
}

fn messages<'a>(
    app: &'a App,
    t: &Titles,
    list: &'a [MessageView],
    turns: &'a [ChatTurn],
    mut col: Column<'a, Message>,
) -> Column<'a, Message> {
    let mut before: Option<Shape> = None;
    for entry in crate::activity::entries(list, turns) {
        let m = match entry {
            Entry::Message(m) => m,
            Entry::Turn(turn) => {
                col = col.push(
                    container(crate::activity::view(app, turn, list, t)).padding(Padding::ZERO.top(UNDER_HEADER)),
                );
                before = Some(Shape::Preview);
                continue;
            }
        };
        let s = shape::shape(m);
        let top = match (before, s) {
            (None, _) => 0.0,
            (Some(Shape::Divider), Shape::Divider) => DIVIDERS,
            (_, Shape::Divider) => BEFORE_DIVIDER,
            (_, Shape::Preview) => BEFORE_PREVIEW,
            (_, Shape::Block | Shape::Bubble) => BEFORE_BLOCK,
        };
        col = col.push(container(message(app, t, m, s)).padding(Padding::ZERO.top(top)));
        before = Some(s);
    }
    col
}

// Shown, it asks for the messages before the oldest one: again each time that one changes.
fn earlier<'a>(v: &PlaceView) -> Element<'a, Message> {
    let line = txt(format!("{} earlier messages", v.earlier), Tone::Dim);
    match v.messages.first().map(|m| m.seq) {
        Some(first) => iced::widget::sensor(line).key(first).on_show(move |_| Message::Older(first)).into(),
        None => line.into(),
    }
}

pub(super) fn message<'a>(app: &'a App, t: &Titles, m: &'a MessageView, s: Shape) -> Element<'a, Message> {
    let id = format!("msg:{}", m.seq);
    let offset = app.screen.tz_offset_min;
    match s {
        Shape::Divider => noted(app, t, m, id, offset),
        Shape::Bubble => bubble(t, m, id, offset),
        Shape::Block => block(app, t, m, id, offset),
        Shape::Preview => folded(app, t, m, id, offset),
    }
}

// A divider with a note folds it under its line (fac-384); copy keeps its own press.
fn noted<'a>(app: &'a App, t: &Titles, m: &'a MessageView, id: String, offset: i32) -> Element<'a, Message> {
    if !shape::foldable(m) {
        return clickable(id, divider(t, m, offset, ""), None);
    }
    let unfolded = app.unfolded(m.seq);
    let line = divider(t, m, offset, if unfolded { "▾ " } else { "▸ " });
    let mut col = column![line];
    if unfolded {
        let body = column![prose::view(&m.text, Tone::Text, t), details(m)];
        col = col.push(container(body).padding(Padding::ZERO.top(UNDER_HEADER)));
    }
    clickable(id, col, Some(Message::Fold(m.seq)))
}

// Expanded card titles can otherwise push the time and copy control outside a narrow column.
fn divider<'a>(t: &Titles, m: &MessageView, offset: i32, glyph: &str) -> Element<'a, Message> {
    let (label, at) = shape::divider(m, offset);
    let label = format!("{glyph}{}", titles::named(&label, t));
    let seq = m.seq;
    let line = responsive(move |size| {
        let room = widgets::chars_in(size.width).saturating_sub(at.chars().count() + COPY.len() + DIVIDER_KEEP);
        let words = row![txt(widgets::cut(&label, room), Tone::Dim), txt(at.clone(), Tone::Dim), copy(seq)]
            .spacing(fonts::ADVANCE * 2.0);
        let line = row![widgets::hline(theme::seam), words, widgets::hline(theme::seam)]
            .spacing(12.0)
            .height(Length::Fixed(fonts::LINE))
            .align_y(Vertical::Center);
        container(line).clip(true).into()
    });
    line.height(Length::Fixed(fonts::LINE)).into()
}

// As wide as its words, up to its share of the column: its header pushed to its right edge.
fn bubble<'a>(t: &Titles, m: &'a MessageView, id: String, offset: i32) -> Element<'a, Message> {
    let meta = shape::bubble_header(m, offset);
    let meta_chars: usize = meta.iter().map(|s| s.text.chars().count() + 1).sum::<usize>() + COPY.len() + 1;
    let text = clean(&m.text);
    let widest = titles::named(&text, t).lines().map(|l| l.chars().count()).max().unwrap_or(0).max(meta_chars);
    let width = if m.images.is_empty() { widest as f32 * fonts::ADVANCE + 1.0 } else { f32::MAX };
    let mut cells: Vec<Element<'a, Message>> = vec![copy(m.seq), iced::widget::space().width(Length::Fill).into()];
    cells.extend(meta.iter().map(|s| widgets::span(s).into()));
    let mut col = column![row(cells).spacing(fonts::ADVANCE), gap_v(2.0), prose::view(&text, Tone::Bright, t)];
    if let Some(thumbs) = images(m) {
        col = col.push(container(thumbs).padding(Padding::ZERO.top(UNDER_HEADER)));
    }
    let b = container(col.width(Length::Fixed(width))).padding(BUBBLE_PAD).style(theme::bubble);
    let side = container(clickable(id, b, None)).width(Length::FillPortion(BUBBLE_SHARE)).align_x(Horizontal::Right);
    row![iced::widget::space().width(Length::FillPortion(100 - BUBBLE_SHARE)), side].into()
}

fn header<'a>(m: &MessageView, offset: i32) -> Element<'a, Message> {
    let mut cells: Vec<Element<'a, Message>> = Vec::new();
    if let Some(tone) = shape::dot(m) {
        let dot = container(gap_v(DOT)).width(Length::Fixed(DOT)).style(theme::dot(tone));
        cells.push(dot.into());
    }
    let spans: Vec<Span> = shape::header(m, offset);
    let last = spans.len().saturating_sub(1);
    for (i, s) in spans.iter().enumerate() {
        if i == last && i > 0 {
            cells.push(gap(fonts::ADVANCE));
        }
        cells.push(widgets::span(s).into());
    }
    cells.extend([iced::widget::space().width(Length::Fill).into(), copy(m.seq)]);
    row(cells).spacing(fonts::ADVANCE).height(Length::Fixed(fonts::LINE)).align_y(Vertical::Center).into()
}

// Its own press, which the message's fold under it doesn't take.
fn copy<'a>(seq: u64) -> Element<'a, Message> {
    widgets::words_button(format!("copy:{seq}"), COPY.into(), Tone::Dim, Message::Copy(seq))
}

fn block<'a>(app: &'a App, t: &Titles, m: &'a MessageView, id: String, offset: i32) -> Element<'a, Message> {
    let unfolded = app.unfolded(m.seq);
    let mut col = column![header(m, offset), gap_v(UNDER_HEADER), prose::view(&shape::words(m), Tone::Text, t)];
    if let Some(thumbs) = images(m) {
        col = col.push(container(thumbs).padding(Padding::ZERO.top(UNDER_HEADER)));
    }
    if let Some(ask) = ask(m, t) {
        col = col.push(container(ask).padding(Padding::ZERO.top(ABOVE_ASK)));
    }
    if shape::folds(m) {
        let more: Element<'a, Message> = if unfolded { details(m) } else { txt("more …", Tone::Dim).into() };
        col = col.push(container(more).padding(Padding::ZERO.top(UNDER_HEADER)));
    }
    let press = shape::foldable(m).then_some(Message::Fold(m.seq));
    let block = with_findings(clickable(id, col, press), m, unfolded, t);
    if !shape::for_operator(m) {
        return block;
    }
    container(block).width(Length::Fill).padding(PANEL_PAD).style(theme::panel).into()
}

// Its header and one dim line, cut to the column; unfolded, its words whole.
fn folded<'a>(app: &'a App, t: &Titles, m: &'a MessageView, id: String, offset: i32) -> Element<'a, Message> {
    let unfolded = app.unfolded(m.seq);
    let body: Element<'a, Message> = if unfolded {
        let mut words = column![prose::view(&m.text, Tone::Text, t)];
        if let Some(thumbs) = images(m) {
            words = words.push(container(thumbs).padding(Padding::ZERO.top(UNDER_HEADER)));
        }
        words.push(details(m)).into()
    } else {
        let (line, more) = shape::preview(m);
        let line = titles::named(&line, t);
        responsive(move |size| {
            let n = widgets::chars_in(size.width);
            let shown =
                if more { format!("{} …", widgets::cut(&line, n.saturating_sub(2))) } else { widgets::cut(&line, n) };
            txt(shown, Tone::Dim).into()
        })
        .height(Length::Fixed(fonts::LINE))
        .into()
    };
    let col = column![header(m, offset), gap_v(1.0), body];
    with_findings(clickable(id, col, Some(Message::Fold(m.seq))), m, unfolded, t)
}

fn images<'a>(m: &'a MessageView) -> Option<Element<'a, Message>> {
    if m.images.is_empty() {
        return None;
    }
    let thumbs = m.images.iter().enumerate().map(|(i, p)| {
        clickable(format!("image:{}:{i}", m.seq), widgets::thumbnail(&p.path), Some(Message::Image(p.path.clone())))
    });
    Some(row(thumbs).spacing(fonts::ADVANCE * 2.0).wrap().into())
}

// Open, framed in orange and bright; answered or replaced, dim.
fn ask<'a>(m: &'a MessageView, t: &Titles) -> Option<Element<'a, Message>> {
    let words = m.hand.as_ref().map(|h| h.ask_words()).filter(|a| !a.trim().is_empty())?;
    if m.ask_open {
        let inner = row![bold("ask", Tone::Orange), gap(fonts::ADVANCE), prose::view(words.trim(), Tone::Bright, t)];
        return Some(container(inner).padding([4.0, 10.0]).style(theme::ask_open).into());
    }
    Some(row![bold("ask", Tone::Dim), gap(fonts::ADVANCE), prose::view(words.trim(), Tone::Dim, t)].into())
}

// The cut notice keeps the view's bounds visible.
fn details(m: &MessageView) -> Element<'_, Message> {
    let mut col = Column::new().spacing(2.0);
    if m.cut {
        col = col.push(txt("(cut: the rest is in the card's record)", Tone::Dim));
    }
    col.into()
}

fn labelled<'a>(label: &'a str, words: &str, t: &Titles) -> Element<'a, Message> {
    row![txt(label, Tone::Dim), gap(fonts::ADVANCE), prose::view(words, Tone::Text, t)].into()
}

fn with_findings<'a>(
    top: Element<'a, Message>,
    m: &'a MessageView,
    unfolded: bool,
    t: &Titles,
) -> Element<'a, Message> {
    if m.findings.is_empty() {
        return top;
    }
    let mut col = column![top];
    for (i, f) in m.findings.iter().enumerate() {
        col = col.push(finding(m.seq, i, f, unfolded, t));
    }
    col.into()
}

fn finding<'a>(seq: u64, i: usize, f: &'a Finding, unfolded: bool, t: &Titles) -> Element<'a, Message> {
    let words = format!("▸ {}:{} {}", clean(&f.file), f.line, titles::named(&clean(&f.summary), t));
    let press = Message::Finding { file: f.file.clone(), line: f.line };
    let line = clickable(format!("finding:{seq}:{i}"), widgets::wrapped(words, Tone::Text), Some(press));
    let mut col = column![container(line).padding(Padding::ZERO.top(UNDER_HEADER))];
    if unfolded && !f.proof.trim().is_empty() {
        let indent = Padding::ZERO.left(2.0 * fonts::ADVANCE);
        col = col.push(container(labelled("proof", &clean(&f.proof), t)).padding(indent));
    }
    col.into()
}
