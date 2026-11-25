//! Asks (3.5.5, 3.9, 4.4.5, 2.6): raised with their colour and words, joined when one decision
//! raises two, claimed for one desktop notice, closed by what answers them.

use factory_types::AgentKey;
use factory_types::ask::{Ask, AskKind, Color};
use serde_json::json;

use crate::decide::{Ctx, Decision, Intent};
use crate::post::Poster;
use crate::state::State;

/// The kind's word, as ids and the screen use it.
pub fn word(k: AskKind) -> &'static str {
    match k {
        AskKind::ApproveBrief => "approve-brief",
        AskKind::Answer => "answer",
        AskKind::Merge => "merge",
        AskKind::ApprovePr => "approve-pr",
        AskKind::Done => "done",
        AskKind::Stuck => "stuck",
        AskKind::Checks => "checks",
        AskKind::Blocked => "blocked",
        AskKind::Error => "error",
        AskKind::Cut => "cut",
        AskKind::Held => "held",
        AskKind::Alarm => "alarm",
        AskKind::Token => "token",
        AskKind::NewPr => "new-pr",
        AskKind::Door => "door",
        AskKind::Mock => "mock",
        AskKind::Triage => "triage",
    }
}

#[derive(Debug, Clone)]
pub struct Raise {
    pub place: factory_types::PlaceId,
    pub kind: AskKind,
    pub color: Color,
    pub text: String,
    pub detail: String,
    pub agent: Option<AgentKey>,
    pub action: Option<factory_types::ActionId>,
    /// A foreman's design question (fac-233).
    pub design: bool,
    /// Its hand's options and pick (fac-696).
    pub choices: Option<factory_types::ask::Choices>,
}

impl Raise {
    pub fn new(place: &str, kind: AskKind, color: Color, text: impl Into<String>) -> Self {
        Self {
            place: place.into(),
            kind,
            color,
            text: text.into(),
            detail: String::new(),
            agent: None,
            action: None,
            design: false,
            choices: None,
        }
    }
}

/// Whether a planner or foreman's own hand marks its ask a design question (fac-233).
pub fn marked(state: &State, who: &super::places::Who, r: &factory_types::reply::Reply) -> bool {
    let child = state.agents.get(&who.key).is_some_and(crate::agents::children::is_child);
    r.hand.design
        && matches!(who.role, factory_types::reply::Role::Planner | factory_types::reply::Role::Foreman)
        && !child
}

/// Reuses an open headline, matching the action too for door approvals; otherwise notifies once.
/// Returns the ask's `<place>/<kind>/<n>` id (2.6.4, fac-124).
pub fn raise(p: &mut Poster<'_>, r: Raise) -> factory_types::AskId {
    if let Some(same) = p.all_asks().find(|a| {
        a.open
            && a.place == r.place
            && a.kind == r.kind
            && a.text == r.text
            && (r.kind != AskKind::Answer || a.agent == r.agent)
            && (r.kind != AskKind::Door || a.action == r.action)
            && a.design == r.design
            && a.choices == r.choices
    }) {
        return same.id.clone();
    }
    // Above every number stored, the unloaded asks' included (fac-336): a count would reuse one.
    let prefix = format!("{}/{}", r.place, word(r.kind));
    let stored = p.state.unloaded.asks.get(&prefix).copied().unwrap_or(0);
    let here = p.all_asks().filter(|a| a.place == r.place && a.kind == r.kind);
    let n = here.filter_map(|a| a.id.as_str().rsplit_once('/')?.1.parse().ok()).fold(stored, u64::max) + 1;
    let id = factory_types::AskId::new(format!("{prefix}/{n}"));
    let card = super::places::card_of(p.state, &r.place);
    let mut a = Ask::new(id.clone(), &r.place, card.clone(), r.kind, r.color, &r.text);
    a.agent = r.agent;
    a.action = r.action;
    a.detail = r.detail;
    a.design = r.design;
    a.choices = r.choices;
    a.raised_ms = p.now;
    if p.notices {
        a.notice = true;
        p.intent(Intent::Notify { ask: id.clone() });
    }
    p.journal("ask", card.as_ref().map(|c| c.as_str()), json!({"id": id, "kind": word(r.kind), "open": true}));
    p.put_ask(a);
    id
}

/// Closes the open asks of a place of these kinds.
pub fn close(p: &mut Poster<'_>, place: &str, kinds: &[AskKind]) {
    let open: Vec<Ask> =
        p.all_asks().filter(|a| a.open && a.place == place && kinds.contains(&a.kind)).cloned().collect();
    for mut a in open {
        a.close();
        a.answered_ms = Some(p.now);
        p.put_ask(a);
    }
}

pub fn close_one(p: &mut Poster<'_>, id: &str) {
    if let Some(mut a) = p.get_ask(id).filter(|a| a.open) {
        a.close();
        a.answered_ms = Some(p.now);
        p.put_ask(a);
    }
}

/// Every open ask of a card (its room and DMs).
pub fn open_of_card(p: &Poster<'_>, card: &str) -> Vec<Ask> {
    p.all_asks().filter(|a| a.open && a.card.as_ref().is_some_and(|c| c.as_str() == card)).cloned().collect()
}

/// `ack`: answers a `new-pr` or `answer` ask without acting on it (4.4.5); closes an alarm unless
/// it holds a merge (fac-157), and an ended card's error ask (fac-264).
pub(super) fn ack(state: &State, id: &str, ctx: Ctx<'_>) -> Decision {
    let Some(a) = state.asks.get(id).filter(|a| a.open) else {
        return Decision::refused(format!("no open ask `{id}`"));
    };
    if a.kind == AskKind::Alarm && a.action.is_some() {
        return Decision::refused(format!("`{id}` holds a merge: its retry or drop answers it, not `ack`"));
    }
    // An ended card's error ask has nothing left to resume: else only a rerun clears it (fac-264).
    let ended = a.card.as_ref().and_then(|c| state.cards.get(c)).is_some_and(crate::lifecycle::gates::ended);
    if !matches!(a.kind, AskKind::NewPr | AskKind::Answer | AskKind::Alarm) && !(a.kind == AskKind::Error && ended) {
        return Decision::refused(format!(
            "`ack` answers a new-pr, answer or alarm ask, or an ended card's error ask; `{id}` is a {} ask",
            word(a.kind)
        ));
    }
    let mut p = Poster::for_ctx(state, ctx);
    close_one(&mut p, id);
    if a.kind == AskKind::Alarm
        && let Some(c) = a.card.as_ref().and_then(|c| p.card(c))
    {
        crate::queue::landing::alarm_closed(&mut p, c, ctx);
    }
    let mut d = Decision::ok(json!({"ack": {"ask": id}}));
    p.finish(&mut d);
    d
}
