//! The route a reply's message, or a seat's `say` to a name, takes from its sender's place (3.4.2): a full card's
//! foreman and planner reach each other only in its challenge place (fac-633).

use factory_types::CardId;
use factory_types::reply::Role;
use factory_types::state::Sender;

use super::places::{self, MAIN, PlaceKind, Who, card_agent, card_of, kind};
use crate::decide::Ctx;
use crate::state::State;

/// Where a reply's message goes (3.4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Posted in a place, to the `orchestrator` label, waking nobody.
    Post { place: String, to: String },
    /// Posted in a place, reaching an agent: it wakes it.
    Agent { place: String, to: String, who: Who },
    /// A recipient the router can't place: posted nowhere, its sender told why. `why` is the router's reason.
    Unplaced { to: String, why: String },
}

/// The route of a reply's message from an agent: every recipient has one.
pub fn route(state: &State, from: &Who, to: &str, ctx: Ctx<'_>) -> Route {
    let routed = placed(state, from, to, ctx).and_then(|route| {
        if let Route::Agent { who, .. } = &route
            && !places::planner_sender(state, &Sender::Agent(from.key.clone()), who)
        {
            return Err("the planner reopens for its foreman, Andrei or an orchestrator".into());
        }
        Ok(route)
    });
    routed.unwrap_or_else(|why| Route::Unplaced { to: to.to_owned(), why })
}

// Why a name isn't placed is kept for the journal's `message.unplaced`.
fn placed(state: &State, from: &Who, to: &str, ctx: Ctx<'_>) -> Result<Route, String> {
    let lower = to.trim().to_lowercase();
    // The sender's own card's full key is its bare name: `fac-2/researcher-1` from fac-2's room (fac-148).
    let own = card_of(state, &from.place);
    let name = own.as_ref().and_then(|c| lower.strip_prefix(c.as_str())?.strip_prefix('/')).unwrap_or(&lower);
    let orchestrator = own.as_ref().and_then(|c| state.cards.get(c)).map(|c| format!("orchestrator:{}", c.project));
    let name = if orchestrator.is_some_and(|o| o.to_lowercase() == name) { "orchestrator" } else { name };
    if places::for_andrei(name) {
        return Err(places::hand_only(matches!(kind(state, &from.place), PlaceKind::Child(..))));
    }
    let post = |place: &str, to: &str| Ok(Route::Post { place: place.to_owned(), to: to.to_owned() });
    match kind(state, &from.place) {
        PlaceKind::Dm(card, role) => match name {
            "orchestrator" => post(&from.place, name),
            "planner" if role == Role::Foreman => challenge(state, &card, Role::Planner),
            n => Err(format!("a DM's messages go to `orchestrator`, not `{n}`")),
        },
        // A child reaches its parent in the parent's room (fac-94).
        PlaceKind::Child(_, _) => match name {
            "orchestrator" => post(&from.place, name),
            "foreman" | "planner" => child_parent(state, from, name).map(room_route),
            n => Err(format!("a child's messages go to its parent or `orchestrator`, not `{n}`")),
        },
        PlaceKind::Room(card) | PlaceKind::Spec(card) => match name {
            "orchestrator" => post(&from.place, name),
            n => in_card(state, &card, n, matches!(kind(state, &from.place), PlaceKind::Spec(_))),
        },
        PlaceKind::Challenge(card) => Err(format!("no seat sits in {card}/challenge: `{to}` is reached from a room")),
        PlaceKind::Main => match name {
            "orchestrator" => post(MAIN, name),
            n => match foreman_of_card(state, n) {
                Some(other) => other_foreman(state, &other, n),
                None => places::main_agent(n, ctx)
                    .map(|who| Route::Agent { place: MAIN.into(), to: n.into(), who })
                    .map_err(|e| format!("`{to}` isn't a recipient in main: {e}")),
            },
        },
        PlaceKind::Other(p) => match name {
            "orchestrator" => post(&p, name),
            _ => Err(format!("`{to}` isn't a recipient in {p}")),
        },
    }
}

// From a card's build or spec room: its own seat there, the other room's seat in the challenge place, a child of
// that room in its DM, or another card's foreman in that card's build room.
fn in_card(state: &State, card: &CardId, n: &str, spec: bool) -> Result<Route, String> {
    if spec && n == FOREMAN_DM {
        return dm_challenge(state, card);
    }
    let other = if spec { Role::Foreman } else { Role::Planner };
    let own = if spec { Role::Planner } else { Role::Foreman };
    match (places::room_role(n), foreman_of_card(state, n), places::child_in(state, card, n, spec)) {
        (Some(r), _, _) if r == other => challenge(state, card, r),
        (Some(r), _, _) if spec && r != own => {
            Err(format!("`{n}` isn't a recipient in a spec room: the planner reaches the build through `foreman`"))
        }
        (Some(r), _, _) => card_agent(state, card, r).map(room_route),
        (None, Some(other), _) => other_foreman(state, &other, n),
        // A foreman's or planner's child hears it in its DM (fac-94).
        (None, None, Some(who)) => Ok(Route::Agent { place: who.place.to_string(), to: n.into(), who }),
        (None, None, None) => Err(format!("`{n}` isn't a recipient in a card room")),
    }
}

