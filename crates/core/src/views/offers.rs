//! The actions a place or an ask offers (1.5.2, 1.12.6): each a command `decide` would accept now from the operator, found by
//! deciding it and keeping the decision unused, so a view offers exactly what the engine would take.

use mork_types::about::{Switch, SwitchPosition};
use mork_types::ask::{Ask, AskKind};
use mork_types::card::{Card, Mode, Stage};
use mork_types::state::{Agent, Sender};
use mork_types::view::Offer;
use mork_types::{AgentKey, CardId, Command};

use crate::decide::{Ctx, Event, decide};
use crate::messages::places::{self, MAIN};
use crate::state::State;

/// Whether `decide` would accept a command from the operator now.
pub(crate) fn accepted(state: &State, cmd: &Command, ctx: Ctx<'_>) -> bool {
    let e = Event::Command { from: Sender::Operator, cmd: cmd.clone() };
    matches!(decide(state, &e, ctx).answer, Some(Ok(_)))
}

fn offer(label: &str, command: Command) -> Offer {
    Offer { label: label.to_owned(), command }
}

/// The commands a place offers now, as its buttons (5.7.6): a card's own commands, and in
/// main or a place of agents, their stop and resume. Saying is the input's, not a button.
pub fn actions(state: &State, place: &str, ctx: Ctx<'_>) -> Vec<Offer> {
    let mut out = Vec::new();
    if let Some(card) = places::card_of(state, place).filter(|_| place != MAIN) {
        let kept = by_room(state, place, &card);
        out.extend(card_actions(state, &card, ctx).into_iter().filter(|o| kept(o.label.as_str())));
        return out;
    }
    if place == MAIN {
        out.extend(crate::messages::auto::offers(state, ctx));
    }
    for a in state.agents.values().filter(|a| a.place == place) {
        for (label, cmd) in [
            ("stop", Command::Stop { card: None, agent: Some(a.key.clone()) }),
            ("resume", Command::Resume { card: None, agent: Some(a.key.clone()) }),
        ] {
            if accepted(state, &cmd, ctx) {
                out.push(offer(label, cmd));
            }
        }
    }
    out
}

// A planner card's spec room offers the plan's and the mock's `approve` and `mock`, its build room the rest, its
// challenge place and DMs nothing (fac-633); a card with one room offers every one in each place.
fn by_room(state: &State, place: &str, card: &CardId) -> impl Fn(&str) -> bool {
    let spec = |label: &str| matches!(label, "approve" | "mock");
    let two = state.cards.get(card).is_some_and(places::two_rooms);
    let kind = places::kind(state, place);
    move |label| match &kind {
        places::PlaceKind::Spec(_) => spec(label),
        places::PlaceKind::Room(_) if two => !spec(label),
        places::PlaceKind::Challenge(_) => false,
        places::PlaceKind::Dm(..) | places::PlaceKind::Child(..) if two => false,
        places::PlaceKind::Main
        | places::PlaceKind::Room(_)
        | places::PlaceKind::Dm(..)
        | places::PlaceKind::Child(..)
        | places::PlaceKind::Other(_) => true,
    }
}

fn card_actions(state: &State, card: &CardId, ctx: Ctx<'_>) -> Vec<Offer> {
    let c = || card.clone();
    let candidates = [
        ("approve", Command::Approve { card: c() }),
        ("mock", Command::Mock { card: c() }),
        ("merge", Command::Merge { card: c() }),
        ("done", Command::Done { card: c() }),
        ("post", Command::Post { card: c() }),
        ("start", start_command(card, None, restart_carries(state.cards.get(card)))),
    ];
    candidates.into_iter().filter(|(_, cmd)| accepted(state, cmd, ctx)).map(|(l, cmd)| offer(l, cmd)).collect()
}

