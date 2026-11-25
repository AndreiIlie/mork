//! Main's Projects tab reads only the screen; its buttons send the same look-back command as the CLI.

use factory_types::lookback::{LastView, ProjectView, Read, ReadView};
use factory_types::{Command, ProjectId};
use iced::alignment::Vertical;
use iced::widget::{Column, Row, column, container, responsive};
use iced::{Element, Length};

use crate::app::{App, Effect, Message, ScrollOp};
use crate::spans::Tone;
use crate::widgets::{self, bold, boxed_button, txt};
use crate::{board, input, theme, time, wrappers};

const LIST: &str = "projects";
const BAR: f32 = 8.0;
const GAP: f32 = 6.0;

pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let mut col = Column::new().spacing(board::GAP).push(bold("Projects", Tone::Bright));
        for p in &app.screen.projects {
            col = col.push(project(app, p));
        }
        let width = (size.width - BAR).max(0.0);
        let list = widgets::scroll(container(col).width(Length::Fixed(width))).id(LIST);
        wrappers::scrolled(list, LIST, app.scroll.map(|(n, op)| (n, step(op))))
    })
    .into()
}

fn step(op: ScrollOp) -> ScrollOp {
    match op {
        ScrollOp::By(y) => ScrollOp::By(-y),
        ScrollOp::To(_) | ScrollOp::Show { .. } => ScrollOp::By(0.0),
    }
}

fn project<'a>(app: &App, p: &'a ProjectView) -> Element<'a, Message> {
    let running = p.rows.iter().any(|r| r.running.is_some());
    let mut col = column![widgets::hline(theme::seam), bold(p.project.as_str(), Tone::Bright)].spacing(GAP);
    for read in &p.rows {
        col = col.push(read_row(&p.project, read, running, app.now_ms));
    }
    col.into()
}

fn read_row<'a>(project: &ProjectId, read: &'a ReadView, running: bool, now_ms: i64) -> Element<'a, Message> {
    let spec = read.read.spec();
    let last = read.last.as_ref().map_or_else(|| "Last run: none yet".to_owned(), |c| last_run(c, now_ms));
    let id = format!("lookback:{project}:{}", spec.name);
    let mut row = Row::new().spacing(GAP).align_y(Vertical::Center);
    if let Some(card) = &read.running {
        row = row.push(widgets::clickable(id, txt(card.as_str(), Tone::Green), Some(Message::Open(card.to_string()))));
    } else if !running {
        row = row.push(boxed_button(
            id,
            spec.button.to_owned(),
            Tone::Green,
            Message::Lookback(project.clone(), read.read),
        ));
    }
    row.push(column![widgets::wrapped(spec.detail, Tone::Text), widgets::wrapped(last, Tone::Dim)].width(Length::Fill))
        .into()
}

fn last_run(last: &LastView, now_ms: i64) -> String {
    let since = match last.commits {
        None => String::new(),
        Some(1) => ", 1 commit since".into(),
        Some(n) => format!(", {n} commits since"),
    };
    let age = time::duration(now_ms.saturating_sub(last.ended_ms));
    format!("Last run: {} ({age} ago{since})", last.date)
}

impl App {
    pub(crate) fn lookback(&mut self, project: ProjectId, read: Read) -> Vec<Effect> {
        self.note = None;
        self.act_on(input::pressed(&Command::Lookback { project, read: Some(read.spec().name.to_owned()) }))
    }
}