// The foreman's and planner's words to each other, posted where neither room shows them (fac-633).
fn challenge(state: &State, card: &CardId, role: Role) -> Result<Route, String> {
    let who = card_agent(state, card, role)?;
    Ok(Route::Agent { place: places::challenge_place(card), to: role.as_str().into(), who })
}

/// The planner's name for its foreman's DM, the way back to the DM that asked it (fac-633).
pub const FOREMAN_DM: &str = "foreman/dm";

// The planner's words to its foreman's DM, addressed to that seat alone: the room foreman never reads them.
fn dm_challenge(state: &State, card: &CardId) -> Result<Route, String> {
    let who = places::dm_agent(state, card, Role::Foreman)?;
    Ok(Route::Agent { place: places::challenge_place(card), to: who.key.as_str().into(), who })
}

fn room_route(who: Who) -> Route {
    Route::Agent { place: who.place.to_string(), to: who.role.as_str().into(), who }
}

fn child_parent(state: &State, from: &Who, name: &str) -> Result<Who, String> {
    let parent = state.agents.get(&from.key).and_then(|a| a.parent.as_ref()).and_then(|k| places::who_of(state, k));
    match parent {
        Some(who) if name == "foreman" || who.role == Role::Planner => Ok(who),
        _ => Err(format!("{} has no parent you can reach as `{name}`", from.key)),
    }
}

// Only the router's own names: `recipients` keeps those `placed` takes from where the sender is.
fn names(state: &State, from: &Who, ctx: Ctx<'_>) -> Vec<String> {
    let own = card_of(state, &from.place);
    let mut out: Vec<String> = vec!["orchestrator".to_owned()];
    out.extend(places::CARD_ROLES.iter().map(|r| r.as_str().to_owned()));
    out.push("planner".to_owned());
    out.push(FOREMAN_DM.to_owned());
    if ctx.faults {
        out.push("echo".to_owned());
    }
    if let Some(c) = own.as_ref() {
        out.extend(
            crate::agents::children::of_card(state, c)
                .iter()
                .map(|w| w.key.as_str().rsplit('/').next().unwrap_or_default().to_owned()),
        );
    }
    let live = state.cards.values().filter(|c| c.stage.is_live() && Some(&c.id) != own.as_ref());
    out.extend(live.map(|c| format!("{}/foreman", c.id)));
    out
}

/// Whom an agent's messages reach from where it is, as `route` places them: the line its prompt shows. Each name says
/// the others that reach the same and whether it is the agent itself; `<card>/foreman` for any other card closes it.
pub fn recipients(state: &State, from: &Who, ctx: Ctx<'_>) -> Vec<String> {
    let own = card_of(state, &from.place);
    let reached = |n: &str| match route(state, from, n, ctx) {
        Route::Agent { who, .. } => Some(Some(who)),
        Route::Post { .. } => Some(None),
        Route::Unplaced { .. } => None,
    };
    let mut out = Vec::new();
    for n in names(state, from, ctx) {
        let Some(target) = reached(&n) else { continue };
        let mut also: Vec<String> = Vec::new();
        if let Some(c) = own.as_ref().filter(|_| n == "foreman") {
            also.extend(Some(format!("{c}/foreman")).filter(|f| reached(f).is_some()));
        }
        if let Some(c) = own.as_ref().filter(|_| n == "orchestrator").and_then(|c| state.cards.get(c)) {
            also.push(format!("orchestrator:{}", c.project));
        }
        let mut notes: Vec<String> = Vec::new();
        if !also.is_empty() {
            notes.push(format!("or {}", also.iter().map(|a| format!("`{a}`")).collect::<Vec<_>>().join(", ")));
        }
        if target.is_some_and(|w| w.key == from.key) {
            notes.push("yourself: it wakes you after this turn".into());
        }
        out.push(if notes.is_empty() { format!("`{n}`") } else { format!("`{n}` ({})", notes.join("; ")) });
    }
    let other = state.cards.keys().filter(|c| Some(*c) != own.as_ref());
    if other.map(|c| format!("{c}/foreman")).any(|f| reached(&f).is_some()) {
        out.push("`<card>/foreman` for any other card".into());
    }
    out
}

fn foreman_of_card(state: &State, name: &str) -> Option<CardId> {
    let card = name.strip_suffix("/foreman")?;
    let id = CardId::new(card);
    state.cards.contains_key(&id).then_some(id)
}

// Another card's foreman hears it in that card's build room.
fn other_foreman(state: &State, card: &CardId, name: &str) -> Result<Route, String> {
    let who = card_agent(state, card, Role::Foreman)?;
    Ok(Route::Agent { place: card.as_str().to_owned(), to: name.to_owned(), who })
}
