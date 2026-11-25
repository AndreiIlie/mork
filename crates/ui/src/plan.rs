//! The side column's Plan section (fac-36, fac-105, fac-128): deliverables that fold, their tasks
//! behind their marks.

use std::collections::BTreeMap;

use factory_types::card::Plan;
use factory_types::view::{Doing, PlaceView};
use iced::widget::{Column, container, row};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::icons::{self, Icon};
use crate::side::Section;
use crate::spans::Tone;
use crate::widgets::{self, txt};
use crate::{fonts, side, window};

/// The plan's width: the side column's inside its padding.
pub const WIDTH: f32 = window::SIDE_INNER;
/// A row: a deliverable's or a task's.
pub const ROW: f32 = 20.0;
/// A task's indent under its deliverable's name, past the chevron; and its words', past its mark.
pub const INDENT: f32 = icons::SIZE + fonts::ADVANCE;

/// A card's deliverables a click folded or unfolded, by name: open or not.
pub type Folds = BTreeMap<String, bool>;

/// How a task stands against the plan's current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Done,
    Current,
    ToCome,
}

/// One row of the plan, its words as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanLine {
    /// A deliverable: its index, its name, its tasks done of all, whether it is the plan's current
    /// one, and whether its tasks show under it.
    Deliverable { index: usize, name: String, done: usize, tasks: usize, current: bool, open: bool },
    /// A task: its deliverable's index and its own, and how it stands.
    Task { deliverable: usize, task: usize, at: At, text: String },
}

/// How a deliverable's task stands: each of a deliverable before the current one is done, each
/// of one after it to come, and in the current one those before its current task are done.
pub fn at(plan: &Plan, deliverable: usize, task: usize) -> At {
    let current = plan.deliverables.get(deliverable).map_or(0, |d| d.current);
    match deliverable.cmp(&plan.current).then(task.cmp(&current)) {
        std::cmp::Ordering::Less => At::Done,
        std::cmp::Ordering::Equal => At::Current,
        std::cmp::Ordering::Greater => At::ToCome,
    }
}

/// How many of a deliverable's tasks are done.
pub fn done(plan: &Plan, deliverable: usize) -> usize {
    let tasks = plan.deliverables.get(deliverable).map_or(0, |d| d.tasks.len());
    (0..tasks).filter(|t| at(plan, deliverable, *t) == At::Done).count()
}

/// Whether a deliverable's tasks show: as a click last left them on this card, else only the
/// current deliverable's.
pub fn open(plan: &Plan, folds: &Folds, deliverable: usize) -> bool {
    let name = plan.deliverables.get(deliverable).map(|d| d.name.as_str());
    name.and_then(|n| folds.get(n)).copied().unwrap_or(deliverable == plan.current)
}

/// The plan's rows in `width`: each deliverable's, and under an open one its tasks', the words
/// cut with `…` to what fits beside the done count, or past the mark.
pub fn lines(plan: &Plan, folds: &Folds, width: f32) -> Vec<PlanLine> {
    let mut out = Vec::new();
    for (d, del) in plan.deliverables.iter().enumerate() {
        let done = done(plan, d);
        let count = format!("{done}/{}", del.tasks.len()).len();
        let name = widgets::cut(&words(&del.name), widgets::chars_in(width - INDENT).saturating_sub(count + 1));
        let open = open(plan, folds, d);
        let tasks = del.tasks.len();
        out.push(PlanLine::Deliverable { index: d, name, done, tasks, current: d == plan.current, open });
        if open {
            let chars = widgets::chars_in(width - 2.0 * INDENT);
            out.extend(del.tasks.iter().enumerate().map(|(t, task)| PlanLine::Task {
                deliverable: d,
                task: t,
                at: at(plan, d, t),
                text: widgets::cut(&words(task), chars),
            }));
        }
    }
    out
}

