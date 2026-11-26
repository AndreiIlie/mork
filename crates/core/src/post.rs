//! One decision, built: messages posted in order, agents woken, cards, asks and actions changed, intents and journal
//! lines collected; each record changed once, at the end, in one transaction (2.4).

mod actions;
mod agent;
mod live;

pub use agent::new_agent;

use std::collections::BTreeMap;

use mork_types::action::Action;
use mork_types::ask::{Ask, AskKind, Color};
use mork_types::card::Card;
use mork_types::state::{Agent, AgentStatus, Message, MessageKind, Owed, Place, Sender};
use mork_types::{AgentKey, AskId, CardId};
use serde_json::json;

use crate::decide::{Decision, Intent, Journal};
use crate::messages::places::Who;
use crate::state::{Change, State};

/// What one decision posts and wakes.
pub struct Poster<'a> {
    pub state: &'a State,
    pub now: i64,
    pub(crate) next_seq: u64,
    messages: Vec<Message>,
    places: BTreeMap<mork_types::PlaceId, Place>,
    agents: BTreeMap<AgentKey, Agent>,
    asks: BTreeMap<mork_types::AskId, Ask>,
    cards: BTreeMap<CardId, Card>,
    actions: BTreeMap<mork_types::ActionId, Action>,
    pub intents: Vec<Intent>,
    pub journal: Vec<Journal>,
    removed: Vec<AgentKey>,
    /// Other changes, in order (leases, notes, owed work).
    pub other: Vec<Change>,
    /// Whether a config allows desktop notices.
    pub notices: bool,
    pub(crate) token_forge: &'static str,
    /// The job this decision settles: a stop decided in it leaves that one alone.
    pub ending: Option<mork_types::JobId>,
    /// A merge train is being kept true (fac-68): what it does keeps none again.
    pub train: bool,
}

impl<'a> Poster<'a> {
    pub fn new(state: &'a State, now: i64) -> Self {
        Self {
            state,
            now,
            next_seq: state.next_seq,
            messages: Vec::new(),
            places: BTreeMap::new(),
            agents: BTreeMap::new(),
            asks: BTreeMap::new(),
            cards: BTreeMap::new(),
            actions: BTreeMap::new(),
            intents: Vec::new(),
            journal: Vec::new(),
            removed: Vec::new(),
            other: Vec::new(),
            notices: true,
            token_forge: "forge",
            ending: None,
            train: false,
        }
    }

    /// A pending decision, with the config's word on notices.
    pub fn for_ctx(state: &'a State, ctx: crate::decide::Ctx<'_>) -> Self {
        let mut p = Self::new(state, ctx.now);
        p.token_forge = ctx
            .config
            .ordered_projects()
            .filter_map(|n| ctx.config.projects.get(n))
            .find(|p| p.forge.hosted())
            .map_or("forge", |p| p.forge.label());
        p.notices = ctx.config.mork.notifications;
        p
    }

    /// A card as this decision has it so far.
    pub fn card(&self, id: &CardId) -> Option<Card> {
        self.cards.get(id).cloned().or_else(|| self.state.cards.get(id).cloned())
    }

    /// The first card, as this decision has it so far, that `f` accepts.
    pub fn card_where(&self, f: impl Fn(&Card) -> bool) -> Option<Card> {
        let kept = self.state.cards.iter().filter(|(id, _)| !self.cards.contains_key(*id)).map(|(_, c)| c);
        self.cards.values().chain(kept).find(|c| f(c)).cloned()
    }

    /// Cards this decision made that the state doesn't hold yet.
    pub fn all_new_cards(&self) -> Vec<Card> {
        self.cards.values().filter(|c| !self.state.cards.contains_key(&c.id)).cloned().collect()
    }

    pub fn put_card(&mut self, mut c: Card) {
        // A card that ends forgets its preview (fac-222): no later start brings it back.
        c.preview &= c.stage.is_live();
        crate::agents::models::rephase(self, &c);
        self.cards.insert(c.id.clone(), c);
    }

