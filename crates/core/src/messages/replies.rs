//! An accepted reply's effects (3.4, 3.5): its hand posted, its messages routed and their
//! recipients woken and its `answer` ask. What its outcome moves is section 4's.

use mork_types::ask::{AskKind, Color};
use mork_types::reply::{Intent, Outcome, Reply, ReplyMessage};
use mork_types::state::{MessageKind, Sender};
use serde_json::json;

use super::places::{self, Route, Who};
use crate::decide::Ctx;
use crate::post::Poster;
use crate::state::State;

/// Every message's route (3.4.2): each has one, a recipient the router can't place included.
pub fn routes(state: &State, who: &Who, reply: &Reply, ctx: Ctx<'_>) -> Vec<Route> {
    reply.messages.iter().map(|m| places::route(state, who, &m.to, ctx)).collect()
}

/// Applies an accepted reply, whose routes are already resolved. `gate`: it raises a gate's ask (4.4.5);
/// `round`: the review round a reviewer's turn was given, still under way (fac-325).
pub fn apply(
    p: &mut Poster<'_>,
    who: &Who,
    reply: &Reply,
    routes: &[Route],
    (gate, round): (bool, Option<u32>),
    ctx: Ctx<'_>,
) {
    let state = p.state;
    let from = Sender::Agent(who.key.clone());
    let h = &reply.hand;
    // Every numbered child's question remains its foreman's (fac-499); the planner's hand is the operator's (fac-633).
    let parent =
        state.agents.get(&who.key).filter(|a| crate::agents::children::is_child(a)).and_then(|a| a.parent.as_ref());
    let to = parent.and_then(|k| k.as_str().rsplit('/').next()).unwrap_or("operator");
    // A recap's typed sketch heads its words in the room, as it heads its plan (fac-696).
    let text = h.sketch().map_or_else(|| h.message.clone(), |s| format!("{}\n\n{}", s.words(), h.message));
    let hand_seq = p.post(&who.place, from.clone(), Some(to), MessageKind::Hand, &h.headline, &text);
    if let Some(m) = p.last_message() {
        m.hand = Some(Box::new(h.clone()));
        m.images = h.images.clone();
        // Its findings stay with it, whatever its outcome: the room shows them, and the record
        // keeps them (T4.7, M3.29).
        m.findings = reply.findings.clone();
    }
    for (m, route) in reply.messages.iter().zip(routes) {
        message(p, who, reply, (m, route, round), ctx);
    }
    if parent.is_some() {
        p.close_answer(&who.key);
    } else {
        answer_ask(p, who, reply, gate, hand_seq);
    }
    p.journal(
        "reply",
        places::card_of(state, &who.place).as_ref().map(|c| c.as_str()),
        json!({"agent": who.key.as_str(), "outcome": reply.outcome.as_str(), "hand": hand_seq}),
    );
}

// Waking only where it reaches an agent, the sender too, whatever its kind: a label wakes nobody,
// an unplaced name its sender alone, for a request.
fn message(
    p: &mut Poster<'_>,
    who: &Who,
    reply: &Reply,
    (m, route, round): (&ReplyMessage, &Route, Option<u32>),
    ctx: Ctx<'_>,
) {
    let from = Sender::Agent(who.key.clone());
    match route {
        Route::Post { place, to } => {
            super::posted(p, (place, from, Some(to)), MessageKind::Update, &m.text, (&m.images, m.kind));
        }
        Route::Agent { place, to, who: target } => {
            let seq =
                super::posted(p, (place, from.clone(), Some(to)), MessageKind::Request, &m.text, (&m.images, m.kind));
            match round.filter(|_| companion(p, who, reply, m, target)) {
                Some(n) => p.wake_for_report(target, &from, Some(n)),
                // A foreman's word reaches its child's running turn (fac-389).
                None if p.deliver(target, &from, seq) => {}
                None => p.wake(target, &from),
            }
        }
        // Posted nowhere, the operator's chat included: he is reached only by a raised hand (fac-390).
        Route::Unplaced { to, why } => {
            let card = places::card_of(p.state, &who.place);
            p.journal(
                "message.unplaced",
                card.as_ref().map(|c| c.as_str()),
                json!({"agent": who.key.as_str(), "to": to, "why": why}),
            );
            // Its sender reads in its next news that it wasn't delivered, and what reaches (fac-148);
            // a request is owed an answer that can't come, so the notice wakes the sender.
            let reach = places::recipients(p.state, who, ctx).join(", ");
            let notice =
                format!("Your message to `{to}` was not delivered: {why}. Names that reach from here: {reach}.");
            let name = who.key.as_str().rsplit('/').next().unwrap_or_default();
            let asked = m.kind == Some(mork_types::reply::Intent::Request);
            let kind = if asked { MessageKind::Request } else { MessageKind::Update };
            p.post(&who.place, Sender::Engine, Some(name), kind, "", &notice);
            if asked {
                p.wake(who, &Sender::Engine);
            }
        }
    }
}

// A reviewer's verdict reply's response or update to its foreman travels with its report, so its wake is that
// report's, covered by the round's verdict (fac-325); a request or an ask never is, a red hand alone is (fac-358).
fn companion(p: &Poster<'_>, who: &Who, reply: &Reply, m: &ReplyMessage, target: &Who) -> bool {
    let h = &reply.hand;
    let final_reply = matches!(reply.outcome, Outcome::ReviewPass | Outcome::ReviewFail);
    let asks = h.ask;
    let parent = p.agent(&who.key).and_then(|a| a.parent);
    final_reply && !asks && m.kind != Some(Intent::Request) && parent.as_ref() == Some(&target.key)
}

// An agent has at most one `answer` ask open: it stays until answered, replaced by this reply's flagged message or
// withdrawn (fac-271); a red hand's headline is raised only when none is pending or withdrawn.
fn answer_ask(p: &mut Poster<'_>, who: &Who, reply: &Reply, gate: bool, seq: u64) {
    let h = &reply.hand;
    let pending = p.all_asks().any(|a| a.kind == AskKind::Answer && a.open && a.agent.as_ref() == Some(&who.key));
    let raised = if h.ask {
        Some((Color::Yellow, h.ask_words()))
    } else if h.color == Color::Red && !gate && !pending && !h.withdraw_ask {
        Some((Color::Red, h.headline.clone()))
    } else {
        None
    };
    if h.ask || h.withdraw_ask {
        p.close_answer(&who.key);
    }
    let Some((color, text)) = raised.filter(|_| !gate) else { return };
    let place = places::asks_room(p.state, who);
    let mut r = super::asks::Raise::new(&place, AskKind::Answer, color, text);
    r.seq = Some(seq);
    r.agent = Some(who.key.clone());
    r.design = super::asks::marked(p.state, who, reply);
    r.choices = (color == Color::Yellow).then(|| h.choices()).flatten();
    super::asks::raise(p, r);
    crate::lifecycle::brief::hand_raised(p, who);
}
