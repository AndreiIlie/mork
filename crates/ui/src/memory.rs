//! Main's Memory tab (fac-159): each memory's counts, then its files, as `memory.stats` has them.
//! Drawn from the screen alone: seeing a file here reads nothing.

use factory_types::memory::{FileRow, MemoryView};
use factory_types::orchestrator::ReviewSection;
use iced::widget::{Column, column, container, responsive};
use iced::{Element, Length};

use crate::app::{App, Message, ScrollOp};
use crate::board::GAP;
use crate::spans::Tone;
use crate::widgets::{self, bold, txt};
use crate::{theme, time, wrappers};

/// The tab's scrollable's id.
pub const LIST: &str = "memory";
const LIST_GAP: f32 = 6.0;
// Room kept for the scrollbar, so it never covers the counts.
const BAR: f32 = 8.0;
const COUNTS: [&str; 3] = ["written", "read", "deleted"];
const READS: usize = 5;
const LAST_READ: usize = 12;
const INDENT: usize = 2;

pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let width = (size.width - BAR).max(0.0);
        let chars = widgets::chars_in(width);
        let mut col = Column::new().spacing(GAP).push(totals(&app.screen.memories));
        for m in &app.screen.memories {
            col = col.push(files(app, m, chars));
        }
        let list = widgets::scroll(container(col).width(Length::Fixed(width))).id(LIST);
        wrappers::scrolled(list, LIST, app.scroll.map(|(n, op)| (n, step(op))))
    })
    .into()
}

// ↑ and ↓ alone scroll the tab, the other way than the conversation, which is anchored at its
// bottom; a share or a band asked is the conversation's or the rooms', so nothing here.
fn step(op: ScrollOp) -> ScrollOp {
    match op {
        ScrollOp::By(y) => ScrollOp::By(-y),
        ScrollOp::To(_) | ScrollOp::Show { .. } => ScrollOp::By(0.0),
    }
}

fn totals(memories: &[MemoryView]) -> Element<'_, Message> {
    let wide = memories.iter().map(|m| m.tally.memory.chars().count()).max().unwrap_or_default().max(6);
    let counts = |m: &MemoryView| [m.tally.written, m.tally.read, m.tally.deleted];
    let mut widths = COUNTS.map(str::len);
    for m in memories {
        for (width, count) in widths.iter_mut().zip(counts(m)) {
            *width = (*width).max(count.to_string().len());
        }
    }
    let head = COUNTS.iter().zip(widths).fold(format!("{:<wide$}", "memory"), |s, (c, w)| format!("{s}  {c:<w$}"))
        + "  reviewed";
    let mut col = column![bold("Memories", Tone::Bright), txt(head, Tone::Dim)];
    for m in memories {
        let line = widths
            .into_iter()
            .zip(counts(m))
            .fold(format!("{:<wide$}", m.tally.memory), |s, (w, n)| format!("{s}  {n:>w$}"));
        let reviewed = m.reviewed.as_deref().unwrap_or("never");
        col = col.push(txt(format!("{line}  {reviewed}"), Tone::Text));
    }
    col.into()
}

fn files<'a>(app: &App, m: &'a MemoryView, chars: usize) -> Element<'a, Message> {
    let n = m.files.len();
    let count = match n {
        0 => "  no files".to_owned(),
        1 => "  1 file".to_owned(),
        n => format!("  {n} files"),
    };
    let memory = m.tally.memory.clone();
    let head = widgets::sectioned(
        vec![bold(memory.clone(), Tone::Bright).into(), txt(count, Tone::Dim).into()],
        format!("review:memories:{memory}"),
        Message::Review(ReviewSection::Memories, memory),
    );
    let mut col = column![widgets::hline(theme::seam), head].spacing(LIST_GAP);
    if n == 0 {
        return col.into();
    }
    let name_w = m.files.iter().map(|f| f.name.chars().count()).max().unwrap_or_default().max(4);
    let pad = " ".repeat(INDENT);
    let heading = format!("{pad}{:<name_w$}  {:>READS$}  {:<LAST_READ$}  description", "name", "reads", "last read");
    let mut rows = column![txt(heading, Tone::Dim)];
    for f in &m.files {
        rows = rows.push(file(app, f, name_w, chars));
    }
    col = col.push(rows);
    col.into()
}

fn file<'a>(app: &App, f: &'a FileRow, name_w: usize, chars: usize) -> Element<'a, Message> {
    let pad = " ".repeat(INDENT);
    let last =
        f.last_read_ms.map_or_else(|| "never".to_owned(), |at| format!("{} ago", time::duration(app.now_ms - at)));
    let left = format!("{pad}{:<name_w$}  {:>READS$}  {last:<LAST_READ$}  ", f.name, f.reads);
    let description = widgets::cut(&f.description, chars.saturating_sub(left.chars().count()));
    widgets::centred(vec![txt(left, Tone::Text).into(), txt(description, Tone::Dim).into()]).into()
}