/// The card's active, delegation and visual controls, with only commands the engine would accept now.
pub fn switches(state: &State, card: &Card, ctx: Ctx<'_>) -> Vec<Switch> {
    let (id, active) = (Some(card.id.clone()), card.stage.is_live() && !card.paused);
    let level = usize::from(card.delegated) + usize::from(card.delegated && card.self_sustained);
    let delegate = |off, self_sustained| Command::Delegate { card: card.id.clone(), off, self_sustained };
    let mut out = vec![
        control(
            state,
            "active",
            usize::from(active),
            vec![
                ("off", Command::Stop { card: id.clone(), agent: None }),
                ("on", Command::Resume { card: id, agent: None }),
            ],
            ctx,
        ),
        control(
            state,
            "delegation",
            level,
            vec![
                ("off", delegate(Some(true), None)),
                ("delegated", delegate(None, None)),
                ("self-sustained", delegate(None, Some(true))),
            ],
            ctx,
        ),
    ];
    if card.mode == Mode::Full {
        out.push(control(
            state,
            "visual",
            usize::from(card.visual),
            vec![
                ("off", Command::Visual { card: card.id.clone(), off: Some(true) }),
                ("on", Command::Visual { card: card.id.clone(), off: None }),
            ],
            ctx,
        ));
    }
    out
}

fn control(state: &State, name: &str, position: usize, choices: Vec<(&str, Command)>, ctx: Ctx<'_>) -> Switch {
    let positions = choices
        .into_iter()
        .enumerate()
        .map(|(i, (label, cmd))| SwitchPosition {
            label: label.to_owned(),
            command: (i != position && accepted(state, &cmd, ctx)).then_some(cmd),
        })
        .collect();
    Switch { name: name.to_owned(), position, positions }
}

// The commands an open ask offers on its hand (5.9.2): its own answer, as `decide` would take
// it now.
fn for_ask(state: &State, ask: &str, ctx: Ctx<'_>) -> Vec<Offer> {
    let Some(a) = state.asks.get(ask).filter(|a| a.open) else { return Vec::new() };
    answers_with(a, |k| state.agents.get(k)).into_iter().filter(|o| accepted(state, &o.command, ctx)).collect()
}

/// What an open ask's hand offers: its answers, then delegation and advice until given (fac-239).
pub fn on_hand(state: &State, ask: &str, ctx: Ctx<'_>) -> Vec<Offer> {
    let mut out = for_ask(state, ask, ctx);
    for (label, command) in [
        ("Orchestrator answers", Command::Give { ask: ask.into() }),
        ("Opinion from orchestrator", Command::Opinion { ask: ask.into() }),
    ] {
        if accepted(state, &command, ctx) {
            out.push(offer(label, command));
        }
    }
    out
}

/// Every answer an ask's buttons may give, whether or not `decide` would take it now, each agent as `agent` gives it: the
/// state a decision leaves, for the orchestrators (fac-119).
pub fn answers_with<'s>(a: &Ask, agent: impl Fn(&AgentKey) -> Option<&'s Agent>) -> Vec<Offer> {
    let ask = a.id.as_str();
    let card = || a.card.clone();
    let mut candidates: Vec<(&str, Command)> = Vec::new();
    match a.kind {
        AskKind::ApproveBrief | AskKind::Mock => {
            candidates.extend(card().map(|c| ("approve", Command::Approve { card: c })));
        }
        AskKind::Merge => candidates.extend(card().map(|c| ("merge", Command::Merge { card: c }))),
        AskKind::Done => candidates.extend(card().map(|c| ("done", Command::Done { card: c }))),
        // A checks ask's `resume` runs them again (4.4.5, M4.27).
        AskKind::Stuck | AskKind::Cut | AskKind::Error | AskKind::Blocked | AskKind::Checks => {
            candidates.extend(session_missing(agent, a).map(|k| ("fresh", Command::Fresh { agent: k.to_string() })));
            candidates.extend(card().map(|c| ("resume", Command::Resume { card: Some(c), agent: None })));
        }
        AskKind::Held | AskKind::Alarm => {
            if let Some(id) = &a.action {
                candidates.push(("retry", Command::ActionRetry { action: id.clone() }));
                candidates.push(("drop", Command::ActionDrop { action: id.clone() }));
            }
        }
        AskKind::Door => {
            candidates.push(("once", Command::DoorOnce { ask: ask.into() }));
            candidates.push(("no", Command::DoorNo { ask: ask.into() }));
        }
        AskKind::Answer | AskKind::Token | AskKind::NewPr | AskKind::ApprovePr | AskKind::Triage => {}
    }
    // An alarm holding a merge is answered by its retry or drop, never acked (fac-157); triage takes `triage` alone.
    if !(a.kind == AskKind::Alarm && a.action.is_some()) && a.kind != AskKind::Triage {
        candidates.push(("ack", Command::Ack { ask: ask.into() }));
    }
    candidates.into_iter().map(|(l, cmd)| offer(l, cmd)).collect()
}

