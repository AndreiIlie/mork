//! The side column's first three sections (fac-105): card, sessions' context and cost, and the
//! files its turns produced.

use factory_types::about::{CardAbout, SeatCost};
use factory_types::cost::Tokens;
use factory_types::seen::OutputFile;
use factory_types::view::{AgentRow, CardHeader, PlaceView, StageAt};
use iced::widget::{Column, row};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::icons::{self, Icon};
use crate::side::{self, Section};
use crate::spans::{self, Span, Tone};
use crate::text::clean;
use crate::widgets::{self, clickable, txt};

/// The characters of a row's key or session's name, before its value.
pub const KEY: usize = 11;
// The widest the sessions' names grow: a longer one is cut from its start, keeping its number.
const NAME_MAX: usize = 20;

/// The right of a section's header: its sessions, its run's cost, its files.
pub fn summary(app: &App, s: Section) -> Element<'_, Message> {
    let Some(v) = app.place() else { return side::summary(String::new()) };
    let words = match s {
        Section::Card => count(v.sessions.len(), "session"),
        Section::Context => v.card.as_ref().map(|h| cost(&h.about).0).unwrap_or_default(),
        _ => count(v.outputs.len(), "file"),
    };
    side::summary(words)
}

fn count(n: usize, what: &str) -> String {
    match n {
        0 => String::new(),
        1 => format!("1 {what}"),
        n => format!("{n} {what}s"),
    }
}

/// The run's cost so far as its journal holds it, and its tone: orange when a turn's is unknown.
pub fn cost(a: &CardAbout) -> (String, Tone) {
    match (a.run_cost_cents, a.run_cost_unknown) {
        (Some(c), false) => (spans::dollars(c), Tone::Text),
        (Some(c), true) => (format!("{} + ?", spans::dollars(c)), Tone::Orange),
        (None, true) => ("?".into(), Tone::Orange),
        (None, false) => (String::new(), Tone::Dim),
    }
}

/// A section's body, for the place shown.
pub fn body(app: &App, s: Section) -> Element<'_, Message> {
    let Some(v) = app.place() else { return side::empty("no view yet") };
    match s {
        Section::Card => card(app, v),
        Section::Context => context(app, v),
        _ => outputs(app, v),
    }
}

fn pair<'a>(key: &str, value: Element<'a, Message>) -> Element<'a, Message> {
    keyed(key, KEY, value)
}

fn keyed<'a>(key: &str, width: usize, value: Element<'a, Message>) -> Element<'a, Message> {
    row![txt(format!("{key:<width$}"), Tone::Dim), value].height(Length::Fixed(crate::fonts::LINE)).into()
}

// Each session's name, whole and told apart: a card's DM agent as its tab says it (`dm foreman`),
// a child by its own (`researcher-2`); and the width they take, at least a key's.
fn names(v: &PlaceView) -> (Vec<String>, usize) {
    let whole = v.sessions.iter().map(|a| {
        let name = clean(spans::agent_name(a));
        if a.agent.as_str().ends_with("/dm") { format!("dm {name}") } else { name }
    });
    let whole: Vec<String> = whole.collect();
    let width = (whole.iter().map(|n| n.chars().count()).max().unwrap_or(0) + 1).clamp(KEY, NAME_MAX);
    (whole.iter().map(|n| cut_start(n, width - 1)).collect(), width)
}

// A name cut to `n` characters from its start, `…` in front: its end stays.
fn cut_start(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_owned();
    }
    let keep: String = s.chars().skip(count + 1 - n.max(1)).collect();
    format!("…{keep}")
}

// Its stage as the header's strip words it (`review ↺2`), paused said as its room's row says it.
fn stage(h: &CardHeader) -> Element<'_, Message> {
    let current = h.pipeline.iter().position(|s| s.at == StageAt::Current);
    let step = current.and_then(|i| spans::strip(h).steps.into_iter().nth(i));
    let mut words = vec![step.map_or_else(|| Span::new(h.stage.as_str(), Tone::Text), |s| s.name)];
    if h.paused {
        words.push(Span::new(" paused", Tone::Orange));
    }
    widgets::line(&words).into()
}

fn card<'a>(app: &App, v: &'a PlaceView) -> Element<'a, Message> {
    let Some(h) = &v.card else { return side::empty("no card") };
    let a = &h.about;
    let known = |s: &str| if s.is_empty() { txt("unknown", Tone::Dim) } else { txt(clean(s), Tone::Text) };
    let branch = a.branch.as_deref().map_or_else(|| txt("none yet", Tone::Dim), known);
    let base = match &a.fork {
        Some(fork) if !a.base.is_empty() => txt(format!("{} at {fork}", clean(&a.base)), Tone::Text),
        _ => known(&a.base),
    };
    let pr: Element<'_, Message> = match (a.pr, &a.pr_url) {
        (Some(n), Some(url)) => {
            let link = widgets::centred(vec![
                txt(format!("#{n}"), Tone::Green).into(),
                widgets::gap(6.0),
                icons::icon(Icon::Link, Tone::Green),
            ]);
            clickable("side:pr".into(), link, Some(Message::Link(url.clone())))
        }
        (Some(n), None) => txt(format!("#{n}"), Tone::Text).into(),
        (None, _) => txt("none", Tone::Dim).into(),
    };
    let mut col = Column::new()
        .push(pair("project", known(a.project.as_str()).into()))
        .push(pair("stage", stage(h)))
        .push(pair("branch", branch.into()))
        .push(pair("base", base.into()))
        .push(pair("pr", pr))
        .push(widgets::gap_v(6.0))
        .push(crate::switches::view(app, v))
        .push(widgets::gap_v(6.0));
    if v.sessions.is_empty() {
        return col.push(side::empty("no sessions")).into();
    }
    let (names, width) = names(v);
    for (a, name) in v.sessions.iter().zip(names) {
        col = col.push(session(a, name, width));
    }
    col.into()
}

