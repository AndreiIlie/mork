//! Places and who is in them (3.3, 3.4): main, a card's build room, its spec room and challenge place, a DM; the agent a
//! name reaches from a place. The route a reply's message takes is `routes`'.

use mork_types::card::{Card, Mode};
use mork_types::reply::Role;
use mork_types::state::Sender;
use mork_types::{AgentKey, CardId, PlaceId, SeatName};

use crate::decide::Ctx;
use crate::state::State;

pub use super::routes::{Route, recipients, route};

pub const MAIN: &str = "main";

/// A place, by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceKind {
    Main,
    /// `<card>`: its build room, the operator's and its foreman's.
    Room(CardId),
    /// `<card>/spec`: a full card's spec room, the operator's and its planner's (fac-633).
    Spec(CardId),
    /// `<card>/challenge`: its foreman's and planner's exchange, which no window shows (fac-633).
    Challenge(CardId),
    /// `<card>/<role>/dm`.
    Dm(CardId, Role),
    /// `<card>/<child>/dm`: a foreman's child (fac-94), whose place is its DM.
    Child(CardId, AgentKey),
    /// A place the tests make (1.10.4), or one that is no more.
    Other(String),
}

/// A card's roles that have agents in its room (3.3.2).
pub const CARD_ROLES: [Role; 2] = [Role::Foreman, Role::Reviewer];

fn card_role(name: &str) -> Option<Role> {
    CARD_ROLES.iter().copied().find(|r| r.as_str() == name)
}

pub(super) fn room_role(name: &str) -> Option<Role> {
    if name == "planner" { Some(Role::Planner) } else { card_role(name) }
}

/// What a place id names, in this state.
pub fn kind(state: &State, place: &str) -> PlaceKind {
    if place == MAIN {
        return PlaceKind::Main;
    }
    let parts: Vec<&str> = place.split('/').collect();
    // An ended card the state doesn't hold is still a card (fac-336).
    let known =
        |card: &str| state.cards.contains_key(&CardId::new(card)) || state.unloaded.cards.contains(&CardId::new(card));
    match parts.as_slice() {
        [card] if known(card) => PlaceKind::Room(CardId::new(*card)),
        [card, "spec"] if known(card) => PlaceKind::Spec(CardId::new(*card)),
        [card, "challenge"] if known(card) => PlaceKind::Challenge(CardId::new(*card)),
        [card, role, "dm"] if known(card) => match card_role(role) {
            Some(r) => PlaceKind::Dm(CardId::new(*card), r),
            None if crate::agents::children::child_name(role) || builder_history(state, place, role) => {
                PlaceKind::Child(CardId::new(*card), AgentKey::new(format!("{card}/{role}")))
            }
            None => PlaceKind::Other(place.to_owned()),
        },
        _ => PlaceKind::Other(place.to_owned()),
    }
}

