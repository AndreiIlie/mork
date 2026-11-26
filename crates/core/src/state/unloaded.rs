//! What the engine leaves in the store at start (fac-336): ended cards' records, read again when a
//! command or a view needs one, and what new ids and dedup need of the rest.

use std::collections::{BTreeMap, BTreeSet};

use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::ask::Ask;
use mork_types::card::{BoardCard, Card};
use mork_types::hash::short;
use mork_types::state::{Agent, Job, Message, Place};
use mork_types::{ActionId, CardId, Command};
use serde_json::Value;

use super::State;
use crate::decide::Event;

/// The stored records the state doesn't hold, as far as new ids and dedup need them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unloaded {
    /// The ended cards whose records the state doesn't hold.
    pub cards: BTreeSet<CardId>,
    /// Each action the state doesn't hold, by its natural key: its id and status, done or dropped.
    pub actions: BTreeMap<String, (ActionId, ActionStatus)>,
    /// The highest action number stored.
    pub action_top: u64,
    /// The highest ask number stored, by `<place>/<kind>`.
    pub asks: BTreeMap<String, u64>,
}

impl Unloaded {
    /// Whether an action the state doesn't hold has this natural key, or one made from it (`<key>@<id>`).
    pub fn key_taken(&self, base: &str) -> bool {
        let made = format!("{base}@");
        self.actions.contains_key(base)
            || self.actions.range(made.clone()..).next().is_some_and(|(k, _)| k.starts_with(&made))
    }

    /// The action the state doesn't hold that a new one with this natural key would be, if `alike` accepts it.
    pub fn alike(&self, base: &str, alike: impl Fn(ActionStatus) -> bool) -> Option<&ActionId> {
        let made = format!("{base}@");
        let made_from = self.actions.range(made.clone()..).take_while(|(k, _)| k.starts_with(&made));
        self.actions
            .get_key_value(base)
            .into_iter()
            .chain(made_from)
            .find(|(_, (_, s))| alike(*s))
            .map(|(_, (id, _))| id)
    }

    /// The actions the state doesn't hold whose natural key begins with `prefix` and whose rest
    /// `rest` accepts.
    pub fn matching(&self, prefix: &str, rest: impl Fn(&str) -> bool) -> Vec<ActionId> {
        let within = self.actions.range(prefix.to_owned()..).take_while(|(k, _)| k.starts_with(prefix));
        within.filter(|(k, _)| rest(&k[prefix.len()..])).map(|(_, (id, _))| id.clone()).collect()
    }
}

/// What deciding an event reads that the state doesn't hold: the engine loads it first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    pub cards: BTreeSet<CardId>,
    pub actions: BTreeSet<ActionId>,
}

impl Wanted {
    fn card(&mut self, state: &State, id: CardId) {
        if state.unloaded.cards.contains(&id) {
            self.cards.insert(id);
        }
    }

    // The notes Mork wrote into a board card's file, which tell them from its description:
    // their key names the file it was in, which a close moves (`dest`) under the same name.
    fn notes(&mut self, state: &State, b: &BoardCard) {
        let name = b.file.rsplit('/').next().unwrap_or(&b.file);
        let prefix = format!("change-card:{}:", b.project);
        let named = |rest: &str| rest.starts_with(&format!("{name}:")) || rest.contains(&format!("/{name}:"));
        self.actions.extend(state.unloaded.matching(&prefix, named));
    }

