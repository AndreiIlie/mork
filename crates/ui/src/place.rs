//! The place shown (5.7, 5.8). Main has no input, so no draft's images nor Send now either
//! (fac-230).

use iced::widget::{Column, column, container, responsive, row, text, text_editor};
use iced::{Element, Length, Padding, Pixels};
use mork_types::view::{AgentRow, CardHeader, Doing, PlaceKind, PlaceView};

use crate::app::{App, Message};
use crate::icons::{self, Icon};
use crate::session::Tab;
use crate::spans::{self, Span, Tone};
use crate::widgets::{self, bold, boxed_button, clickable, small, txt};
use crate::{
    board, conversation, fonts, hands, keys, memory, overlay, project_notes, projects, tabs, theme, window, wrappers,
};

/// The input's id.
pub const INPUT: &str = "input";
/// An agent's row: 24 px, its icon in the middle (fac-103).
pub const AGENT_ROW: f32 = 24.0;

// The gaps around the conversation, the buttons and the input, inside the place's panel (fac-66).
const HEAD_PAD: f32 = 8.0;
const UNDER_HEAD: f32 = 4.0;
const ABOVE_INPUT: f32 = 6.0;
const FIELD_Y: f32 = 4.0;
// Send now's lift from the input's bottom, centring it on the field's last line.
const SEND_NOW_Y: f32 = 3.0;
const FOOT_PAD: f32 = 6.0;
// The lit stage's pill, either side of its name.
const PILL_X: f32 = 7.0;
// The gap under main's tab row.
const UNDER_TABS: f32 = 10.0;
// A title's characters at least, beside its card's tabs.
const MIN_TITLE: usize = 16;

/// The place's area: a room's header across its panel's top, the rest inset under it.
pub fn view(app: &App) -> Element<'_, Message> {
    // Only `responsive` knows the place's height, a third of which caps the input and asks.
    responsive(move |size| area(app, size.height)).into()
}

fn area(app: &App, place_h: f32) -> Element<'_, Message> {
    let (head, summary, body) = match (&app.overlay, app.place()) {
        (Some(o), _) => (None, None, overlay::view(app, o)),
        (None, _) if app.session.place == "main" => (None, None, main(app)),
        (None, Some(v)) => (
            Some(header(app, v)),
            crate::summary::view(app, v, place_h / 4.0),
            column![agents(app, v), conversation::view(app, v)].into(),
        ),
        (None, None) => {
            let waiting =
                column![txt(app.session.place.clone(), Tone::Bright), txt("waiting for the engine's view", Tone::Dim)];
            (None, None, waiting.into())
        }
    };
    let mut below = Column::new();
    if let Some(asks) = hands::view(app, place_h / 3.0) {
        below = below.push(asks);
    }
    if let Some(note) = &app.note {
        below = below.push(widgets::wrapped(note.clone(), Tone::Orange));
    }
    if let Some(lists) = crate::lists::view(app) {
        below = below.push(lists);
    }
    below = below.push(buttons(app));
    if app.takes_input() {
        below = below.push(draft_images(app)).push(input(app, input_max_h(place_h)));
    }
    below = below.push(footer(app.takes_input()));
    let col = column![container(body).height(Length::Fill), wrappers::full_height(below)];
    let top = if head.is_some() { UNDER_HEAD } else { window::PAD };
    let mut inset = Padding { top, right: window::INSET, bottom: FOOT_PAD, left: window::INSET };
    let mut frame = Column::new();
    if let Some(h) = head {
        frame = frame.push(h);
    }
    // The lower controls can take all the remaining space, so the summary needs its own share first.
    if let Some(s) = summary {
        frame = frame.push(container(s).padding(inset.bottom(0.0)));
        inset.top = 0.0;
    }
    frame.push(container(col).padding(inset).height(Length::Fill)).into()
}

fn main(app: &App) -> Element<'_, Message> {
    let mut tabs = row![];
    for (i, t) in Tab::ALL.into_iter().enumerate() {
        if i > 0 {
            tabs = tabs.push(txt(" · ", Tone::Rule));
        }
        let (id, name) = tab_name(t);
        let words = if app.session.tab == t { bold(name, Tone::Bright) } else { txt(name, Tone::Dim) };
        tabs = tabs.push(clickable(format!("tab:{id}"), words, Some(Message::Tab(t))));
    }
    let body: Element<'_, Message> = match (app.session.tab, app.place()) {
        (Tab::Board, _) => board::view(app),
        (Tab::Engine, Some(v)) => column![agents(app, v), conversation::view(app, v)].into(),
        (Tab::Engine, None) => txt("waiting for the engine's view", Tone::Dim).into(),
        (Tab::Memory, _) => memory::view(app),
        (Tab::Notes, _) => project_notes::view(app),
        (Tab::Projects, _) => projects::view(app),
    };
    column![tabs.align_y(iced::alignment::Vertical::Center), widgets::gap_v(UNDER_TABS), body].into()
}

