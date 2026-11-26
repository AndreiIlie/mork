//! Main's Notes tab (fac-287): each project's notes to its orchestrator, newest first, each under
//! its kind, seat and card, one send-to-orchestrator button per project. Drawn from the screen alone.

use iced::widget::{Column, column, container, responsive};
use iced::{Element, Length};
use mork_types::note::{Note, NoteWhat, ProjectNotes};
use mork_types::orchestrator::ReviewSection;

use crate::app::{App, Message, ScrollOp};
use crate::board::GAP;
use crate::spans::Tone;
use crate::widgets::{self, bold, txt};
use crate::{fonts, prose, theme, time, titles, wrappers};

/// The tab's scrollable's id.
pub const LIST: &str = "notes";
const LIST_GAP: f32 = 6.0;
// Room kept for the scrollbar, so it never covers a note.
const BAR: f32 = 8.0;
const INDENT: f32 = 2.0;

pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let width = (size.width - BAR).max(0.0);
        let mut col = Column::new().spacing(GAP).push(bold("Notes", Tone::Bright));
        if app.screen.notes.is_empty() {
            col = col.push(txt("none filed", Tone::Dim));
        }
        let t = titles::of(&app.screen);
        for p in &app.screen.notes {
            col = col.push(project(app, p, &t));
        }
        let list = widgets::scroll(container(col).width(Length::Fixed(width))).id(LIST);
        wrappers::scrolled(list, LIST, app.scroll.map(|(n, op)| (n, step(op))))
    })
    .into()
}

// ↑ and ↓ scroll the tab as they do the Memory tab's.
fn step(op: ScrollOp) -> ScrollOp {
    match op {
        ScrollOp::By(y) => ScrollOp::By(-y),
        ScrollOp::To(_) | ScrollOp::Show { .. } => ScrollOp::By(0.0),
    }
}

fn project<'a>(app: &App, p: &'a ProjectNotes, t: &titles::Titles) -> Element<'a, Message> {
    let head = widgets::sectioned(
        vec![bold(p.project.as_str(), Tone::Bright).into()],
        format!("review:notes:{}", p.project),
        Message::Review(ReviewSection::Notes, p.project.as_str().to_owned()),
    );
    let mut col = column![widgets::hline(theme::seam), head].spacing(LIST_GAP);
    for n in &p.notes {
        col = col.push(note(app, n, t));
    }
    col.into()
}

// `kind  by  ago`, then what it holds, indented under it.
fn note<'a>(app: &App, n: &'a Note, t: &titles::Titles) -> Element<'a, Message> {
    let ago = format!("  {} ago", time::duration(app.now_ms - n.time));
    let head = widgets::centred(vec![
        bold(format!("{}  ", n.what.kind()), Tone::Text).into(),
        txt(by(n), Tone::Dim).into(),
        txt(ago, Tone::Dim).into(),
    ]);
    let body: Element<'a, Message> = match &n.what {
        NoteWhat::Survey { text, .. } | NoteWhat::Feedback { text } | NoteWhat::Lesson { text } => {
            prose::view(text, Tone::Text, t)
        }
        NoteWhat::Smell { file, line, what, why } => column![
            widgets::wrapped(format!("{file}:{line}  {what}"), Tone::Bright),
            widgets::wrapped(why.as_str(), Tone::Dim),
        ]
        .into(),
        NoteWhat::Proposal { title, text, files } => {
            let mut col = column![widgets::wrapped(title.as_str(), Tone::Bright), prose::view(text, Tone::Text, t)];
            for file in files {
                col = col.push(widgets::wrapped(file.as_str(), Tone::Dim));
            }
            col.into()
        }
        NoteWhat::Handoff { dir, files, skipped, .. } => {
            let mut col = column![widgets::wrapped(dir.as_str(), Tone::Text)];
            for f in files {
                col = col.push(txt(format!("{}  {} bytes", f.path, f.bytes), Tone::Dim));
            }
            for s in skipped {
                col = col.push(txt(format!("{s}  not copied"), Tone::Dim));
            }
            col.into()
        }
    };
    let body = container(body).padding(iced::Padding::ZERO.left(INDENT * fonts::ADVANCE));
    widgets::clickable(format!("note:{}", n.id), column![head, body], None)
}

// Its seat's role or name and card; a survey's own heading, or none outside a seat.
fn by(n: &Note) -> String {
    if let NoteWhat::Survey { from, .. } = &n.what {
        return format!("by {from}");
    }
    let card = n.card.as_ref().map(|c| c.as_str());
    let Some(seat) = &n.seat else {
        return card.map_or_else(|| "filed outside a seat".to_owned(), |c| format!("of {c}"));
    };
    let mut parts = seat.as_str().split('/');
    let first = parts.next().unwrap_or_default();
    let name = parts.next_back().unwrap_or(first);
    format!("by {name} ({})", card.unwrap_or(first))
}
