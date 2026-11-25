//! What each line says, and in which of the palette's colours (5.2): worked out apart from the
//! widgets, so the tests read it without pixels.

use factory_types::Command;
use factory_types::ask::Color as Light;
use factory_types::card::Mode;
use factory_types::reply::Role;
use factory_types::view::{
    AgentLight, AgentRow, Author, CacheState, CardHeader, Doing, Fact, Master, Phase, Resources, Share, StageAt,
    TopLine, Update,
};

use crate::text::clean;
use crate::time;

/// A colour of the palette, by its name (5.2): the theme maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Bright,
    Strong,
    Text,
    Dim,
    Rule,
    Edge,
    Green,
    Orange,
    Red,
    White,
}

/// A run of text in one tone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub tone: Tone,
    pub bold: bool,
}

impl Span {
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self { text: text.into(), tone, bold: false }
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

const GAP: &str = "   ";

/// A card's light, or an ask's colour, as a tone (5.2).
pub fn light(l: Option<Light>) -> Tone {
    match l {
        None => Tone::Rule,
        Some(Light::Grey) => Tone::Dim,
        Some(Light::Green) => Tone::Green,
        Some(Light::Yellow) => Tone::Orange,
        Some(Light::Red) => Tone::Red,
    }
}

/// An agent's square and icon (5.6.1).
pub fn agent_light(l: AgentLight) -> Tone {
    match l {
        AgentLight::Working => Tone::Green,
        AgentLight::Asking => Tone::Orange,
        AgentLight::Error => Tone::Red,
        AgentLight::Waiting => Tone::Dim,
        AgentLight::Idle => Tone::Rule,
    }
}

/// The top line's name and pin.
pub fn top_head(t: &TopLine) -> Vec<Span> {
    vec![Span::new("factory", Tone::Bright).bold(), Span::new(GAP, Tone::Dim), Span::new(pin(t), Tone::Dim)]
}

/// Words of the update after the pin, each with the button that follows it (fac-19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePart {
    pub words: Vec<Span>,
    /// The button's label and the command it sends.
    pub button: Option<(String, Command)>,
}

/// The pin waiting for a reload, and master's tip on its way to the pin (fac-19).
pub fn top_updates(t: &TopLine) -> Vec<UpdatePart> {
    let part = |words: String, tone: Tone, button: Option<(&str, Command)>| UpdatePart {
        words: vec![Span::new(GAP, Tone::Dim), Span::new(words, tone)],
        button: button.map(|(l, c)| (l.to_owned(), c)),
    };
    let mut out = Vec::new();
    if t.pin.ready {
        out.push(part("update ready".into(), Tone::Green, Some(("reload", Command::Reload { target: None }))));
    }
    match &t.pin.update {
        None => {}
        Some(Update::Preparing { short }) => out.push(part(format!("preparing {short}"), Tone::Dim, None)),
        Some(Update::BuildFailed { short, retry }) => {
            out.push(part(format!("{short}'s build failed"), Tone::Red, Some(("retry", retry.clone()))));
        }
        Some(Update::NotPinned { short, retry }) => {
            out.push(part(format!("{short} not pinned"), Tone::Red, Some(("retry", retry.clone()))));
        }
    }
    out
}

/// The top line after the update: the machine's load and the phase.
pub fn top_tail(t: &TopLine) -> Vec<Span> {
    let mut out = Vec::new();
    if let Some(r) = &t.resources {
        out.push(Span::new(GAP, Tone::Dim));
        out.extend(resources(r));
    }
    match &t.phase {
        Phase::Ready => {}
        Phase::Recovering { doing } => {
            out.push(Span::new(GAP, Tone::Dim));
            let doing = clean(doing);
            let words = if doing.is_empty() { "recovering".to_owned() } else { format!("recovering: {doing}") };
            out.push(Span::new(words, Tone::Orange).bold());
        }
        Phase::Reloading { short } => {
            out.push(Span::new(GAP, Tone::Dim));
            out.push(Span::new(format!("reloading to {short}"), Tone::Orange).bold());
        }
    }
    out
}

fn pin(t: &TopLine) -> String {
    let master = match t.pin.master {
        Master::Equal => "= master".to_owned(),
        Master::Ahead { commits } => format!("· master +{commits}"),
        Master::Unknown => "· master ?".to_owned(),
    };
    match &t.pin.pin {
        Some(p) => format!("pin {p} {master}"),
        None => format!("no pin {master}"),
    }
}

/// A window's used figure, projected end and time left, without layout spacing.
pub(crate) fn usage_cells(s: &Share, now_ms: i64) -> [Span; 3] {
    let stale = s.stale || s.reset_ms.is_some_and(|r| r <= now_ms);
    let projected = if !stale && let Some(end) = s.ends_percent {
        let tone = if s.signed_out {
            Tone::Dim
        } else if end >= 100 {
            Tone::Red
        } else {
            Tone::Text
        };
        Span::new(format!("→{end}%"), tone)
    } else {
        Span::new("", Tone::Dim)
    };
    let left = reset_in(s, now_ms).map(|left| time::countdown(left, s.window == WEEK)).unwrap_or_default();
    [share(s, stale), projected, Span::new(left, Tone::Dim)]
}

