//! `history.search` (fac-371): cards' posted messages and records matched as `memory.search` matches,
//! each hit marked only as the records mark it: a decision standing or withdrawn, a delegated answer.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use mork_types::action::Action;
use mork_types::card::Card;
use mork_types::decisions;
use mork_types::state::{Message, Sender};
use mork_types::{CardId, ProjectId};
use serde_json::{Value, json};

use crate::board::notes::Written;
use crate::board::writes::name;
use crate::search_text::{excerpt, fold};
use crate::state::State;

const HITS_MAX: usize = 20;

/// One record the store reads for a search.
pub enum Found {
    /// A message posted in one of the card's places.
    Message { card: String, message: Box<Message> },
    /// A card's own record: its title, its text and its dated notes.
    Card(Box<Card>),
    /// One of the card's journal events.
    Event { card: String, time: i64, kind: String, data: Value },
}

/// Whose history a sender searches: a seat its card's project's, anyone else every project's (`None`).
pub fn scope(state: &State, from: &Sender) -> Result<Option<ProjectId>, String> {
    let Sender::Agent(key) = from else { return Ok(None) };
    state
        .agents
        .get(key)
        .and_then(|a| crate::messages::places::card_of(state, &a.place))
        .and_then(|c| state.cards.get(&c))
        .map(|c| Some(c.project.clone()))
        .ok_or_else(|| format!("{key} is no card's seat, so it has no project's history to search"))
}

/// A card's file as the engine last read its board: what `card.show` numbers its notes by, later
/// writes to an ended card included.
pub struct OnBoard {
    project: ProjectId,
    file: String,
    description: String,
}

/// The board cards of `project` (every project's with none), taken from the state for a search.
pub fn on_boards(state: &State, project: Option<&ProjectId>) -> BTreeMap<CardId, OnBoard> {
    let theirs = state.board.values().filter(|b| project.is_none_or(|p| *p == b.project));
    let one = |b: &mork_types::card::BoardCard| OnBoard {
        project: b.project.clone(),
        file: b.file.clone(),
        description: b.description.clone(),
    };
    theirs.map(|b| (b.id.clone(), one(b))).collect()
}

struct Hit {
    rank: (usize, i64),
    text: String,
    about: Value,
}

/// A search under way: the best hits so far, the most words matched first, then the newest.
pub struct Search {
    words: Vec<String>,
    hits: Vec<Hit>,
    boards: BTreeMap<CardId, OnBoard>,
    written: BTreeMap<(ProjectId, String), Vec<Written>>,
}

impl Search {
    /// A search for any of these words, a card's notes read from `boards` and told apart by the
    /// recorded board writes `writes`.
    pub fn new(words: &str, boards: BTreeMap<CardId, OnBoard>, writes: &[Action]) -> Self {
        let mut words: Vec<String> = words.split_whitespace().map(fold).collect();
        words.sort_unstable();
        words.dedup();
        // Filed by file name, as `board::writes::written` finds them, so a note is numbered as `card.show` numbers it.
        let mut written: BTreeMap<(ProjectId, String), Vec<Written>> = BTreeMap::new();
        for (project, w) in writes.iter().filter_map(crate::board::writes::written_note) {
            let names = [name(&w.file), name(&w.dest)].map(str::to_owned);
            for at in names.iter().filter(|n| !n.is_empty()).collect::<std::collections::BTreeSet<_>>() {
                written.entry((project.clone(), at.clone())).or_default().push(w.clone());
            }
        }
        Self { words, hits: Vec::new(), boards, written }
    }

    /// Weighs one record the store read.
    pub fn offer(&mut self, found: Found) {
        match found {
            Found::Message { card, message: m } => {
                let ask = m.hand.as_ref().map(|h| h.ask_words()).unwrap_or_default();
                let mut text = [m.headline.as_str(), m.text.as_str(), ask.as_str()].join("\n");
                for f in &m.findings {
                    let _ = write!(text, "\n{}:{}: {}\n{}", f.file, f.line, f.summary, f.proof);
                }
                let marked = m.delegated.as_ref().map(|a| format!("delegated answer to {}", a.as_str()));
                self.consider(m.time, text, || {
                    json!({"card": card, "source": "message", "place": m.place, "seq": m.seq, "time": m.time,
                           "author": m.from.label(), "marked": marked})
                });
            }
            Found::Card(c) => self.card(&c),
            Found::Event { card, time, kind, data } => {
                let mut text = kind.clone();
                words_of(&data, &mut text);
                self.consider(time, text, || {
                    json!({"card": card, "source": "journal", "kind": kind, "time": time, "author": "engine", "marked": null})
                });
            }
        }
    }

    // A card's text and each of its notes, which carry no time: what was settled ranks before the
    // chat about it at as many words.
    fn card(&mut self, c: &Card) {
        let (notes, body) = self.notes(c);
        let gone = decisions::withdrawn(&notes);
        let total = notes.len();
        for (i, note) in notes.iter().enumerate() {
            let marked = decisions::decision_words(note).map(|_| {
                if gone.get(i).copied().unwrap_or(false) { "withdrawn decision" } else { "standing decision" }
            });
            let author = note.split_once(' ').map(|(who, _)| who.to_owned());
            self.consider(i64::MAX, note.clone(), || {
                json!({"card": c.id, "source": "note", "number": decisions::number(total, i), "time": null,
                       "author": author, "marked": marked})
            });
        }
        self.consider(
            i64::MAX,
            format!("{}\n{body}", c.title),
            || json!({"card": c.id, "source": "card", "time": null, "author": null, "marked": null}),
        );
    }

    // Its notes and what is under them, as `card.show` tells them apart for a card on the board.
    fn notes(&self, c: &Card) -> (Vec<String>, String) {
        let board = self.boards.get(&c.id);
        let written = board.and_then(|b| self.written.get(&(b.project.clone(), name(&b.file).to_owned())));
        let lines: Vec<&str> = board.map_or(&c.description, |b| &b.description).lines().collect();
        let n = crate::board::notes::split(&lines, written.map_or(&[], Vec::as_slice));
        (n.notes, lines.get(n.end..).unwrap_or_default().join("\n"))
    }

    fn consider(&mut self, time: i64, text: String, about: impl FnOnce() -> Value) {
        let hay = fold(&text);
        let matched = self.words.iter().filter(|w| hay.contains(w.as_str())).count();
        let rank = (matched, time);
        let full = self.hits.len() >= HITS_MAX;
        if matched == 0 || (full && self.hits.last().is_some_and(|h| h.rank >= rank)) {
            return;
        }
        let at = self.hits.partition_point(|h| h.rank >= rank);
        self.hits.insert(at, Hit { rank, text, about: about() });
        self.hits.truncate(HITS_MAX);
    }

    /// The answer: the best hits, each with its excerpt.
    pub fn answer(self) -> Value {
        let words = self.words;
        let hits: Vec<Value> = self
            .hits
            .into_iter()
            .map(|mut h| {
                h.about["excerpt"] = json!(excerpt(&h.text, &words));
                h.about
            })
            .collect();
        json!({"history.search": {"hits": hits}})
    }
}

fn words_of(v: &Value, out: &mut String) {
    match v {
        Value::String(s) => {
            out.push(' ');
            out.push_str(s);
        }
        Value::Array(vs) => vs.iter().for_each(|v| words_of(v, out)),
        Value::Object(m) => m.values().for_each(|v| words_of(v, out)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
