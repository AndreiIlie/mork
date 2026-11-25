// A card's record (2.9.1), `records/<project>/<card>.md`, is rewritten from the state whenever the card changes;
// written for reading, never read back.

use std::fmt::Write as _;

use factory_types::CardId;
use factory_types::state::{Message, Sender};

use super::Engine;
use crate::store::JournalRow;

fn when(ms: i64) -> String {
    let (y, mo, d, h, mi, s) = factory_core::state::names::civil_from_ms(ms);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

fn of_card(m: &Message, card: &str) -> bool {
    let place_of = m.place.split('/').next() == Some(card);
    let from_agent = matches!(&m.from, Sender::Agent(k) if k.as_str().split('/').next() == Some(card));
    place_of || from_agent
}

impl Engine {
    pub(super) fn write_record(&self, id: &CardId) {
        let Some(card) = self.state.cards.get(id) else { return };
        let events = self.store.card_events(id.as_str()).unwrap_or_default();
        let starts: Vec<(i64, u64)> = events
            .iter()
            .filter(|(_, _, k, _)| k == "card.start")
            .map(|(t, _, _, d)| (*t, d.get("run").and_then(serde_json::Value::as_u64).unwrap_or(1)))
            .collect();
        let mut text = format!(
            "# {} {}\n\nProject {}. Mode {}.\n",
            id.as_str().to_uppercase(),
            card.title,
            card.project,
            factory_core::lifecycle::facts::mode_word(card.mode)
        );
        for (i, (from, run)) in starts.iter().enumerate() {
            let to = starts.get(i + 1).map_or(i64::MAX, |(t, _)| *t);
            self.run_section(&mut text, id.as_str(), *run, *from, to, &events);
        }
        if starts.is_empty() {
            self.run_section(&mut text, id.as_str(), u64::from(card.run), 0, i64::MAX, &events);
        }
        let dir = self.opts.home.records().join(card.project.as_str());
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = crate::files::write_whole(&dir.join(format!("{id}.md")), text.as_bytes());
        }
    }

    fn run_section(&self, text: &mut String, card: &str, run: u64, from: i64, to: i64, events: &[JournalRow]) {
        let _ = write!(text, "\n## Run {run}, from {}\n", when(from));
        let in_run: Vec<&JournalRow> = events.iter().filter(|(t, _, _, _)| *t >= from && *t < to).collect();
        if let Some((_, _, _, d)) = in_run.iter().rev().find(|(_, _, k, _)| k == "brief.approved") {
            let _ = write!(
                text,
                "\n### The brief\n\n{}\n",
                d.get("brief").and_then(serde_json::Value::as_str).unwrap_or("")
            );
        }
        if let Some((t, _, _, d)) = in_run.iter().find(|(_, _, k, _)| k == "card.closed") {
            let _ = write!(
                text,
                "\nClosed {}: {}\n",
                when(*t),
                d.get("why").and_then(serde_json::Value::as_str).unwrap_or("")
            );
        }
        text.push_str(&timeline_text(&in_run));
        text.push_str("\n### Events\n\n");
        for (t, _, k, d) in in_run.iter().filter(|(_, _, k, _)| k != "message") {
            let _ = writeln!(text, "- {} {k} {d}", when(*t));
        }
        text.push_str("\n### Messages\n");
        for m in self.state.messages.iter().filter(|m| m.time >= from && m.time < to && of_card(m, card)) {
            let to_whom = m.to.as_deref().map(|t| format!(" → {t}")).unwrap_or_default();
            let _ = write!(
                text,
                "\n**{} {} in {}{to_whom}** ({:?}) {}\n\n{}\n",
                when(m.time),
                m.from.label(),
                m.place,
                m.kind,
                m.headline,
                m.text
            );
            text.push_str(&whole_hand(m));
        }
    }
}

fn whole_hand(m: &Message) -> String {
    let mut out = String::new();
    if let Some(h) = m.hand.as_ref() {
        let word = |v: serde_json::Value| v.as_str().map(str::to_owned).unwrap_or_default();
        let color = word(serde_json::json!(h.color));
        let _ = write!(out, "\nA {color} hand.\n");
        if !h.ask.is_empty() {
            let _ = write!(out, "\nAsks: {}\n", h.ask_words());
        }
    }
    for f in &m.findings {
        let _ = write!(out, "\n- {}:{}: {}\n  Proof: {}\n", f.file, f.line, f.summary, f.proof);
    }
    out
}

/// A run's timeline from its journal (4.19.1, M3.30): the time in each stage, waiting on you,
/// paused, its lifetime, and its cost.
pub(super) fn timeline_text(rows: &[&JournalRow]) -> String {
    let marks: Vec<(i64, &str, &serde_json::Value)> = rows.iter().map(|(t, _, k, d)| (*t, k.as_str(), d)).collect();
    let (points, cost) = factory_core::views::timeline::points(&marks);
    let t = factory_core::views::timeline::totals(&points, crate::clock::now_ms());
    let min = |ms: i64| format!("{}m", ms / 60_000);
    let stages: Vec<String> = t.stages.iter().map(|(s, ms)| format!("{s} {}", min(*ms))).collect();
    // why: millionths of a dollar shown as dollars; far below f64's exact range.
    #[allow(clippy::cast_precision_loss)]
    let dollars = cost as f64 / 1_000_000.0;
    format!(
        "\n### Timeline\n\n{}; waiting on you {}; paused {}; {} in all; cost ${dollars:.2}\n",
        stages.join(", "),
        min(t.you),
        min(t.paused),
        min(t.lifetime)
    )
}