fn builder_history(state: &State, place: &str, name: &str) -> bool {
    state.places.contains_key(place) && (name == "builder" || crate::agents::children::builder_dm_name(name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub key: AgentKey,
    pub seat: SeatName,
    pub role: Role,
    pub place: PlaceId,
}

impl Who {
    fn new(key: String, seat: &str, role: Role, place: &str) -> Self {
        Self { key: AgentKey::new(key), seat: SeatName::new(seat), role, place: PlaceId::new(place) }
    }
}

/// The card a place belongs to: its rooms, its challenge place, or one of its DMs.
pub fn card_of(state: &State, place: &str) -> Option<CardId> {
    match kind(state, place) {
        PlaceKind::Room(c)
        | PlaceKind::Spec(c)
        | PlaceKind::Challenge(c)
        | PlaceKind::Dm(c, _)
        | PlaceKind::Child(c, _) => Some(c),
        PlaceKind::Main | PlaceKind::Other(_) => None,
    }
}

/// Whether a card has a spec room beside its build room: a full card's with a planner (fac-633).
pub fn two_rooms(card: &Card) -> bool {
    card.mode == Mode::Full && !card.no_planner
}

/// A card's spec room, `<card>/spec`.
pub fn spec_room(card: &CardId) -> String {
    format!("{card}/spec")
}

/// A card's challenge place, `<card>/challenge`.
pub fn challenge_place(card: &CardId) -> String {
    format!("{card}/challenge")
}

/// The room a card's seat in a role sits in: the planner's spec room, every other seat's the build room.
pub fn room_of(card: &CardId, role: Role) -> String {
    if role == Role::Planner { spec_room(card) } else { card.as_str().to_owned() }
}

/// The room a seat's asks are raised in, by its role: its card's spec room for the planner, else its build room.
pub fn asks_room(state: &State, who: &Who) -> String {
    card_of(state, &who.place).map_or_else(|| who.place.to_string(), |card| room_of(&card, who.role))
}

/// Whether a place is a card's that is done or closed: one the state doesn't hold is (fac-336).
pub fn ended(state: &State, place: &str) -> bool {
    card_of(state, place)
        .is_some_and(|c| state.cards.get(&c).map_or(state.unloaded.cards.contains(&c), |c| !c.stage.is_live()))
}

fn card_has(card: &Card, role: Role) -> Result<(), String> {
    match role {
        Role::Planner if card.mode != Mode::Full || card.no_planner => Err(format!("{} has no planner", card.id)),
        Role::Planner | Role::Foreman => Ok(()),
        Role::Reviewer | Role::Researcher | Role::Baseline | Role::Echo => {
            Err(format!("{} has no {}", card.id, role.as_str()))
        }
    }
}

/// A card's seat in a role, in its own room.
pub fn card_agent(state: &State, card: &CardId, role: Role) -> Result<Who, String> {
    let c = state.cards.get(card).ok_or_else(|| format!("no card `{card}`"))?;
    card_has(c, role)?;
    Ok(Who::new(format!("{card}/{}", role.as_str()), seat_of(Some(c), role), role, &room_of(card, role)))
}

pub fn dm_agent(state: &State, card: &CardId, role: Role) -> Result<Who, String> {
    let c = state.cards.get(card).ok_or_else(|| format!("no card `{card}`"))?;
    card_has(c, role)?;
    let place = format!("{card}/{}/dm", role.as_str());
    Ok(Who::new(place.clone(), seat_of(Some(c), role), role, &place))
}

/// The seat a new agent of a role takes on a card: a review card's foreman has its own (fac-246).
pub fn seat_of(card: Option<&Card>, role: Role) -> &'static str {
    if role == Role::Foreman {
        mork_types::config::foreman_seat(card.is_some_and(|c| c.mode == Mode::Review))
    } else {
        role.as_str()
    }
}

// An agent of main by name: only the tests' echo agent (fac-119 removed the assistant and the
// advisors).
pub(super) fn main_agent(name: &str, ctx: Ctx<'_>) -> Result<Who, String> {
    match name {
        "echo" if ctx.faults => Ok(Who::new(format!("{MAIN}/echo"), "echo", Role::Echo, MAIN)),
        n => Err(format!("no agent `{n}` in main")),
    }
}

/// The leading `@name` of the operator's words, if any (3.4.5, 5.10.1).
pub(super) fn addressee(text: &str) -> Option<&str> {
    let first = text.split_whitespace().next()?;
    first.strip_prefix('@').filter(|n| !n.is_empty())
}

/// The agent the operator's words (or a `say`) in a place reach: `to` when given, else a leading
/// `@name`, else the place's own (3.4.5).
pub(super) fn addressed(state: &State, place: &str, to: Option<&str>, text: &str, ctx: Ctx<'_>) -> Result<Who, String> {
    let named = to.or_else(|| addressee(text));
    match kind(state, place) {
        PlaceKind::Main => match named {
            Some(n) => main_agent(n, ctx),
            None => Err("The operator's words in main reach nobody: name an agent".into()),
        },
        PlaceKind::Room(card) => in_room(state, &card, named, false),
        PlaceKind::Spec(card) => in_room(state, &card, named, true),
        PlaceKind::Challenge(card) => Err(format!(
            "{card}/challenge is its foreman's and planner's exchange, where nobody says anything: say it in `{card}` or `{card}/spec`"
        )),
        PlaceKind::Child(card, key) => {
            let own = key.as_str().rsplit('/').next().unwrap_or_default();
            match (named, crate::agents::children::named(state, &card, own)) {
                (Some(n), _) if n != own => Err(format!("a DM holds only its agent, not `{n}`")),
                (_, Some(who)) => Ok(who),
                (_, None) => Err(format!("no agent `{own}` in {card}")),
            }
        }
        PlaceKind::Dm(card, role) => match named {
            None => dm_agent(state, &card, role),
            Some(n) if n == role.as_str() => dm_agent(state, &card, role),
            Some(n) => Err(format!("a DM holds only its agent, not `{n}`")),
        },
        PlaceKind::Other(p) => Err(format!(
            "no place `{p}`: use `main`, a card's room `<card>` or `<card>/spec` (one of its agents with `--to <name>`), or a DM `<card>/<name>/dm`"
        )),
    }
}

// The seat a name reaches in a card's build or spec room, its own seat unnamed: each seat is reached in its own room
// alone (fac-633), a child in the room of its parent.
fn in_room(state: &State, card: &CardId, named: Option<&str>, spec: bool) -> Result<Who, String> {
    let c = state.cards.get(card).ok_or_else(|| format!("no card `{card}`"))?;
    if spec && !two_rooms(c) {
        return Err(format!("{card} has no spec room: only a full card with a planner has one"));
    }
    let n = named.unwrap_or(if spec { "planner" } else { "foreman" });
    let n = n.strip_prefix(card.as_str()).and_then(|s| s.strip_prefix('/')).unwrap_or(n);
    match room_role(n) {
        Some(Role::Planner) if !spec && two_rooms(c) => {
            Err(format!("the planner is in {card}'s spec room: say it in `{card}/spec`"))
        }
        Some(r) if spec && r != Role::Planner => Err(format!("`{n}` is in {card}'s build room: say it in `{card}`")),
        Some(r) => card_agent(state, card, r),
        None => child_in(state, card, n, spec).ok_or_else(|| {
            let room = if spec { spec_room(card) } else { card.to_string() };
            format!("no agent `{n}` in {room}")
        }),
    }
}

/// A card's child by its name in one of its rooms: the planner's researchers in the spec room, every other child in
/// the build room.
pub(super) fn child_in(state: &State, card: &CardId, name: &str, spec: bool) -> Option<Who> {
    let who = crate::agents::children::named(state, card, name)?;
    let planner = format!("{card}/planner");
    let of_planner = state.agents.get(&who.key).and_then(|a| a.parent.as_ref()).is_some_and(|p| p.as_str() == planner);
    (of_planner == spec).then_some(who)
}

// After approval only a named request or a current researcher's report wakes a planner.
pub(super) fn planner_sender(state: &State, from: &Sender, to: &Who) -> bool {
    if to.role != Role::Planner {
        return true;
    }
    let Some(card) = card_of(state, &to.place).and_then(|c| state.cards.get(&c)) else { return false };
    if crate::lifecycle::facts::agreeing(card) || *from == Sender::Operator || from.is_orchestrator() {
        return true;
    }
    let Sender::Agent(key) = from else { return false };
    state.agents.get(key).is_some_and(|a| {
        a.retiring.is_none()
            && (a.parent.as_ref() == Some(&to.key)
                || (a.role == Role::Foreman && card_of(state, &a.place).as_ref() == Some(&card.id)))
    })
}

// The names that once posted for the operator or a room, and its aliases: he is reached only by a raised hand (fac-390).
const TO_OPERATOR: [&str; 4] = ["operator", "room", "runner", "mork"];

/// Why an agent's words for the operator go nowhere: where its flagged hand message takes them instead (fac-390).
pub fn hand_only(child: bool) -> String {
    if child {
        "you never reach the operator: your flagged hand message reaches your foreman as a request, and it carries what needs him"
            .into()
    } else {
        "an agent reaches the operator only by raising its hand: put it in your flagged hand message, end your turn and wait for his answer".into()
    }
}

/// Whether a name is one of those the router refuses for the operator's sake.
pub fn for_operator(name: &str) -> bool {
    TO_OPERATOR.contains(&name.trim().to_lowercase().as_str())
}

/// Who an existing agent is, from its key.
pub fn who_of(state: &State, key: &AgentKey) -> Option<Who> {
    let a = state.agents.get(key)?;
    Some(Who { key: key.clone(), seat: a.seat.clone(), role: a.role, place: a.place.clone() })
}
