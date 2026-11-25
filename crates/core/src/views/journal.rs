use std::collections::BTreeSet;

use factory_types::ProjectId;
use factory_types::state::{Message, Sender};
use serde::Serialize;
use serde_json::{Value, json};

use crate::state::State;

const BUDGET: usize = 600 * 1024;
const TEXT_MAX: usize = 64 * 1024;
const WORDS_MAX: usize = 4 * 1024;

/// Whose journal the sender reaches, with no project limit for the factory's own card seats.
pub fn scope(state: &State, from: &Sender, factory: &ProjectId) -> Result<Option<ProjectId>, String> {
    super::search::scope(state, from).map(|p| p.filter(|p| p != factory))
}

/// Whether a target project is within the sender's journal reach.
pub fn reaches(scope: Option<&ProjectId>, target: &ProjectId) -> bool {
    scope.is_none_or(|p| p == target)
}

/// The journal position an event selector names.
pub enum Anchor {
    Seq(i64),
    At(i64),
    Before(i64),
}

/// A validated, bounded journal request.
pub struct Read {
    pub card: String,
    pub count: u64,
    pub anchor: Anchor,
    pub kinds: Vec<String>,
}

impl Read {
    /// Accepts at most one selector, none meaning the newest rows, and a positive count; card ids follow the
    /// other card reads.
    pub fn new(
        card: &str,
        count: u64,
        seq: Option<u64>,
        at: Option<u64>,
        before: Option<u64>,
        kinds: Vec<String>,
    ) -> Result<Self, String> {
        let (selector, anchor): (Option<u64>, fn(i64) -> Anchor) = match (seq, at, before) {
            (Some(n), None, None) => (Some(n), Anchor::Seq),
            (None, Some(n), None) => (Some(n), Anchor::At),
            (None, None, Some(n)) => (Some(n), Anchor::Before),
            (None, None, None) => (None, Anchor::Before),
            _ => return Err("name at most one of `seq`, `at` or `before`".into()),
        };
        if count == 0 {
            return Err("`count` must be greater than zero".into());
        }
        // With no selector, every stored seq is below it, so the page is the newest rows.
        let n = match selector {
            Some(n) => i64::try_from(n).map_err(|_| "the selector exceeds the journal's integer range")?,
            None => i64::MAX,
        };
        Ok(Self { card: card.to_lowercase(), count, anchor: anchor(n), kinds })
    }

    /// Whether this row's kind was requested, or no kinds were specified.
    pub fn includes(&self, kind: &str) -> bool {
        self.kinds.is_empty() || self.kinds.iter().any(|k| k == kind)
    }
}

fn seat(card: &str, name: &str) -> bool {
    name.strip_prefix(card).and_then(|s| s.strip_prefix('/')).is_some_and(|s| !s.is_empty())
}

/// A cardless row belongs only through a named seat, place or exact job id, never through its text.
pub fn cardless(card: &str, jobs: &BTreeSet<String>, data: &Value) -> bool {
    let named = ["agent", "from", "to", "place"].into_iter().any(|field| {
        data.get(field).and_then(Value::as_str).is_some_and(|s| seat(card, s) || field == "place" && s == card)
    });
    named
        || data.pointer("/from/agent").and_then(Value::as_str).is_some_and(|s| seat(card, s))
        || data.get("job").and_then(Value::as_str).is_some_and(|j| jobs.contains(j))
}

/// A stored event, with the message or hand its data points at when one is retained.
#[derive(Serialize)]
pub struct Row {
    pub seq: i64,
    pub time: i64,
    pub kind: String,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Value>,
}

/// The stored message with history's text limits and an explicit truncation mark.
pub fn message(m: Message) -> Value {
    let mut v = serde_json::to_value(m).unwrap_or_default();
    let mut cut = false;
    for (path, max) in [
        ("/text", TEXT_MAX),
        ("/headline", WORDS_MAX),
        ("/hand/message", TEXT_MAX),
        ("/hand/headline", WORDS_MAX),
        ("/hand/ask", WORDS_MAX),
    ] {
        if let Some(Value::String(s)) = v.pointer_mut(path) {
            cut |= trim(s, max);
        }
    }
    if let Some(fs) = v.get_mut("findings").and_then(Value::as_array_mut) {
        for f in fs {
            if let Some(Value::String(s)) = f.get_mut("proof") {
                cut |= trim(s, WORDS_MAX);
            }
        }
    }
    v["cut"] = json!(cut);
    v
}