// A session's name and what it runs, which opens its picker when it has choices; what its next
// turn takes, where that differs, under it.
fn session(a: &AgentRow, name: String, width: usize) -> Element<'_, Message> {
    let mut setting = vec![Span::new(format!("{} · {}", clean(&a.label), clean(&a.effort)), Tone::Text)];
    let setting: Element<'_, Message> = if a.choices.is_empty() {
        widgets::line(&setting).into()
    } else {
        setting.push(Span::new(" ▾", Tone::Dim));
        clickable(format!("side:pick:{}", a.agent), widgets::line(&setting), Some(Message::Picker(a.agent.to_string())))
    };
    let mut col = Column::new().push(row![txt(format!("{name:<width$}"), spans::role(a.role)), setting]);
    if let Some(p) = &a.pending {
        let next = format!("→ {} · {} next turn", clean(&p.model.label), p.effort.as_str());
        col = col.push(row![txt(format!("{:width$}", ""), Tone::Dim), widgets::wrapped(next, Tone::Orange)]);
    }
    col.into()
}

fn context<'a>(app: &App, v: &'a PlaceView) -> Element<'a, Message> {
    let (mut col, (names, width)) = (Column::new(), names(v));
    for (a, name) in v.sessions.iter().zip(&names) {
        let ctx = match a.context {
            Some((used, window)) => spans::context(used, window),
            None => Span::new("ctx unknown", Tone::Dim),
        };
        let mut parts = vec![ctx];
        if let Some(cache) = a.cache {
            parts.push(Span::new(" · ", Tone::Dim));
            parts.push(spans::cache_status(cache, app.now_ms));
        }
        col = col.push(keyed(name, width, widgets::line(&parts).into()));
    }
    if v.sessions.is_empty() {
        col = col.push(side::empty("no sessions running"));
    }
    let (words, tone) = match v.card.as_ref().map(|h| cost(&h.about)) {
        Some((words, tone)) if !words.is_empty() => (words, tone),
        _ => ("none recorded".to_owned(), Tone::Dim),
    };
    col = col.push(widgets::gap_v(6.0)).push(keyed("run cost", width, txt(words, tone).into()));
    if let Some(card) = &v.card {
        col = col.push(seat_costs(&card.about.seat_costs));
    }
    col.into()
}

fn seat_costs(costs: &[SeatCost]) -> Element<'_, Message> {
    let names: Vec<String> = costs.iter().map(|c| cost_name(c.agent.as_str())).collect();
    let width = (names.iter().map(|n| n.chars().count()).max().unwrap_or(0) + 1).clamp(KEY, NAME_MAX);
    let mut col = Column::new();
    for (cost, name) in costs.iter().zip(names) {
        let dollars = spans::dollars(cost.cost_cents);
        let (dollars, tone) =
            if cost.unknown { (format!("{dollars} + ?"), Tone::Orange) } else { (dollars, Tone::Text) };
        col = col
            .push(widgets::gap_v(6.0))
            .push(keyed(&cut_start(&name, width - 1), width, txt(dollars, tone).into()))
            .push(widgets::wrapped(token_counts(cost.tokens), Tone::Dim));
    }
    col.into()
}

fn cost_name(key: &str) -> String {
    let (key, dm) = key.strip_suffix("/dm").map_or((key, false), |k| (k, true));
    let name = key.rsplit('/').next().unwrap_or(key);
    clean(&if dm { format!("dm {name}") } else { name.to_owned() })
}

fn token_counts(tokens: Tokens) -> String {
    format!(
        "in {} · read {} · write {} · out {}",
        token_count(tokens.input),
        token_count(tokens.cache_read),
        token_count(tokens.cache_write),
        token_count(tokens.output),
    )
}

fn token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{}.{:01}M", n / 1_000_000, (n / 100_000) % 10)
    } else if n >= 1000 {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

fn outputs<'a>(app: &'a App, v: &'a PlaceView) -> Element<'a, Message> {
    if v.outputs.is_empty() {
        return side::empty("no files produced yet");
    }
    let rows = v.outputs.iter().enumerate().map(|(i, o)| output(app, i, o));
    Column::with_children(rows).into()
}

// A file's name, as much as fits before its size and age.
fn output<'a>(app: &App, i: usize, o: &'a OutputFile) -> Element<'a, Message> {
    let meta = format!("  {} · {}", size(o.size), crate::time::duration(app.now_ms - o.modified_ms));
    let room = widgets::chars_in(crate::window::SIDE_INNER).saturating_sub(meta.chars().count());
    let line = row![
        txt(widgets::cut(&clean(&o.name), room), Tone::Text),
        iced::widget::space().width(Length::Fill),
        txt(meta, Tone::Dim),
    ]
    .height(Length::Fixed(crate::fonts::LINE));
    let open = Message::Output { job: o.job.to_string(), name: o.name.clone() };
    clickable(format!("output:{i}"), line, Some(open))
}

/// A file's size: `512 B`, `12 KB`, `4.2 MB`.
pub fn size(bytes: u64) -> String {
    const KB: u64 = 1024;
    match bytes {
        0..KB => format!("{bytes} B"),
        KB..1_048_576 => format!("{} KB", bytes.div_ceil(KB)),
        _ => format!("{}.{} MB", bytes / 1_048_576, bytes % 1_048_576 * 10 / 1_048_576),
    }
}