// An error ask's agent whose CLI can't find its session: `fresh` gives it a new one (fac-140).
fn session_missing<'s, 'a>(agent: impl Fn(&AgentKey) -> Option<&'s Agent>, ask: &'a Ask) -> Option<&'a AgentKey> {
    let k = ask.agent.as_ref().filter(|_| ask.kind == AskKind::Error)?;
    let status = &agent(k)?.status;
    let missing = matches!(status, mork_types::state::AgentStatus::Error { words } if crate::agents::fresh::session_missing(words));
    missing.then_some(k)
}

fn restart_carries(prior: Option<&Card>) -> Vec<CardId> {
    prior
        .filter(|c| c.stage != Stage::Done)
        .map(|c| c.carries.iter().map(|k| k.id.clone()).collect())
        .unwrap_or_default()
}

fn start_command(card: &CardId, mode: Option<Mode>, carries: Vec<CardId>) -> Command {
    Command::Start {
        card: card.clone(),
        mode,
        visual: None,
        model: None,
        effort: None,
        delegated: None,
        self_sustained: None,
        no_planner: None,
        carries,
    }
}

pub(super) fn start_defaults(state: &State, place: &str, ctx: Ctx<'_>) -> Option<mork_types::about::StartDefaults> {
    let id = CardId::new(place);
    let board = state.board.get(&id)?;
    let prior = state.cards.get(&id);
    if prior.is_some_and(|c| c.stage.is_live()) {
        return None;
    }
    let carries = restart_carries(prior);
    let takes = |mode| accepted(state, &start_command(&id, Some(mode), carries.clone()), ctx);
    let modes = mork_types::about::StartModes { full: takes(Mode::Full), report: takes(Mode::Report) };
    let (delegated, self_sustained) = crate::messages::auto::inherited(state, &board.project, prior);
    (modes.full || modes.report).then(|| mork_types::about::StartDefaults {
        level: u8::from(delegated) + u8::from(self_sustained),
        from: if prior.is_some_and(|c| c.delegated) { "run" } else { "project" }.into(),
        visual: crate::lifecycle::visual::inherited(Mode::Full, prior),
        carries,
        modes,
    })
}

pub(super) fn preview(state: &State, card: &Card, ctx: Ctx<'_>) -> Option<mork_types::about::PreviewControl> {
    let (_, port) = crate::lifecycle::preview::serves(card, ctx.config)?;
    let action = if card.preview { "stop" } else { "start" };
    let command = Command::Preview { card: card.id.clone(), action: Some(action.into()) };
    Some(mork_types::about::PreviewControl {
        on: card.preview,
        url: crate::lifecycle::preview::url(&card.id, port),
        enabled: accepted(state, &command, ctx),
    })
}

pub(super) fn header_actions(state: &State, c: &Card, ctx: Ctx<'_>) -> Vec<String> {
    let id = || c.id.clone();
    [
        Command::Brief { card: id(), words: None },
        Command::Mock { card: id() },
        Command::Close { card: id(), why: "close".into() },
    ]
    .into_iter()
    .filter(|cmd| accepted(state, cmd, ctx))
    .map(|cmd| cmd.name().to_owned())
    .collect()
}