fn trim(s: &mut String, max: usize) -> bool {
    if s.len() <= max {
        return false;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push('…');
    true
}

#[derive(Default)]
struct Side {
    seen: u64,
    bytes: usize,
    full: bool,
    rows: Vec<Row>,
}

/// A page collecting each side nearest first, without retaining a card's whole journal.
pub struct Page {
    card: String,
    pivot: i64,
    limits: [u64; 2],
    sides: [Side; 2],
    oversized: Option<i64>,
}

impl Page {
    /// Splits an event's count before and from its pivot, or keeps a backward page alone.
    pub fn new(read: &Read, pivot: i64) -> Self {
        let before = matches!(read.anchor, Anchor::Before(_));
        let left = if before { read.count } else { read.count / 2 };
        Self {
            card: read.card.clone(),
            pivot,
            limits: [left, if before { 0 } else { read.count - left }],
            sides: Default::default(),
            oversized: None,
        }
    }

    /// Counts a matching row and says whether its payload is still needed for this side.
    pub fn wants(&mut self, side: usize) -> bool {
        let s = &mut self.sides[side];
        s.seen += 1;
        !s.full && s.seen <= self.limits[side]
    }

    /// Retains a nearest row while it fits; an indivisible oversized row is reported at the end.
    pub fn offer(&mut self, side: usize, row: Row) {
        let size = serde_json::to_vec(&row).map_or(BUDGET + 1, |v| v.len());
        let s = &mut self.sides[side];
        if size > BUDGET && s.rows.is_empty() {
            self.oversized = Some(row.seq);
        }
        s.full = s.bytes.saturating_add(size) > BUDGET;
        if !s.full {
            s.bytes += size;
            s.rows.push(row);
        }
    }

    /// Gives the rows oldest first, with exact omitted counts, keeping those nearest the pivot.
    pub fn answer(mut self) -> Result<Value, String> {
        if let Some(seq) = self.oversized {
            return Err(format!("journal row {seq} cannot fit in one history page"));
        }
        let mut size = serde_json::to_vec(&self.value()).map_or(usize::MAX, |v| v.len());
        while size > BUDGET {
            if self.sides.iter().map(|s| s.rows.len()).sum::<usize>() == 1 {
                return Err("the journal row cannot fit in one history answer".into());
            }
            let distances = self.sides.each_ref().map(|s| s.rows.last().map(|r| r.seq.abs_diff(self.pivot)));
            let side = if distances[0] >= distances[1] { 0 } else { 1 };
            let Some(row) = self.sides[side].rows.pop() else {
                return Err("the journal page cannot fit in one history answer".into());
            };
            size = size.saturating_sub(serde_json::to_vec(&row).map_or(0, |v| v.len() + 1));
            if size <= BUDGET {
                size = serde_json::to_vec(&self.value()).map_or(usize::MAX, |v| v.len());
            }
        }
        Ok(self.value())
    }

    fn value(&self) -> Value {
        let [left, right] = &self.sides;
        let rows: Vec<&Row> = left.rows.iter().rev().chain(right.rows.iter()).collect();
        json!({"history.journal": {"card": self.card, "rows": rows,
            "earlier": left.seen - left.rows.len() as u64, "later": right.seen - right.rows.len() as u64}})
    }
}

#[cfg(test)]
mod tests {
    use super::{Anchor, Read};

    #[test]
    fn no_selector_reads_the_newest_rows() {
        let read = Read::new("FAC-1", 5, None, None, None, Vec::new());
        assert!(matches!(read.map(|r| r.anchor), Ok(Anchor::Before(i64::MAX))));
    }

    #[test]
    fn two_selectors_are_refused() {
        let read = Read::new("fac-1", 5, Some(1), None, Some(2), Vec::new());
        assert_eq!(read.err().as_deref(), Some("name at most one of `seq`, `at` or `before`"));
    }
}