// A tab's id, after `tab:`, and its words.
fn tab_name(t: Tab) -> (&'static str, &'static str) {
    match t {
        Tab::Board => ("board", "Board"),
        Tab::Engine => ("engine", "Engine"),
        Tab::Memory => ("memory", "Memory"),
        Tab::Notes => ("notes", "Notes"),
        Tab::Projects => ("projects", "Projects"),
    }
}

// A room's header: its lines, its card's tabs at its right, an underline under them.
fn header<'a>(app: &'a App, v: &'a PlaceView) -> Element<'a, Message> {
    let lines: Element<'a, Message> = match &v.card {
        Some(h) => {
            let tabs = tabs::of(app, v);
            responsive(move |size| head_lines(app, h, &tabs, widgets::chars_in(size.width)))
                .height(Length::Shrink)
                .into()
        }
        None => txt(place_name(v), Tone::Bright).into(),
    };
    let pad = Padding { top: HEAD_PAD, right: window::INSET, bottom: HEAD_PAD, left: window::INSET };
    let head = container(lines).padding(pad).width(Length::Fill);
    let line = container(text("")).width(Length::Fill).height(Length::Fixed(1.0)).style(theme::underline);
    column![head, container(line).padding([0.0, window::INSET])].into()
}

// Its tabs beside its title while that leaves the title some room, else on their own line under it.
fn head_lines<'a>(app: &'a App, h: &'a CardHeader, tabs: &[tabs::Tab], chars: usize) -> Element<'a, Message> {
    let used = h.id.chars().count() + 2;
    let wide = tabs::chars(app, tabs);
    let beside = tabs.is_empty() || used + MIN_TITLE + wide <= chars;
    let title = widgets::cut(&crate::text::clean(&h.title), chars.saturating_sub(used + if beside { wide } else { 0 }));
    let header = row![
        bold(h.id.to_string(), Tone::Bright),
        txt(format!("  {title}"), Tone::Text),
        iced::widget::space().width(Length::Fill),
    ];
    let tabs = tabs::view(app, tabs.to_vec());
    let mut col = Column::new();
    if beside {
        col = col.push(header.push(tabs));
    } else {
        col = col.push(header).push(container(tabs).width(Length::Fill).align_x(iced::alignment::Horizontal::Right));
    }
    let facts = txt(spans::facts(&h.facts, chars), Tone::Dim);
    col.push(strip(&spans::strip(h))).push(facts).into()
}

// Each step's time under its name (fac-56); the current step's name, the strip's one bold green
// span, is a lit pill, and its time is inset by the pill's padding to stay under it.
fn strip<'a>(s: &spans::Strip) -> Element<'a, Message> {
    let mut cells: Vec<Element<'a, Message>> = Vec::new();
    for (i, step) in s.steps.iter().enumerate() {
        if i > 0 {
            cells.push(widgets::span(&Span::new(" · ", Tone::Rule)).into());
        }
        let lit = step.name.bold && step.name.tone == Tone::Green;
        let inset = if lit { PILL_X } else { 0.0 };
        let name = container(widgets::span(&step.name)).padding([0.0, inset]);
        let mut cell = Column::new().push(if lit { name.style(theme::stage_pill) } else { name });
        if let Some(t) = &step.time {
            cell = cell.push(container(widgets::span(t)).padding(Padding::ZERO.left(inset)));
        }
        cells.push(cell.into());
    }
    cells.push(widgets::span(&s.aside).into());
    row(cells).height(Length::Fixed(2.0 * fonts::LINE)).into()
}

fn place_name(v: &PlaceView) -> String {
    match &v.kind {
        PlaceKind::Dm { card, role } => format!("{card} · dm {}", role.as_str()),
        PlaceKind::Child { card, agent, .. } => format!("{card} · foreman › {}", child_name(agent)),
        PlaceKind::Card { card } => card.to_string(),
        PlaceKind::Spec { card } => format!("{card} · spec"),
        PlaceKind::Challenge { card } => format!("{card} · challenge"),
        PlaceKind::Main => "main".to_owned(),
        PlaceKind::Other => v.place.to_string(),
    }
}

fn child_name(agent: &mork_types::AgentKey) -> &str {
    agent.as_str().rsplit('/').next().unwrap_or_default()
}

fn agents<'a>(app: &'a App, v: &'a PlaceView) -> Element<'a, Message> {
    Column::with_children(v.agents.iter().map(|a| agent(app, a))).into()
}

