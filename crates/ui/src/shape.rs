//! How each message of the conversation draws (fac-39), worked out apart from the widgets so the
//! tests read it without pixels: its shape, its header, its folded preview and what it folds.

use factory_types::view::{Author, Line, MessageView};

use crate::spans::{self, Span, Tone, first_line};
use crate::text::clean;
use crate::{markdown, time};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// An engine request to an agent: a thin divider, its label and time, its text unfolding under it.
    Divider,
    /// Andrei's words: a bubble on the right.
    Bubble,
    /// Words for Andrei: a header, the words whole under it, a hand's ask, what it folds.
    Block,
    /// The traffic around him: a header and one dim line that unfolds.
    Preview,
}

/// The shape of a message: agent traffic folds; words addressed to people show whole.
pub fn shape(m: &MessageView) -> Shape {
    match m.kind {
        Line::Marker => Shape::Divider,
        Line::Andrei => Shape::Bubble,
        Line::Engine => Shape::Block,
        Line::Agent if m.hand.is_some() || for_people(m.to.as_deref()) => Shape::Block,
        Line::Agent => Shape::Preview,
    }
}

// The labels that post in the place without reaching an agent (3.4.2).
fn for_people(to: Option<&str>) -> bool {
    to.is_none_or(|t| {
        matches!(t, "andrei" | "orchestrator" | "pa" | "room" | "runner" | "factory") || t.starts_with("orchestrator:")
    })
}

/// Whether a click folds and unfolds a preview or a block's hidden details.
pub fn foldable(m: &MessageView) -> bool {
    match shape(m) {
        Shape::Preview => true,
        Shape::Block => folds(m),
        Shape::Divider => !m.text.trim().is_empty(),
        Shape::Bubble => false,
    }
}

/// Whether a block is an agent's message addressed to Andrei, drawn as a panel.
pub fn for_andrei(m: &MessageView) -> bool {
    m.kind == Line::Agent && m.to.as_deref() == Some("andrei")
}

/// A block's words, or its headline when it has none.
pub fn words(m: &MessageView) -> String {
    [m.text.as_str(), m.headline.as_str()].into_iter().map(clean).find(|w| !w.trim().is_empty()).unwrap_or_default()
}

/// What a block folds: findings' proofs, or the engine's cut.
/// A hand's message and ask are never folded.
pub fn folds(m: &MessageView) -> bool {
    m.findings.iter().any(|f| !f.proof.is_empty()) || m.cut
}

/// A preview's one line, its marks gone: an agent's headline or first line, and whether more is folded.
pub fn preview(m: &MessageView) -> (String, bool) {
    let text = clean(&m.text);
    let head = clean(&m.headline);
    let (shown, more) = if head.trim().is_empty() {
        let trimmed = text.trim();
        (first_line(trimmed), trimmed.lines().count() > 1)
    } else {
        (first_line(&head), !text.trim().is_empty() && text.trim() != head.trim())
    };
    (markdown::plain(&shown), more || folds(m))
}

/// A header's words, after the hand's dot: who wrote it, to whom, when. An agent's name is bold
/// in its colour; the engine's is dim.
pub fn header(m: &MessageView, offset_min: i32) -> Vec<Span> {
    let mut out = Vec::new();
    let name = spans::author_name(&m.from);
    match (m.kind, &m.from) {
        (Line::Agent, Author::Agent { .. }) => out.push(Span::new(name, spans::author(&m.from)).bold()),
        _ => out.push(Span::new(name, Tone::Dim)),
    }
    let to = m.to.as_deref().map(clean);
    if let Some(to) = to.filter(|_| m.kind != Line::Engine) {
        out.push(Span::new(format!("→ {to}"), Tone::Dim));
    }
    out.push(Span::new(time::hhmm(m.time_ms, offset_min), Tone::Dim));
    out
}

/// A bubble's header: to whom, its number in main, when.
pub fn bubble_header(m: &MessageView, offset_min: i32) -> Vec<Span> {
    let mut out = Vec::new();
    if let Some(to) = &m.to {
        out.push(Span::new(format!("→ {}", clean(to)), Tone::Dim));
    }
    if let Some(n) = m.number {
        out.push(Span::new(format!("#{n}"), Tone::Dim));
    }
    out.push(Span::new(time::hhmm(m.time_ms, offset_min), Tone::Dim));
    out
}

/// A divider's label, its time.
pub fn divider(m: &MessageView, offset_min: i32) -> (String, String) {
    (first_line(&clean(&m.headline)), time::hhmm(m.time_ms, offset_min))
}

/// A hand's dot: its colour (5.2).
pub fn dot(m: &MessageView) -> Option<Tone> {
    m.hand.as_ref().map(|h| spans::light(Some(h.color)))
}