    // Every word of a command naming an ended card, its id or a record's under it, or a board card.
    fn words(&mut self, state: &State, v: &Value) {
        match v {
            Value::String(s) => {
                for word in [s.as_str(), s.split('/').next().unwrap_or_default()].map(str::to_lowercase) {
                    // As decide reads a card's id: lowercased.
                    self.card(state, CardId::new(word.as_str()));
                    if let Some(b) = state.board.get(&CardId::new(word.as_str())) {
                        self.notes(state, b);
                    }
                }
            }
            Value::Array(vs) => vs.iter().for_each(|v| self.words(state, v)),
            Value::Object(m) => m.values().for_each(|v| self.words(state, v)),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}

/// The ended cards and the records an event names that the state doesn't hold (fac-336): a
/// command's words, a PR's review card, a project's board creates, a live card's board notes.
pub fn wanted(state: &State, event: &Event) -> Wanted {
    let mut w = Wanted::default();
    // Its words are a search's, read from the store on its own connection (fac-371).
    if let Event::Command { cmd: Command::HistorySearch { .. }, .. } = event {
        return w;
    }
    if let Event::Command { cmd, .. } = event {
        w.words(state, &serde_json::to_value(cmd).unwrap_or_default());
        if let Command::Review { project, pr } | Command::ReviewMerge { project, pr } = cmd {
            w.card(state, crate::reviews::cards::card_id(project.as_str(), *pr));
        }
        // The board cards its project's earlier creates wrote: the same request answers the first.
        if let Command::CardCreate { project, .. } = cmd {
            w.actions.extend(state.unloaded.matching(&format!("create-card:{project}:"), |_| true));
        }
        // Our card's PR's review card, ended, which `rereview` reopens (fac-349).
        if let Command::Rereview { card } = cmd
            && let Some(c) = state.cards.get(&CardId::new(card.as_str().to_lowercase()))
            && let Some(pr) = c.pr
        {
            w.card(state, crate::reviews::cards::card_id(c.project.as_str(), pr));
        }
        if let Command::ReviewBranch { project, branch } = cmd {
            w.card(state, crate::reviews::cards::branch_id(project.as_str(), branch));
        }
    }
    if let Event::PrRead { card, .. } = event {
        w.card(state, card.clone());
    }
    if let Event::ProofsCopied { review, .. } = event {
        w.card(state, review.clone());
    }
    if let Event::OpenPrsRead { project, prs } = event {
        for pr in prs {
            w.card(state, crate::reviews::cards::card_id(project.as_str(), pr.pr));
            w.card(state, crate::reviews::cards::branch_id(project.as_str(), &pr.branch));
        }
    }
    if let Event::BoardsRead { cards, .. } = event {
        // A carried card's notes are told apart too: its run announces them (fac-563).
        let carried = |id: &CardId| crate::lifecycle::carried::carrying(state, id, |c| c.stage.is_live()).is_some();
        // A closed run a start loaded only to hold its cards has its history read once something names it.
        let held_only = |id: &CardId| state.unloaded.cards.contains(id);
        let read = |id: &CardId| (state.cards.contains_key(id) && !held_only(id)) || carried(id);
        for b in cards.iter().filter(|b| read(&b.id)) {
            w.notes(state, b);
        }
    }
    w
}

/// The stored actions the state doesn't hold that a dedup compares a new one with: the same
/// request from the same sender (fac-184), the same comment on the same PR (R18).
pub fn alike_stored(state: &State, what: &ActionWhat) -> BTreeSet<ActionId> {
    let (prefix, end) = if let ActionWhat::Door(c) = what {
        (format!("door:{}:", c.from.label()), format!(":{}", short(&c.request())))
    } else if let ActionWhat::Comment { repo, pr, body, .. } = what {
        (format!("comment:{repo}:{pr}:"), format!(":{}", short(body)))
    } else {
        return BTreeSet::new();
    };
    let made_from = |rest: &str| rest.split('@').next().is_some_and(|base| base.ends_with(&end));
    state.unloaded.matching(&prefix, made_from).into_iter().collect()
}

/// The cards whose records deciding on `card` reads: its PR's review card, its branch's, an older
/// build's `review-<card>`, and the card a review card gates.
pub fn companions(card: &Card) -> Vec<CardId> {
    let mut out = vec![crate::reviews::gate::id_of(card), CardId::new(format!("review-{}", card.id))];
    out.extend(card.pr.map(|pr| crate::reviews::cards::card_id(card.project.as_str(), pr)));
    out.extend(crate::reviews::gate::subject_of(card).cloned());
    out
}

/// An ended card's records, read from the store.
#[derive(Debug, Clone, Default)]
pub struct Records {
    pub card: Option<Card>,
    pub agents: Vec<Agent>,
    pub places: Vec<Place>,
    pub asks: Vec<Ask>,
    pub jobs: Vec<Job>,
    pub actions: Vec<Action>,
    pub messages: Vec<Message>,
}

impl State {
    /// Takes an ended card's records in, so what decides on it sees them whole. A record the state
    /// holds already is its newer copy, and stays.
    pub fn absorb(&mut self, id: &CardId, r: Records) {
        if !self.unloaded.cards.remove(id) {
            return;
        }
        if let Some(c) = r.card {
            self.cards.entry(c.id.clone()).or_insert(c);
        }
        for a in r.agents {
            self.agents.entry(a.key.clone()).or_insert(a);
        }
        for p in r.places {
            self.places.entry(p.id.clone()).or_insert(p);
        }
        for a in r.asks {
            self.asks.entry(a.id.clone()).or_insert(a);
        }
        for j in r.jobs {
            self.jobs.entry(j.id.clone()).or_insert(j);
        }
        self.take_actions(r.actions);
        if !r.messages.is_empty() {
            self.messages.extend(r.messages);
            self.messages.sort_by_key(|m| m.seq);
            self.messages.dedup_by_key(|m| m.seq);
        }
    }

    /// Takes in actions read from the store; one the state holds already stays.
    pub fn take_actions(&mut self, actions: Vec<Action>) {
        for a in actions {
            self.unloaded.actions.remove(&a.key);
            self.actions.entry(a.id.clone()).or_insert(a);
        }
    }
}
