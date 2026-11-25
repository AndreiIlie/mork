//! Shared markdown layout keeps card titles consistent (fac-33) and wraps long lines and table
//! cells so the conversation remains readable in a narrow window (fac-39).

use iced::widget::text::{LineHeight, Span, Wrapping};
use iced::widget::{Column, Row, column, container, responsive, rich_text, row};
use iced::{Element, Length, Padding, Pixels};

use crate::app::Message;
use crate::markdown::{self, Block, Run};
use crate::spans::Tone;
use crate::text::clean;
use crate::titles::Titles;
use crate::widgets::{self, bold, gap, gap_v, txt};
use crate::{fonts, theme};

/// An empty line's height between blocks.
pub const GAP: f32 = 8.0;
const HEADING_GAP: f32 = 6.0;
const CELL_Y: f32 = 3.0;
// A table cell's padding on each side, in characters.
const CELL_PAD: usize = 1;

/// A text's markdown, its words in `tone`, the cards it names with their titles.
pub fn view<'a>(text: &str, tone: Tone, titles: &Titles) -> Element<'a, Message> {
    let blocks = crate::titles::blocks(markdown::parse(&clean(text)), titles);
    let mut col = Column::new();
    for (i, b) in blocks.into_iter().enumerate() {
        col = col.push(block(b, tone, i == 0));
    }
    col.into()
}

fn block<'a>(b: Block, tone: Tone, first: bool) -> Element<'a, Message> {
    match b {
        Block::Line(runs) => words(runs, tone, false),
        Block::Gap => gap_v(GAP),
        Block::Heading(runs) => {
            let top = if first { 0.0 } else { HEADING_GAP };
            container(words(runs, heading_tone(tone), true)).padding(Padding::ZERO.top(top)).into()
        }
        Block::Item { depth, marker, runs } => {
            let hang = (marker.chars().count() + 1).max(3) as f32 * fonts::ADVANCE;
            let marker = container(txt(marker, Tone::Dim)).width(Length::Fixed(hang));
            row![gap(depth as f32 * 2.0 * fonts::ADVANCE), marker, words(runs, tone, false)].into()
        }
        Block::Code(lines) => {
            let code =
                words(crate::urls::runs(Run { text: lines.join("\n"), ..Run::default() }), code_tone(tone), false);
            let well = container(code).padding([6.0, 10.0]).width(Length::Fill).style(theme::fence);
            container(well).padding([4.0, 0.0]).into()
        }
        Block::Table { head, rows } => table(head, rows, tone),
    }
}

/// Runs as one text that wraps: plain when nothing is marked, so it reads as one string.
pub fn words<'a>(runs: Vec<Run>, tone: Tone, strong: bool) -> Element<'a, Message> {
    if !markdown::marked(&runs) {
        let all: String = runs.into_iter().map(|r| r.text).collect();
        let t = if strong { bold(all, tone) } else { txt(all, tone) };
        return t.wrapping(Wrapping::WordOrGlyph).into();
    }
    let spans: Vec<Span<'a, String>> = runs.into_iter().map(|r| span(r, tone, strong)).collect();
    rich_text(spans)
        .on_link_click(Message::ChatLink)
        .font(fonts::MONO)
        .size(fonts::SIZE)
        .line_height(LineHeight::Absolute(Pixels(fonts::LINE)))
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

fn span<'a>(r: Run, tone: Tone, strong: bool) -> Span<'a, String> {
    let font = if r.bold || strong {
        fonts::MONO_BOLD
    } else if r.italic {
        fonts::MONO_ITALIC
    } else {
        fonts::MONO
    };
    let link = r.link.then(|| r.text.clone());
    let s = Span::new(r.text).font(font).link_maybe(link).underline(r.link);
    let s = if r.code {
        let (fill, edge) = theme::code_highlight();
        s.color(theme::color(code_tone(tone))).background(fill).border(edge).padding([0.0, 1.0])
    } else {
        s.color(theme::color(tone))
    };
    if r.link { s.color(theme::GREEN) } else { s }
}

// Code and headings stand out, but in a dimmed text.
fn code_tone(tone: Tone) -> Tone {
    if tone == Tone::Dim { Tone::Dim } else { Tone::Bright }
}

fn heading_tone(tone: Tone) -> Tone {
    code_tone(tone)
}

// Laid out in the width it is given, as `fit` shares it: every column shows, its cells wrapping.
fn table<'a>(head: Vec<Vec<Run>>, rows: Vec<Vec<Vec<Run>>>, tone: Tone) -> Element<'a, Message> {
    let chars = |c: &[Run]| c.iter().map(|r| r.text.chars().count()).sum::<usize>();
    let natural: Vec<usize> = (0..head.len())
        .map(|i| {
            let widest = rows.iter().filter_map(|r| r.get(i)).map(|c| chars(c)).max().unwrap_or(0);
            widest.max(chars(&head[i])).max(1)
        })
        .collect();
    let laid = responsive(move |size| {
        let widths = fit(&natural, widgets::chars_in(size.width));
        let room = widths.iter().map(|w| w + 2 * CELL_PAD).sum::<usize>() as f32 * fonts::ADVANCE;
        let mut col = column![cells(head.clone(), &widths, heading_tone(tone), true)];
        for r in &rows {
            col = col.push(widgets::hline(theme::table_rule)).push(cells(r.clone(), &widths, tone, false));
        }
        container(col).max_width(room + widths.len() as f32).into()
    })
    .height(Length::Shrink);
    container(laid).padding([4.0, 0.0]).into()
}

/// Column widths within `room` characters, padding apart: natural while all fit; past that,
/// narrow columns keep theirs and the rest share the remainder, at least a character each.
pub fn fit(natural: &[usize], room: usize) -> Vec<usize> {
    let mut left = room.saturating_sub(2 * CELL_PAD * natural.len());
    if natural.iter().sum::<usize>() <= left {
        return natural.to_vec();
    }
    let mut out = natural.to_vec();
    let mut open: Vec<usize> = (0..natural.len()).collect();
    loop {
        let share = left / open.len().max(1);
        let (narrow, wide): (Vec<usize>, Vec<usize>) = open.iter().partition(|i| natural[**i] <= share);
        if narrow.is_empty() || wide.is_empty() {
            let extra = left % open.len().max(1);
            for (k, i) in open.iter().enumerate() {
                out[*i] = natural[*i].min(share + usize::from(k < extra)).max(1);
            }
            return out;
        }
        left -= narrow.iter().map(|i| natural[*i]).sum::<usize>();
        open = wide;
    }
}

fn cells<'a>(cells: Vec<Vec<Run>>, widths: &[usize], tone: Tone, strong: bool) -> Element<'a, Message> {
    let cell = |(c, chars): (Vec<Run>, &usize)| {
        let portion = u16::try_from(chars + 2 * CELL_PAD).unwrap_or(u16::MAX);
        container(words(c, tone, strong))
            .padding([CELL_Y, CELL_PAD as f32 * fonts::ADVANCE])
            .width(Length::FillPortion(portion))
            .into()
    };
    Row::with_children(cells.into_iter().zip(widths).map(cell)).width(Length::Fill).into()
}