fn agent<'a>(app: &'a App, a: &'a AgentRow) -> Element<'a, Message> {
    let working = matches!(a.doing, Doing::Working { .. });
    let (icon, tone) = if working { (Icon::Working, Tone::Green) } else { (Icon::Agent, spans::agent_light(a.light)) };
    let icon =
        if working && app.settings.animations { icons::working(tone, app.now_ms) } else { icons::icon(icon, tone) };
    let name = [Span::new(spans::agent_name(a), spans::role(a.role))];
    let mut setting = spans::setting(a);
    let mut rest = vec![Span::new("   ", Tone::Dim), spans::doing(&a.doing)];
    let ctx = a.context.map(|(used, window)| {
        rest.push(Span::new("   ", Tone::Dim));
        spans::context(used, window)
    });
    let setting: Element<'a, Message> = if a.choices.is_empty() {
        widgets::line(&setting).into()
    } else {
        setting.push(Span::new(" ▾", Tone::Dim));
        clickable(format!("pick:{}", a.agent), widgets::line(&setting), Some(Message::Picker(a.agent.to_string())))
    };
    let mut cells: Vec<Element<'a, Message>> = vec![
        container(icon).height(Length::Fixed(AGENT_ROW)).align_y(iced::alignment::Vertical::Center).into(),
        widgets::gap(fonts::ADVANCE),
        widgets::line(&name).into(),
        setting,
        widgets::line(&rest).into(),
    ];
    if let Some(t) = &a.turn {
        let n = t.steps.len();
        let steps = widgets::line(&[Span::new(format!("   {n} step{} ▸", if n == 1 { "" } else { "s" }), Tone::Dim)]);
        cells.push(clickable(format!("turn:{}", a.agent), steps, Some(Message::Turn(a.agent.to_string()))));
    }
    cells.push(iced::widget::space().width(Length::Fill).into());
    if let Some(c) = ctx {
        cells.push(widgets::span(&c).into());
    }
    row(cells).height(Length::Fixed(AGENT_ROW)).align_y(iced::alignment::Vertical::Center).into()
}

fn buttons(app: &App) -> Element<'_, Message> {
    let offers = app.place().map(|v| v.offers.as_slice()).unwrap_or(&[]);
    let mut r = row![].spacing(fonts::ADVANCE * 2.0);
    for (i, o) in offers.iter().enumerate() {
        let press = Message::Press {
            ask: None,
            place: app.session.place.clone(),
            label: o.label.clone(),
            command: Box::new(o.command.clone()),
        };
        r = r.push(boxed_button(format!("offer:{i}"), o.label.clone(), Tone::Green, press));
    }
    if offers.is_empty() {
        return r.into();
    }
    container(r.wrap()).padding(Padding::ZERO.top(ABOVE_INPUT)).into()
}

// Each pasted image with its × (fac-31).
fn draft_images(app: &App) -> Element<'_, Message> {
    let mut r = row![].spacing(fonts::ADVANCE * 2.0).align_y(iced::alignment::Vertical::Top);
    for (i, path) in app.draft_images().iter().enumerate() {
        let thumb = clickable(format!("draft-image:{i}"), widgets::thumbnail(path), None);
        let remove = clickable(format!("unpaste:{i}"), txt("×", Tone::Dim), Some(Message::Unpaste(i)));
        r = r.push(row![thumb, remove]);
    }
    r.wrap().into()
}

// Whole lines, so a capped input never shows a row cut; its field's padding counts in the third.
fn input_max_h(place_h: f32) -> f32 {
    ((place_h / 3.0 - 2.0 * FIELD_Y) / fonts::LINE).floor().max(1.0) * fonts::LINE
}

// No fixed height: the editor measures its wrapped lines, which `line_count` leaves out.
fn input(app: &App, max_h: f32) -> Element<'_, Message> {
    let to = app.context().to;
    let lines = app.input.line_count().max(1);
    let editor = text_editor(&app.input)
        .id(INPUT)
        .placeholder(if to.is_empty() || to == "planner" { "say".to_owned() } else { format!("say to {to}") })
        .on_action(Message::Edit)
        .key_binding(keys::binding(lines > 1))
        .font(fonts::MONO)
        .size(fonts::SIZE)
        .line_height(iced::widget::text::LineHeight::Absolute(Pixels(fonts::LINE)))
        .padding(0)
        .max_height(max_h)
        .style(theme::input);
    // The spec room's one seat is its planner: plain `say`, no prefix (fac-633).
    let field = row![bold("› ", Tone::White), editor];
    let field = container(field).padding([FIELD_Y, 10.0]).width(Length::Fill).style(theme::field);
    // Enter queues the draft; this cuts the recipient's running turn (fac-78).
    let now = boxed_button("send-now".into(), "send now".into(), Tone::Orange, Message::SendNow);
    let now = container(now).padding(Padding::ZERO.bottom(SEND_NOW_Y));
    let line = row![field, now].spacing(fonts::ADVANCE).align_y(iced::alignment::Vertical::Bottom);
    container(line).padding(Padding::ZERO.top(ABOVE_INPUT)).into()
}

// Main takes no lines, so it offers no commands (fac-230).
fn footer<'a>(input: bool) -> Element<'a, Message> {
    let keys = if input { "tab unfold   ↑↓ scroll   / commands" } else { "tab unfold   ↑↓ scroll" };
    container(small(keys, Tone::Dim)).padding(Padding::ZERO.top(3.0)).into()
}
