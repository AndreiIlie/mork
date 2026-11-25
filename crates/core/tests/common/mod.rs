// fac-653: the engine's part, played for `core` alone (1.12.1): a state, `decide`, and the answers a test gives
// for the jobs and actions `core` asks for.

// why: test code: a failed unwrap is the test failing; each test binary uses part of this.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cards;
mod jobs;
mod train;

use std::collections::BTreeMap;

use factory_core::decide::{Ctx, Decision, Event, Intent, decide};
use factory_types::Command;
use factory_types::action::{Action, ActionWhat};
use factory_types::card::{BoardCard, Card, Stage};
use factory_types::config::Config;
use factory_types::protocol::ErrorBody;
use factory_types::state::{Message, Sender};
use factory_types::{CardId, Sha};
use serde_json::{Value, json};

/// The factory's home, as the jobs' paths name it.
pub const HOME: &str = "/f";

/// A commit hash made from a short name, so tests read `sha("h1")`.
pub fn sha(name: &str) -> Sha {
    let hex: String = name.bytes().map(|b| format!("{b:02x}")).collect();
    Sha::new(format!("{hex:0<40}"))
}

/// The test config: a local project `web` and a Bitbucket project `hf`, with checks and a test command, neither the
/// factory's own project, whose suite would owe its wall tests a second run.
pub fn config() -> Config {
    let c = json!({
        "factory": {"project": "factory"},
        "git": {"author": "Factory Test <t@example.invalid>"},
        "projects": {
            "web": {"repo": "/r/web", "board": "/b/web", "test": "npm test",
                    "checks": {"command": "checks", "folder": "/c/web"}},
            "hf": {"repo": "/r/hf", "board": "/b/hf", "test": "npm test",
                   "bitbucket": {"workspace": "ws", "slug": "hf"},
                   "checks": {"command": "checks", "folder": "/c/hf"}}
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
pub fn contents(head: &str, files: &[&str]) -> factory_types::review::Contents {
    files.iter().map(|f| ((*f).to_owned(), format!("100644 {}", sha(&format!("{head}:{f}"))))).collect()
}

/// The simulated engine.
pub struct Sim {
    pub state: factory_core::state::State,
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
            state: factory_core::state::State::default(),
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

    pub fn andrei(&mut self, cmd: Value) -> Result<Value, ErrorBody> {
        self.cmd(Sender::Andrei, cmd)
    }

    /// Puts a card on its project's board, `Backlog`, as the engine reads it.
    pub fn board(&mut self, id: &str, project: &str) {
        let title = format!("The title of {id}");
        let description = format!("The description of {id}.");
        let text = factory_core::board::new_card(&id.to_uppercase(), &title, &description, "2026-11-21 10:00")
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
        factory_core::agents::turns::due(&self.state, &self.config, self.now).iter().any(|k| k.as_str() == key)
    }
}
