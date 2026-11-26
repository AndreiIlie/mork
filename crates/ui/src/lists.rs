//! The agents' step lists above a place's input (fac-128): a box per agent whose running turn
//! keeps one, a row per step behind its mark: done, turning while in progress, or still to come.

use iced::widget::{Column, container, responsive};
use iced::{Element, Length, Padding};
use mork_types::seen::{AgentList, ListStep, StepStatus};

use crate::app::{App, Message};
use crate::icons::{self, Icon};
use crate::spans::Tone;
use crate::widgets::{self, txt};
use crate::{fonts, theme};

// Above each box, and inside its edge, as the input's field.
const ABOVE: f32 = 6.0;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 4.0;

/// The place's lists stacked, or none when none shows.
pub fn view(app: &App) -> Option<Element<'_, Message>> {
    let lists = app.place().map(|v| v.lists.as_slice()).filter(|l| !l.is_empty())?;
    let named = lists.len() > 1;
    let boxes = lists.iter().map(|l| {
        let b = responsive(move |size| list_box(app, l, named, size.width)).height(Length::Shrink);
        container(b).padding(Padding::ZERO.top(ABOVE)).into()
    });
    Some(Column::with_children(boxes).into())
}

fn list_box<'a>(app: &'a App, l: &'a AgentList, named: bool, width: f32) -> Element<'a, Message> {
    let words = width - 2.0 * (PAD_X + theme::BORDER) - icons::SIZE - fonts::ADVANCE;
    let chars = widgets::chars_in(words);
    let mut col = Column::new();
    if named {
        col = col.push(txt(l.agent.as_str().rsplit('/').next().unwrap_or_default().to_owned(), Tone::Dim));
    }
    for s in &l.steps {
        col = col.push(step(app, s, chars));
    }
    container(col).padding([PAD_Y, PAD_X]).width(Length::Fill).style(theme::field).into()
}

fn step<'a>(app: &App, s: &ListStep, chars: usize) -> Element<'a, Message> {
    let (mark, tone) = match s.status {
        StepStatus::Completed => (icons::icon(Icon::Done, Tone::Green), Tone::Dim),
        StepStatus::InProgress if app.settings.animations => (icons::working(Tone::Green, app.now_ms), Tone::Bright),
        StepStatus::InProgress => (icons::icon(Icon::Working, Tone::Green), Tone::Bright),
        StepStatus::Pending => (icons::icon(Icon::Pending, Tone::Dim), Tone::Text),
    };
    let words = txt(widgets::cut(&crate::text::clean(&s.words), chars), tone);
    widgets::centred(vec![mark, widgets::gap(fonts::ADVANCE), words.into()]).into()
}