    /// Removes an agent (its session ends).
    pub fn remove_agent(&mut self, key: &AgentKey) {
        self.agents.remove(key);
        self.removed.push(key.clone());
    }

    /// Owed work is stored with its intent, surviving a crash before the job is recorded (M3.9).
    pub fn intent(&mut self, i: Intent) {
        if let Intent::StartJob { purpose } = &i
            && (!self.state.owed_work.contains_key(&purpose.key())
                || self.other.iter().any(|c| matches!(c, Change::RemoveOwedWork { key } if *key == purpose.key())))
        {
            self.other.push(Change::PutOwedWork(purpose.clone()));
            self.journal("work.owed", purpose.card().map(|c| c.as_str()), json!({"purpose": purpose}));
        }
        self.intents.push(i);
    }

    pub fn journal(&mut self, kind: &str, card: Option<&str>, data: serde_json::Value) {
        self.journal.push(Journal { kind: kind.to_owned(), card: card.map(str::to_owned), data });
    }

    /// Every ask in id order, this decision's over the state's.
    pub fn all_asks(&self) -> impl Iterator<Item = &Ask> {
        let (mut stored, mut pending) = (self.state.asks.iter().peekable(), self.asks.iter().peekable());
        std::iter::from_fn(move || match (stored.peek(), pending.peek()) {
            (Some((s, _)), Some((n, _))) if s < n => stored.next().map(|(_, a)| a),
            (Some((s, _)), Some((n, _))) if s == n => {
                stored.next();
                pending.next().map(|(_, a)| a)
            }
            (Some(_), None) => stored.next().map(|(_, a)| a),
            _ => pending.next().map(|(_, a)| a),
        })
    }

    /// An ask as this decision has it so far.
    pub fn get_ask(&self, id: &str) -> Option<Ask> {
        self.asks.get(id).cloned().or_else(|| self.state.asks.get(id).cloned())
    }

    pub fn put_ask(&mut self, a: Ask) {
        self.asks.insert(a.id.clone(), a);
    }

    /// An agent as this decision has it so far.
    pub fn agent(&self, key: &AgentKey) -> Option<Agent> {
        self.agents.get(key).cloned().or_else(|| self.state.agents.get(key).cloned())
    }

    /// An agent that will still have its seat after this decision.
    pub(crate) fn remaining_agent(&self, key: &AgentKey) -> Option<Agent> {
        if self.removed.contains(key) { None } else { self.agent(key) }
    }

    pub fn put_agent(&mut self, a: Agent) {
        self.agents.insert(a.key.clone(), a);
    }

    /// The agents, as this decision has them, that `f` accepts; any it removed gone.
    pub fn agents_where(&self, f: impl Fn(&Agent) -> bool) -> Vec<Agent> {
        let kept = self.state.agents.iter().filter(|(k, _)| !self.agents.contains_key(*k)).map(|(_, a)| a);
        self.agents.values().chain(kept).filter(|a| !self.removed.contains(&a.key) && f(a)).cloned().collect()
    }

    /// Makes sure a place exists.
    pub fn touch_place(&mut self, id: &str) {
        self.place(id);
    }

    fn place(&mut self, id: &str) -> &mut Place {
        let existing = self.state.places.get(id).cloned();
        self.places
            .entry(mork_types::PlaceId::new(id))
            .or_insert_with(|| existing.unwrap_or(Place { id: id.into(), next_number: 1 }))
    }

