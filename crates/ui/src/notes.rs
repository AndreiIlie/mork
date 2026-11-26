//! The open card's notes (fac-94, fac-105): the operator's decisions are among them, the card being its
//! agents' memory. When they overflow, the section scrolls inside itself.

use iced::widget::text::{LineHeight, Span, Wrapping};
use iced::widget::{Column, container, rich_text};
use iced::{Element, Length, Pixels};

use crate::app::{App, Message};
use crate::spans::Tone;
use crate::{fonts, side, theme, window};

/// The notes' width: the side column's inside its padding.
pub const WIDTH: f32 = window::SIDE_INNER;
pub const NOTE_GAP: f32 = 4.0;

/// The open place's card notes, newest first.
pub fn of(app: &App) -> &[String] {
    app.place().map_or(&[], |v| v.notes.as_slice())
}

/// A note's signature and date (`fac-12/foreman 14.10:`), and its words; all words when it has none.
pub fn signed(note: &str) -> (&str, &str) {
    let Some((head, rest)) = note.split_once(": ") else { return ("", note) };
    let dated = head
        .rsplit_once(' ')
        .is_some_and(|(_, d)| d.contains('.') && d.chars().all(|c| c.is_ascii_digit() || c == '.'));
    if dated { (head, rest) } else { ("", note) }
}

/// The right of the section's header: how many notes.
pub fn summary(app: &App) -> Element<'_, Message> {
    let n = of(app).len();
    side::summary(if n == 0 { String::new() } else { n.to_string() })
}

/// The section's body: the notes, or a line saying there are none.
pub fn body(app: &App) -> Element<'_, Message> {
    let notes = of(app);
    if notes.is_empty() {
        return side::empty("no notes yet");
    }
    let struck = mork_types::decisions::withdrawn(notes);
    let list = Column::with_children(notes.iter().enumerate().map(|(i, n)| note(i, notes.len(), n, struck[i])))
        .spacing(NOTE_GAP);
    container(list).id("notes").width(Length::Fill).into()
}

// A note's words wrap under the panel's width, its signature and date bright before them; a
// withdrawn decision is struck through and dim (fac-256).
fn note(i: usize, total: usize, n: &str, struck: bool) -> Element<'_, Message> {
    let clean = crate::text::clean(n);
    let (who, words) = signed(&clean);
    let (bright, text) = if struck { (Tone::Dim, Tone::Dim) } else { (Tone::Bright, Tone::Text) };
    let mut spans: Vec<Span<'_>> = Vec::new();
    let head = match mork_types::decisions::number(total, i) {
        n if who.is_empty() => format!("#{n} "),
        n => format!("#{n} {who}: "),
    };
    spans.push(Span::new(head).font(fonts::MONO_BOLD).color(theme::color(bright)).strikethrough(struck));
    spans.push(Span::new(words.to_owned()).font(fonts::MONO).color(theme::color(text)).strikethrough(struck));
    let text = rich_text(spans)
        .font(fonts::MONO)
        .size(fonts::SIZE)
        .line_height(LineHeight::Absolute(Pixels(fonts::LINE)))
        .wrapping(Wrapping::WordOrGlyph);
    container(text).id(format!("note:{i}")).width(Length::Fill).into()
}