fn words(s: &str) -> String {
    crate::text::clean(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The open place's plan, when it has deliverables.
pub fn of(app: &App) -> Option<&Plan> {
    app.place()?.plan.as_ref().filter(|p| !p.deliverables.is_empty())
}

/// The open card's folds, the same in its room and its DMs.
pub fn folds(app: &App) -> &Folds {
    static NONE: Folds = BTreeMap::new();
    card(app).and_then(|c| app.plan_folds.get(c)).unwrap_or(&NONE)
}

fn card(app: &App) -> Option<&str> {
    app.place()?.card.as_ref().map(|h| h.id.as_str())
}

/// Whether one of the card's agents works: its room's, its DMs' or its children's, from any of them.
pub fn working(app: &App) -> bool {
    let doing = |v: &PlaceView| v.agents.iter().chain(&v.sessions).any(|a| matches!(a.doing, Doing::Working { .. }));
    app.place().is_some_and(doing)
}

/// Whether the section shows the current task's mark turning: while an agent works, as the animations run.
pub fn turning(app: &App) -> bool {
    let card = app.place().is_some_and(|v| v.card.is_some()) && working(app);
    let Some(plan) = of(app).filter(|_| card && app.section_open(Section::Plan)) else { return false };
    lines(plan, folds(app), WIDTH).iter().any(|l| matches!(l, PlanLine::Task { at: At::Current, .. }))
}

impl App {
    pub(crate) fn fold_deliverable(&mut self, deliverable: usize) {
        let (Some(plan), Some(card)) = (of(self), card(self)) else { return };
        let Some(name) = plan.deliverables.get(deliverable).map(|d| d.name.clone()) else { return };
        let (open, card) = (open(plan, folds(self), deliverable), card.to_owned());
        self.plan_folds.entry(card).or_default().insert(name, !open);
    }
}

/// The right of the section's header: the plan's tasks done of all.
pub fn summary(app: &App) -> Element<'_, Message> {
    let Some(plan) = of(app) else { return side::summary(String::new()) };
    let done: usize = (0..plan.deliverables.len()).map(|d| done(plan, d)).sum();
    let all: usize = plan.deliverables.iter().map(|d| d.tasks.len()).sum();
    side::summary(format!("{done}/{all}"))
}

/// The section's body: every deliverable and every unfolded task.
pub fn body(app: &App) -> Element<'_, Message> {
    let Some(plan) = of(app) else { return side::empty("no plan yet") };
    let lines = lines(plan, folds(app), WIDTH);
    let rows = lines.iter().map(|l| line(app, l));
    let list = Column::with_children(rows).width(Length::Fill);
    container(list).id("plan").width(Length::Fill).into()
}

/// The id of a task's mark, which says how the task stands: `plan:done:0:3`,
/// `plan:current:1:0`, `plan:to-come:1:1`, by its deliverable and itself.
pub fn mark_id(deliverable: usize, task: usize, at: At) -> String {
    let at = match at {
        At::Done => "done",
        At::Current => "current",
        At::ToCome => "to-come",
    };
    format!("plan:{at}:{deliverable}:{task}")
}

/// The id of a deliverable's row, which a click folds or unfolds.
pub fn deliverable_id(deliverable: usize) -> String {
    format!("plan:deliverable:{deliverable}")
}

fn line<'a>(app: &App, l: &PlanLine) -> Element<'a, Message> {
    let cells: Vec<Element<'a, Message>> = match l {
        PlanLine::Deliverable { index, name, done, tasks, current, open } => {
            let chevron = icons::icon(if *open { Icon::Unfolded } else { Icon::Folded }, Tone::Dim);
            let tone = if *current { Tone::Bright } else { Tone::Text };
            let head = vec![
                chevron,
                widgets::gap(fonts::ADVANCE),
                txt(name.clone(), tone).into(),
                iced::widget::space().width(Length::Fill).into(),
                txt(format!("{done}/{tasks}"), Tone::Dim).into(),
            ];
            let head = row(head).height(Length::Fixed(ROW)).align_y(iced::alignment::Vertical::Center);
            return widgets::clickable(deliverable_id(*index), head, Some(Message::PlanFold(*index)));
        }
        PlanLine::Task { deliverable, task, at, text } => {
            let (mark, tone) = match at {
                At::Done => (icons::icon(Icon::Done, Tone::Green), Tone::Dim),
                At::Current if app.settings.animations && working(app) => {
                    (icons::working(Tone::Green, app.now_ms), Tone::Bright)
                }
                At::Current => (icons::icon(Icon::Working, Tone::Green), Tone::Bright),
                At::ToCome => (icons::icon(Icon::Pending, Tone::Dim), Tone::Text),
            };
            let mark = container(mark).id(mark_id(*deliverable, *task, *at)).into();
            vec![widgets::gap(INDENT), mark, widgets::gap(fonts::ADVANCE), txt(text.clone(), tone).into()]
        }
    };
    row(cells).height(Length::Fixed(ROW)).align_y(iced::alignment::Vertical::Center).into()
}