    /// Posts a message; returns its sequence number. The operator's in main are numbered (3.3.5).
    pub fn post(
        &mut self,
        place: &str,
        from: Sender,
        to: Option<&str>,
        kind: MessageKind,
        headline: &str,
        text: &str,
    ) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        let number = if from == Sender::Operator && place == crate::messages::places::MAIN {
            let p = self.place(place);
            let n = p.next_number;
            p.next_number += 1;
            Some(n)
        } else {
            self.place(place);
            None
        };
        // An engine message to an agent is a marker: its label never comes from its text, which
        // shows only unfolded under it (5.7.5, fac-384).
        let marker = from == Sender::Engine && to.is_some_and(|t| t != "operator");
        let headline = match (headline.is_empty(), marker) {
            (false, _) => headline,
            (true, true) => "a note from the engine",
            (true, false) => text.lines().next().unwrap_or(""),
        };
        let role = crate::messages::author_role(self.state, &from);
        self.messages.push(Message {
            seq,
            place: place.into(),
            time: self.now,
            role,
            from,
            to: to.map(str::to_owned),
            kind,
            headline: headline.to_owned(),
            text: text.to_owned(),
            number,
            hand: None,
            images: Vec::new(),
            findings: Vec::new(),
            intent: None,
            delegated: None,
            entries: None,
            choice: None,
        });
        seq
    }

    /// The messages this decision posted in a place.
    pub fn posted_in(&self, place: &str) -> usize {
        self.messages.iter().filter(|m| m.place.as_str() == place).count()
    }

    /// The seq of the hand this decision posted from `from`, its latest.
    pub fn posted_hand(&self, from: &Sender) -> Option<u64> {
        self.messages.iter().rev().find(|m| m.kind == MessageKind::Hand && m.from == *from).map(|m| m.seq)
    }

    /// The last message posted, to add to it (a hand).
    pub fn last_message(&mut self) -> Option<&mut Message> {
        self.messages.last_mut()
    }

    /// A request reaches an agent: it is owed a turn, and a failed or cut agent is retried
    /// (1.4.10.1). A message from the operator, or an orchestrator's for a given ask, answers its `answer` ask (3.5.5).
    pub fn wake(&mut self, who: &Who, from: &Sender) {
        self.wake_for(who, from, None, false, false);
    }

    /// `wake`, for the engine's "Build it" request (fac-328): while only it woke the agent, the card leaving build
    /// or the agent's accepted `built` drops the wake.
    pub fn wake_for_build(&mut self, who: &Who) {
        self.wake_for(who, &Sender::Engine, None, false, true);
    }

    /// `wake`, for a child's completion report (fac-314), a reviewer's of review round `round` (fac-277): while
    /// only such reports woke the agent, a report card's `report-ready` or the round's verdict drops the wake.
    pub fn wake_for_report(&mut self, who: &Who, from: &Sender, round: Option<u32>) {
        self.wake_for(who, from, round, true, false);
    }

    fn wake_for(&mut self, who: &Who, from: &Sender, report: Option<u32>, completion: bool, build: bool) {
        // A card done or closed owes its seats no turns, nor brings back one its end released: the
        // message stays in its place, unread (fac-146).
        let card = crate::messages::places::card_of(self.state, &who.place).and_then(|c| self.card(&c));
        if card.as_ref().is_some_and(|c| !c.stage.is_live()) {
            self.answered(who, from);
            return;
        }
        let mut a = self.agent(&who.key).unwrap_or_else(|| {
            let mut fresh = new_agent(who);
            // A new run's agent reads its run's news, not the run before's.
            fresh.seen = card.as_ref().map_or(0, |c| c.first_seq);
            fresh.paused = card.as_ref().is_some_and(|c| c.paused);
            fresh.seat = crate::messages::places::seat_of(card.as_ref(), who.role).into();
            fresh
        });
        let pending = matches!(a.owed, Some(Owed::Wake)) || a.wake_after;
        match a.owed {
            // A seat ending with its closing turn takes its turn of news once that settles (fac-158).
            _ if a.retiring.is_some() => a.wake_after = true,
            None => a.owed = Some(Owed::Wake),
            Some(Owed::Wake | Owed::Survey | Owed::Lesson) => {}
            Some(Owed::Retry { .. } | Owed::Correction { .. }) => a.wake_after = true,
        }
        let owed = matches!(a.owed, Some(Owed::Wake)) || a.wake_after;
        a.report_round = report.filter(|n| owed && (!pending || a.report_round == Some(*n)));
        a.completion_wake = completion && owed && (!pending || a.completion_wake);
        a.build_wake = build && owed && (!pending || a.build_wake);
        if matches!(a.status, AgentStatus::Error { .. } | AgentStatus::Cut | AgentStatus::Stopped) {
            a.status = AgentStatus::Idle;
            a.cuts = 0;
            // A paused card's agents wait for its `resume` (4.13.2).
            let card = crate::messages::places::card_of(self.state, &a.place).and_then(|c| self.card(&c));
            if !card.is_some_and(|c| c.paused) {
                a.paused = false;
            }
        }
        self.put_agent(a);
        self.answered(who, from);
    }

    fn answered(&mut self, who: &Who, from: &Sender) {
        if *from == Sender::Operator || from.is_orchestrator() {
            // Only the operator's message closes an ask at will; an orchestrator's closes just those given to it (fac-288).
            let only_given = from.is_orchestrator();
            let intent = self.messages.iter().rev().find(|m| m.from == *from).and_then(|m| m.intent);
            let mut given = crate::lifecycle::brief::close_given_plan(self, who, from, intent);
            given.extend(self.close_answers(&who.key, only_given, true));
            given.extend(self.close_by_message(who, only_given));
            // An orchestrator's message that closes a given ask is marked with it: plan or answer ask first (fac-160).
            if from.is_orchestrator()
                && let Some(id) = given.into_iter().next()
                && let Some(m) = self.messages.iter_mut().rev().find(|m| m.from == *from)
            {
                m.delegated = Some(id);
            }
        }
    }

    // A card that ended in this decision drops the turns still owed to its seats (fac-146), one that left build
    // the wake only its "Build it" request caused (fac-328).
    fn drop_ended_turns(&mut self) {
        let ended: Vec<CardId> = self.cards.values().filter(|c| !c.stage.is_live()).map(|c| c.id.clone()).collect();
        let left: Vec<CardId> =
            self.cards.values().filter(|c| c.stage != mork_types::card::Stage::Build).map(|c| c.id.clone()).collect();
        if ended.is_empty() && left.is_empty() {
            return;
        }
        let keys: Vec<AgentKey> =
            self.state.agents.keys().chain(self.agents.keys()).filter(|k| !self.removed.contains(k)).cloned().collect();
        for key in keys {
            let Some(mut a) = self.agent(&key) else { continue };
            let card = crate::messages::places::card_of(self.state, &a.place);
            let on = card.as_ref().is_some_and(|c| ended.contains(c));
            if on && a.retiring.is_none() && (a.owed.is_some() || a.wake_after) {
                a.owed = None;
                a.wake_after = false;
                self.put_agent(a);
            } else if card.is_some_and(|c| left.contains(&c)) && crate::agents::children::build_left(&mut a) {
                self.put_agent(a);
            }
        }
    }

    // A message from the operator answers its `blocked` and `error` asks, and its card's `stuck` for any of its
    // agents (M5.35); an orchestrator's, only those given to it. Returns the ids of those given to an orchestrator.
    fn close_by_message(&mut self, who: &Who, only_given: bool) -> Vec<AskId> {
        let card = crate::messages::places::card_of(self.state, &who.place);
        let open: Vec<Ask> = self
            .all_asks()
            .filter(|a| a.open && (a.given || !only_given))
            .filter(|a| match a.kind {
                AskKind::Blocked | AskKind::Error => a.agent.as_ref() == Some(&who.key),
                AskKind::Stuck => card.as_ref().is_some_and(|c| a.place == c.as_str()),
                // `answer` is closed above; the rest are answered by their own commands.
                AskKind::ApproveBrief
                | AskKind::Answer
                | AskKind::Merge
                | AskKind::ApprovePr
                | AskKind::Done
                | AskKind::Checks
                | AskKind::Cut
                | AskKind::Held
                | AskKind::Alarm
                | AskKind::Token
                | AskKind::NewPr
                | AskKind::Door
                | AskKind::Mock
                | AskKind::Triage => false,
            })
            .cloned()
            .collect();
        let mut given = Vec::new();
        for mut a in open {
            if a.given {
                given.push(a.id.clone());
            }
            a.close();
            a.answered_ms = Some(self.now);
            self.put_ask(a);
        }
        given.sort();
        given
    }

    /// Closes an agent's open `answer` ask; returns the ids of those that were given to an orchestrator.
    pub fn close_answer(&mut self, key: &AgentKey) -> Vec<AskId> {
        self.close_answers(key, false, false)
    }

    // An ask closed by the operator's or the orchestrator's message is answered then; one replaced or withdrawn is not.
    fn close_answers(&mut self, key: &AgentKey, only_given: bool, answered: bool) -> Vec<AskId> {
        let open: Vec<Ask> = self
            .state
            .asks
            .values()
            .chain(self.asks.values())
            .filter(|a| {
                a.kind == AskKind::Answer && a.open && (a.given || !only_given) && a.agent.as_ref() == Some(key)
            })
            .cloned()
            .collect();
        let mut given = Vec::new();
        for mut a in open {
            if a.given {
                given.push(a.id.clone());
            }
            a.close();
            if answered {
                a.answered_ms = Some(self.now);
            }
            self.asks.insert(a.id.clone(), a);
        }
        given.sort();
        given
    }

    pub fn ask(
        &mut self,
        id: mork_types::AskId,
        place: &str,
        kind: AskKind,
        color: Color,
        text: &str,
        agent: Option<&AgentKey>,
    ) {
        let card = crate::messages::places::card_of(self.state, place);
        let mut a = Ask::new(id.clone(), place, card, kind, color, text);
        a.agent = agent.cloned();
        a.raised_ms = self.now;
        self.asks.insert(id, a);
    }

    /// Everything, as changes, with a journal line per message.
    pub fn finish(mut self, d: &mut Decision) {
        self.drop_ended_turns();
        // Every path by which a foreman's last child at work stops passes here: its held reports wake it (fac-528).
        let touched: Vec<AgentKey> = self.agents.keys().chain(&self.removed).cloned().collect();
        crate::agents::held::release(&mut self, &touched);
        for p in self.places.into_values() {
            d.changes.push(Change::PutPlace(p));
        }
        // A card's record is written again when it changes, or when a message is posted in
        // its places or by its agents (2.9.1).
        let mut records: std::collections::BTreeSet<CardId> = self.cards.keys().cloned().collect();
        for m in self.messages {
            let card = crate::messages::places::card_of(self.state, &m.place).map(|c| c.0);
            records.extend(card.clone().map(CardId::new));
            if let Sender::Agent(k) = &m.from
                && let Some(c) =
                    k.as_str().split('/').next().map(CardId::new).filter(|c| self.state.cards.contains_key(c))
            {
                records.insert(c);
            }
            d.journal(
                "message",
                card.as_deref(),
                json!({"place": m.place, "seq": m.seq, "from": m.from.label(), "to": m.to}),
            );
            d.changes.push(Change::AddMessage(m));
        }
        for a in self.asks.into_values() {
            d.journal("ask", a.card.as_ref().map(|c| c.as_str()), json!({"id": a.id, "kind": a.kind, "open": a.open}));
            d.changes.push(Change::PutAsk(a));
        }
        for a in self.agents.into_values() {
            d.changes.push(Change::PutAgent(a));
        }
        for key in self.removed {
            d.changes.push(Change::RemoveAgent { key });
        }
        for c in self.cards.into_values() {
            // A PR approval can reopen a done review without a separate start row (fac-557).
            let before = self.state.cards.get(&c.id).map(|o| (o.stage, o.paused));
            if before != Some((c.stage, c.paused)) {
                let data = json!({"stage": c.stage, "paused": c.paused, "run": c.run});
                d.journal("card.stage", Some(c.id.as_str()), data);
            }
            d.changes.push(Change::PutCard(Box::new(c)));
        }
        d.intents.extend(records.into_iter().map(|card| Intent::WriteRecord { card }));
        for a in self.actions.into_values() {
            d.changes.push(Change::PutAction(Box::new(a)));
        }
        d.changes.extend(self.other);
        d.intents.extend(self.intents);
        d.journal.extend(self.journal);
    }
}
