//! The engine's requests and updates to a card's agents, in each one's room: a request wakes its agent,
//! but for a foreman whose recap waits on the operator (fac-228).

use mork_types::card::Stage;
use mork_types::reply::Role;
use mork_types::state::{MessageKind, Sender};
use mork_types::{AgentKey, CardId};

use super::places::Who;
use crate::post::Poster;

/// A card's agent in a role, by key, in its room (no check: the card may be new in this decision).
pub fn agent_of(card: &CardId, role: Role) -> Who {
    Who {
        key: AgentKey::new(format!("{card}/{}", role.as_str())),
        seat: role.as_str().into(),
        role,
        place: super::places::room_of(card, role).into(),
    }
}

/// The engine's request to a card's agent: posted in its room, waking it. Its message's seq.
pub fn ask_agent(p: &mut Poster<'_>, card: &CardId, role: Role, headline: &str, text: &str) -> u64 {
    ask(p, card, role, headline, text, false)
}

/// The engine's request to a full card's foreman in its challenge place, which no window shows (fac-633), waking it.
pub(crate) fn ask_challenge(p: &mut Poster<'_>, card: &CardId, headline: &str, text: &str) -> u64 {
    let place = super::places::challenge_place(card);
    let seq = p.post(&place, Sender::Engine, Some(Role::Foreman.as_str()), MessageKind::Request, headline, text);
    p.wake(&agent_of(card, Role::Foreman), &Sender::Engine);
    seq
}

/// `ask_agent` for the "Build it" request (fac-328): its wake is dropped once the card leaves build or the agent's
/// `built` is accepted.
pub(crate) fn ask_build(p: &mut Poster<'_>, card: &CardId, text: &str) -> u64 {
    ask(p, card, Role::Foreman, "Build it", text, true)
}

/// `ask_build` for the request to continue the build after a reopened mock (fac-591).
pub(crate) fn ask_continue(p: &mut Poster<'_>, card: &CardId, text: &str) -> u64 {
    ask(p, card, Role::Foreman, "Continue the build", text, true)
}

fn ask(p: &mut Poster<'_>, card: &CardId, role: Role, headline: &str, text: &str, build: bool) -> u64 {
    let who = agent_of(card, role);
    let seq = p.post(who.place.as_str(), Sender::Engine, Some(role.as_str()), MessageKind::Request, headline, text);
    if !recap_waits(p, card, role) {
        if build {
            p.wake_for_build(&who);
        } else {
            p.wake(&who, &Sender::Engine);
        }
    }
    seq
}

// A foreman woken while its recap or checked specification waits would withdraw it (fac-228):
// the request stays in the room for the turn the operator's answer brings.
fn recap_waits(p: &Poster<'_>, card: &CardId, role: Role) -> bool {
    p.card(card).is_some_and(|c| {
        crate::lifecycle::facts::holder(&c) == role
            && matches!(c.stage, Stage::Starting | Stage::Brief)
            && c.draft.is_some()
            && !crate::lifecycle::mock::open(&c)
    })
}

/// The engine's update to a card's agent, in its room: posted, waking nobody. Its message's seq.
pub fn tell_agent(p: &mut Poster<'_>, card: &CardId, role: Role, headline: &str, text: &str) -> u64 {
    let place = super::places::room_of(card, role);
    p.post(&place, Sender::Engine, Some(role.as_str()), MessageKind::Update, headline, text)
}