// The machine's own load: a reading that couldn't be made shows as `?`, never as zero.
fn resources(r: &Resources) -> Vec<Span> {
    let cpu = r.cpu_percent.map_or_else(|| Span::new("?", Tone::Dim), |p| Span::new(format!("{p}%"), Tone::Text));
    let ram = r.ram.map_or_else(
        || Span::new("?", Tone::Dim),
        |m| Span::new(format!("{}/{} GiB", gib(m.used_mib), gib(m.total_mib)), Tone::Text),
    );
    vec![Span::new("CPU ", Tone::Dim), cpu, Span::new(" · RAM ", Tone::Dim), ram]
}

fn gib(mib: u64) -> u64 {
    (mib + 512) / 1024
}

const WEEK: &str = "week";

/// The time left until a window resets, while it has one to run: none for a stale window, one
/// with no reset time, one whose reset has passed, or Fable, which resets with the week.
pub fn reset_in(s: &Share, now_ms: i64) -> Option<i64> {
    s.reset_ms.map(|r| r.saturating_sub(now_ms)).filter(|left| *left > 0 && !s.stale && s.window != "fable")
}

/// Whether usage shows a countdown to the second, which needs a frame each second.
pub fn counts_seconds(shares: &[Share], now_ms: i64) -> bool {
    shares.iter().any(|s| s.window != WEEK && reset_in(s, now_ms).is_some())
}

fn share(s: &Share, stale: bool) -> Span {
    if stale {
        return Span::new("stale", Tone::Dim);
    }
    let Some(p) = s.percent else {
        return if s.reached {
            Span::new("reached", if s.signed_out { Tone::Dim } else { Tone::Red }).bold()
        } else {
            Span::new("?", Tone::Dim)
        };
    };
    let words = format!("{p}%");
    if s.signed_out {
        Span::new(words, Tone::Dim)
    } else if s.reached || p >= 100 {
        Span::new(words, Tone::Red).bold()
    } else if p >= 75 {
        Span::new(words, Tone::Orange)
    } else {
        Span::new(words, Tone::Text)
    }
}

/// Cents as dollars: `$4.20`.
pub fn dollars(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// A step of the stage strip (5.7.2): its name, and its time under it once it has one (fac-56).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: Span,
    pub time: Option<Span>,
}

/// The stage strip: its steps, then what isn't a step's time (waiting on you, paused, the cost)
/// to their right, on the names' line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strip {
    pub steps: Vec<Step>,
    pub aside: Span,
}

/// The card's stage strip; the current step's name is its one bold green span.
pub fn strip(h: &CardHeader) -> Strip {
    let steps = h
        .pipeline
        .iter()
        .map(|s| {
            let words = if s.runs > 1 { format!("{} ↺{}", s.label, s.runs) } else { s.label.clone() };
            let (name, tone) = match s.at {
                StageAt::Current => (Span::new(words, Tone::Green).bold(), Tone::Green),
                StageAt::Passed => (Span::new(words, Tone::Text), Tone::Dim),
                StageAt::ToCome => (Span::new(words, Tone::Rule), Tone::Dim),
            };
            let ms = h.times.stages.iter().find(|(label, _)| *label == s.label).map(|(_, ms)| *ms);
            let time = ms.map(|ms| Span::new(time::duration(ms), tone));
            Step { name, time }
        })
        .collect();
    Strip { steps, aside: Span::new(format!("{GAP}{}", aside(h)), Tone::Dim) }
}

fn aside(h: &CardHeader) -> String {
    let mut parts = vec![format!("{} waiting on you", time::duration(h.times.waiting_on_you))];
    if h.paused || h.times.paused > 0 {
        parts.push(format!("paused {}", time::duration(h.times.paused)));
    }
    if h.about.delegated {
        parts.push(if h.about.self_sustained { "self-sustained" } else { "delegated" }.to_owned());
    }
    if let Some(c) = h.cost_cents {
        parts.push(dollars(c));
    }
    parts.join(", ")
}

/// The facts line, as much as fits in `width` characters: whole facts, dropped from the end.
pub fn facts(facts: &[Fact], width: usize) -> String {
    let mut out = String::new();
    for f in facts {
        let words = fact(f);
        let next = if out.is_empty() { words } else { format!("{out} · {words}") };
        if next.chars().count() > width {
            break;
        }
        out = next;
    }
    out
}

fn fact(f: &Fact) -> String {
    match f {
        Fact::Head { head } => format!("head {head}"),
        Fact::JoinOwed { base, tip, asked: true } => format!("join owed: {base} at {tip}, asked of the foreman"),
        Fact::JoinOwed { base, tip, asked: false } => format!("join owed: {base} at {tip}"),
        Fact::BaseAhead { base, commits } => format!("{base} +{commits} not in head"),
        Fact::Overlap { card, files: 1 } => format!("⋈ {card} (1 file)"),
        Fact::Overlap { card, files } => format!("⋈ {card} ({files} files)"),
        Fact::Mode { mode } => mode_word(*mode).to_owned(),
        Fact::Run { run } => format!("run {run}"),
        Fact::Carries { cards } => {
            format!("carries {}", cards.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(" "))
        }
    }
}

