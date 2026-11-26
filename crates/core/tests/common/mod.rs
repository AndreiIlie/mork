// fac-653: the engine's part, played for `core` alone (1.12.1): a state, `decide`, and the answers a test gives
// for the jobs and actions `core` asks for.

// why: test code: a failed unwrap is the test failing; each test binary uses part of this.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cards;
mod jobs;
mod train;

use std::collections::BTreeMap;

use mork_core::decide::{Ctx, Decision, Event, Intent, decide};
use mork_types::Command;
use mork_types::action::{Action, ActionWhat};
use mork_types::card::{BoardCard, Card, Stage};
use mork_types::config::Config;
use mork_types::protocol::ErrorBody;
use mork_types::state::{Message, Sender};
use mork_types::{CardId, Sha};
use serde_json::{Value, json};

/// Mork's home, as the jobs' paths name it.
pub const HOME: &str = "/f";

/// A commit hash made from a short name, so tests read `sha("h1")`.
pub fn sha(name: &str) -> Sha {
    let hex: String = name.bytes().map(|b| format!("{b:02x}")).collect();
    Sha::new(format!("{hex:0<40}"))
}

/// The test config: a local project `web` and a Bitbucket project `ex`, with checks and a test command, neither the
/// mork's own project, whose suite would owe its wall tests a second run.
pub fn config() -> Config {
    let c = json!({
        "mork": {"project": "mork"},
        "git": {"author": "Mork Test <t@example.invalid>"},
        "projects": {
            "web": {"repo": "/r/web", "board": {"kind": "markdown", "path": "/b/web"}, "test": "npm test",
                    "checks": {"command": "checks", "folder": "/c/web"}},
            "ex": {"repo": "/r/ex", "board": {"kind": "markdown", "path": "/b/ex"}, "test": "npm test",
                   "forge": {"kind": "bitbucket", "workspace": "ws", "slug": "ex"},
                   "checks": {"command": "checks", "folder": "/c/ex"}}
        }
    });
    serde_json::from_value(c).unwrap()
}

/// A whole reply of a role, with its outcome and its hand's headline.
pub fn reply(outcome: &str, headline: &str) -> Value {
    json!({
        "hand": {"color": "grey", "headline": headline, "message": "A message.", "ask": ""},
        "messages": [], "findings": [], "risks": [],
        "outcome": outcome, "schema": 6
    })
}

/// What head `head` makes of the files it changes: each file's content its own at every head (fac-95).
pub fn contents(head: &str, files: &[&str]) -> mork_types::review::Contents {
    files.iter().map(|f| ((*f).to_owned(), format!("100644 {}", sha(&format!("{head}:{f}"))))).collect()
}

/// The simulated engine.
pub struct Sim {
    pub state: mork_core::state::State,
    pub config: Config,
    pub now: i64,
    /// Intents not yet carried out.
    pub intents: Vec<Intent>,
    /// Journal rows emitted by `decide`, in order, for timeline-backed view tests.
    pub journal: Vec<(i64, String, Option<String>, Value)>,
    /// The base's tip as the mirror has it: what a fetch and a take read.
    pub tip: &'static str,
    next: u32,
}

impl Sim {
    pub fn new() -> Self {
        Self {
            state: mork_core::state::State::default(),
            config: config(),
            now: 1_792_000_000_000,
            intents: Vec::new(),
            journal: Vec::new(),
            tip: "base1",
            next: 0,
        }
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx { now: self.now, config: &self.config, faults: false }
    }

    /// Decides one event and commits its changes, a second later than the last.
    pub fn apply(&mut self, e: Event) -> Decision {
        self.now += 1000;
        let d = decide(&self.state, &e, self.ctx());
        self.state.commit(&d.changes);
        for j in &d.journal {
            self.journal.push((self.now, j.kind.clone(), j.card.clone(), j.data.clone()));
        }
        self.intents.extend(d.intents.iter().cloned());
        d
    }

    /// One command, from a sender, as JSON (`{"start": {"card": "web-1"}}`).
    pub fn cmd(&mut self, from: Sender, cmd: Value) -> Result<Value, ErrorBody> {
        let cmd: Command = serde_json::from_value(cmd.clone()).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        self.apply(Event::Command { from, cmd }).answer.unwrap_or(Ok(Value::Null))
    }

    pub fn operator(&mut self, cmd: Value) -> Result<Value, ErrorBody> {
        self.cmd(Sender::Operator, cmd)
    }

    /// Puts a card on its project's board, `Backlog`, as the engine reads it.
    pub fn board(&mut self, id: &str, project: &str) {
        let title = format!("The title of {id}");
        let description = format!("The description of {id}.");
        let text = mork_core::board::new_card(&id.to_uppercase(), &title, &description, "2026-11-21 10:00")
            .map(|(_, t)| t)
            .unwrap()
            .replace("status: To Do", "status: Backlog");
        let card = BoardCard {
            id: CardId::new(id),
            project: project.into(),
            title,
            description,
            acceptance: vec![format!("{id} is accepted")],
            status: "Backlog".into(),
            labels: Vec::new(),
            done_status: "Done".into(),
            file: format!("tasks/{id} - title.md"),
            text,
            prefix: project.to_owned(),
        };
        self.apply(Event::BoardsRead { cards: vec![card], prefixes: BTreeMap::new() });
    }

    pub fn card(&self, id: &str) -> Card {
        self.state.cards.get(&CardId::new(id)).cloned().unwrap_or_else(|| panic!("no card {id}"))
    }

    pub fn stage(&self, id: &str) -> Stage {
        self.card(id).stage
    }

    pub fn messages(&self, place: &str) -> Vec<Message> {
        self.state.messages_in(place).cloned().collect()
    }

    /// A place's requests from the engine to `to`.
    pub fn engine_to(&self, place: &str, to: &str) -> Vec<Message> {
        let to_them = |m: &Message| m.from == Sender::Engine && m.to.as_deref() == Some(to);
        self.messages(place).into_iter().filter(to_them).collect()
    }

    pub fn actions(&self) -> Vec<Action> {
        self.state.actions.values().cloned().collect()
    }

    /// Whether any action of the card's state matches `what`, whatever its status.
    pub fn has_action(&self, what: impl Fn(&ActionWhat) -> bool) -> bool {
        self.state.actions.values().any(|a| what(&a.what))
    }

    /// Whether an agent is owed a turn now.
    pub fn owed(&self, key: &str) -> bool {
        mork_core::agents::turns::due(&self.state, &self.config, self.now).iter().any(|k| k.as_str() == key)
    }
}
