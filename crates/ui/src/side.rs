//! The side column's folds belong to the window (fac-105), shared by main and every card and
//! kept across restarts.

use iced::widget::{Column, container, row};
use iced::{Element, Length, Padding};

use crate::app::{App, Effect, Message};
use crate::icons::{self, Icon};
use crate::spans::Tone;
use crate::widgets::{self, bold, clickable, txt};
use crate::{fonts, theme, window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Usage,
    Card,
    Context,
    Outputs,
    Notes,
    Plan,
}

impl Section {
    /// The sections, top to bottom.
    pub const ALL: [Section; 6] =
        [Section::Usage, Section::Card, Section::Context, Section::Outputs, Section::Notes, Section::Plan];

    /// Its header's name.
    pub fn name(self) -> &'static str {
        match self {
            Section::Usage => "Usage",
            Section::Card => "Card",
            Section::Context => "Context",
            Section::Outputs => "Outputs",
            Section::Notes => "Notes",
            Section::Plan => "Plan",
        }
    }

    /// Its key in the window's state and its ids: `side:notes`, `side:notes:body`.
    pub fn key(self) -> &'static str {
        match self {
            Section::Usage => "usage",
            Section::Card => "card",
            Section::Context => "context",
            Section::Outputs => "outputs",
            Section::Notes => "notes",
            Section::Plan => "plan",
        }
    }
}

/// The id of the column's scroll.
pub const SCROLL: &str = "side:scroll";
/// A header's padding above and below its line.
pub const HEAD_PAD: f32 = 6.0;
// The gap after a header's chevron, and under an open section's body.
const CHEVRON_GAP: f32 = 6.0;
const BODY_PAD: f32 = 8.0;

impl App {
    pub fn section_open(&self, s: Section) -> bool {
        self.session.sections.get(s.key()).copied().unwrap_or(true)
    }

    pub(crate) fn fold_section(&mut self, s: Section) {
        let open = !self.section_open(s);
        self.session.sections.insert(s.key().to_owned(), open);
    }

    /// A card's pull request's page, only the one the latest view links.
    pub(crate) fn link(&mut self, url: String) -> Vec<Effect> {
        let linked = self.place().and_then(|v| v.card.as_ref()).and_then(|h| h.about.pr_url.as_deref());
        if linked != Some(url.as_str()) {
            self.note = Some("✗ that pull request is no longer linked: nothing opened".into());
            return Vec::new();
        }
        vec![Effect::Link(url)]
    }
}

/// Main's Usage section, or all sections beside a card's place; none beside any other place.
pub fn view(app: &App) -> Option<Element<'_, Message>> {
    let sections: &[Section] = if app.session.place == "main" {
        &[Section::Usage]
    } else {
        app.place()?.card.as_ref()?;
        &Section::ALL
    };
    // Sections apart by the panes' divider, edge to edge (fac-168); their words inside the padding.
    let mut col = Column::new();
    for (i, &s) in sections.iter().enumerate() {
        if i > 0 {
            col = col.push(widgets::hline(theme::divider));
        }
        col = col.push(container(section(app, s)).padding(Padding::ZERO.horizontal(window::PAD)));
    }
    let scroll = widgets::scroll(col.width(Length::Fill)).id(SCROLL);
    Some(container(scroll).id("side").width(Length::Fixed(window::SIDE_W)).height(Length::Fill).into())
}

fn section(app: &App, s: Section) -> Element<'_, Message> {
    let open = app.section_open(s);
    let summary = match s {
        Section::Usage => widgets::gap(0.0),
        Section::Card | Section::Context | Section::Outputs => crate::sections::summary(app, s),
        Section::Notes => crate::notes::summary(app),
        Section::Plan => crate::plan::summary(app),
    };
    let chevron = icons::icon(if open { Icon::Unfolded } else { Icon::Folded }, Tone::Dim);
    let line = row![chevron, widgets::gap(CHEVRON_GAP), bold(s.name(), Tone::Strong)]
        .push(iced::widget::space().width(Length::Fill))
        .push(summary)
        .height(Length::Fixed(fonts::LINE))
        .align_y(iced::alignment::Vertical::Center);
    let band = container(line).padding(Padding::ZERO.vertical(HEAD_PAD)).width(Length::Fill);
    let head = clickable(format!("side:{}", s.key()), band, Some(Message::Section(s)));
    if !open {
        return head;
    }
    let body = match s {
        Section::Usage => crate::usage::body(app),
        Section::Card | Section::Context | Section::Outputs => crate::sections::body(app, s),
        Section::Notes => crate::notes::body(app),
        Section::Plan => crate::plan::body(app),
    };
    let body = container(body).id(format!("side:{}:body", s.key())).padding(Padding::ZERO.bottom(BODY_PAD));
    Column::new().push(head).push(body).into()
}

/// A section's empty body: one dim line saying so.
pub fn empty<'a>(words: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Message> {
    widgets::wrapped(words, Tone::Dim).into()
}

pub fn summary<'a>(words: String) -> Element<'a, Message> {
    txt(words, Tone::Dim).into()
}