/// A mode's word (section 4).
pub fn mode_word(m: Mode) -> &'static str {
    match m {
        Mode::Full => "full",
        Mode::Light => "light",
        Mode::Report => "report",
        Mode::Review => "review",
    }
}

/// An agent's session's model and effort, and what its next turn takes where that differs (fac-96).
pub fn setting(a: &AgentRow) -> Vec<Span> {
    let mut out = vec![Span::new(format!("  {} · {}", clean(&a.label), clean(&a.effort)), Tone::Dim)];
    if let Some(p) = &a.pending {
        let next = format!(" → {} · {} next turn", clean(&p.model.label), p.effort.as_str());
        out.push(Span::new(next, Tone::Orange));
    }
    out
}

/// An agent's name in its row: a foreman's child by its own (`researcher-2`, fac-94), as two of a
/// role may share a room; every other agent by its role.
pub fn agent_name(a: &AgentRow) -> &str {
    let name = a.agent.as_str().rsplit('/').next().unwrap_or_default();
    let child = name.strip_prefix(a.role.as_str()).is_some_and(|n| n.starts_with('-'));
    if child { name } else { a.role.as_str() }
}

/// What an agent does, as its row says it (5.7.4).
pub fn doing(d: &Doing) -> Span {
    match d {
        Doing::Working { step } => Span::new(first_line(&clean(step)), Tone::Green),
        Doing::WaitingForSlot => Span::new("waiting for a test slot", Tone::Dim),
        Doing::WaitingOnYou => Span::new("waiting on you", Tone::Orange),
        Doing::Error { words } => Span::new(format!("✗ {}", first_line(&clean(words))), Tone::Red),
        Doing::Idle => Span::new("idle", Tone::Dim),
    }
}

/// A session's context used of its window (`ctx 84k/200k`), orange from three quarters, red full.
pub fn context(used: u64, window: u64) -> Span {
    let words = format!("ctx {}/{}", thousands(used), thousands(window));
    let share = used.saturating_mul(100).checked_div(window).unwrap_or(100);
    let tone = match share {
        0..75 => Tone::Text,
        75..100 => Tone::Orange,
        _ => Tone::Red,
    };
    Span::new(words, tone)
}

fn thousands(n: u64) -> String {
    if n >= 1000 { format!("{}k", n / 1000) } else { n.to_string() }
}

const CACHE_MINUTE_MS: i64 = 60_000;
const HAND_COLD_MAX_M: i64 = 10;

fn minutes_left(cold_at_ms: i64, now_ms: i64) -> Option<i64> {
    let left = cold_at_ms.saturating_sub(now_ms);
    (left > 0).then(|| left.saturating_add(CACHE_MINUTE_MS - 1) / CACHE_MINUTE_MS)
}

/// A session's prompt-cache status span for Context (fac-670).
pub fn cache_status(cache: CacheState, now_ms: i64) -> Span {
    match cache {
        CacheState::Running => Span::new("warm", Tone::Green),
        CacheState::Idle { cold_at_ms } => match minutes_left(cold_at_ms, now_ms) {
            Some(m) => Span::new(format!("warm {m}m"), Tone::Green),
            None => Span::new("cold", Tone::Dim),
        },
    }
}

/// Countdown words for Raised Hands while a waiting seat has at most 10 minutes left (fac-670).
pub fn hand_cold(cold_at_ms: i64, now_ms: i64) -> Option<String> {
    let m = minutes_left(cold_at_ms, now_ms).filter(|m| *m <= HAND_COLD_MAX_M)?;
    Some(format!("cold in {m}m"))
}

/// A role's conversation colour (5.2).
pub fn role(r: Role) -> Tone {
    match r {
        Role::Planner => Tone::Green,
        Role::Foreman => Tone::Strong,
        Role::Reviewer | Role::Researcher | Role::Baseline | Role::Echo => Tone::Text,
    }
}

/// An author's colour (5.2).
pub fn author(a: &Author) -> Tone {
    match a {
        Author::Andrei => Tone::Bright,
        Author::Engine => Tone::Dim,
        Author::Orchestrator { .. } | Author::Pa => Tone::Green,
        Author::Agent { role: r, .. } => role(*r),
    }
}

/// An author's name in a message's line (5.2).
pub fn author_name(a: &Author) -> String {
    match a {
        Author::Andrei => "andrei".to_owned(),
        Author::Engine => "engine".to_owned(),
        Author::Orchestrator { project } => format!("{}{}", factory_types::state::ORCHESTRATOR, clean(project)),
        Author::Pa => "pa".to_owned(),
        Author::Agent { label, .. } => clean(label),
    }
}

pub fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").to_owned()
}
