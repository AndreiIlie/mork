//! A conversation message's words as its `copy` puts them on the clipboard (fac-33): as written,
//! what `more …` folds included, card ids bare.

use mork_types::view::MessageView;

use crate::app::{App, Effect};
use crate::shape::{self, Shape};
use crate::text::clean;

impl App {
    pub(crate) fn copy(&self, seq: u64) -> Vec<Effect> {
        let Some(v) = self.place() else { return Vec::new() };
        let found = v.messages.iter().chain(&v.pinned).find(|m| m.seq == seq);
        found.map(|m| Effect::Copy(text(m))).into_iter().collect()
    }
}

/// Copies the message with its ask options, findings and proofs, including the visible headline
/// when it adds to the message.
pub fn text(m: &MessageView) -> String {
    let words = m.text.trim();
    // A block or a bubble shows its words alone, a hand's headline never; a divider copies its note.
    let shown = !matches!(shape::shape(m), Shape::Block | Shape::Bubble | Shape::Divider) || words.is_empty();
    let head = if shown { m.headline.trim() } else { "" };
    let mut parts: Vec<String> = Vec::new();
    if !head.is_empty() && !words.starts_with(head) {
        parts.push(head.to_owned());
    }
    if !words.is_empty() {
        parts.push(m.hand.as_ref().map_or_else(|| words.to_owned(), |h| h.with_options(words)));
    }
    for f in &m.findings {
        let mut lines = vec![format!("{}:{} {}", f.file, f.line, f.summary.trim())];
        lines.extend(labelled("proof", &f.proof));
        parts.push(lines.join("\n"));
    }
    clean(&parts.join("\n\n"))
}

fn labelled(label: &str, words: &str) -> Option<String> {
    let words = words.trim();
    (!words.is_empty()).then(|| format!("{label}: {words}"))
}
