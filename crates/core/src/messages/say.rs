//! `say` (3.4.5): Andrei's or a seat's words, posted where they reach and waking the agent they address.

use factory_types::Command;
use factory_types::ask::AskKind;
use factory_types::command::Delivery;
use factory_types::reply::{Intent, Role};
use factory_types::state::{MessageKind, Sender};
use serde_json::json;

use super::places::{self, MAIN};
use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

/// `say`: a request to the agent it addresses, which wakes it (3.4.5). A seat's `say` is the
/// same as Andrei's; its kind, which the buzz asks of a seat (fac-18), is kept with the message.
pub(super) fn say(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    let Command::Say { place, to, text, images, kind, delivery, choice } = cmd else { return Decision::default() };
    let (place, to, images, kind) = (place.as_str(), to.as_deref(), images.as_deref().unwrap_or_default(), *kind);
    let (delivery, choice) = (*delivery, *choice);
    if choice.is_some() && !(*from == Sender::Andrei || from.is_orchestrator()) {
        return Decision::refused("only Andrei or an orchestrator answers with a choice");
    }
    // The window's image-only message is a message (fac-321).
    if text.trim().is_empty() && images.is_empty() {
        return Decision::invalid("`text` is empty: a message needs words or an image");
    }
    if let Some(e) = to_andrei(state, from, place, to.or_else(|| places::addressee(text))) {
        return Decision::refused(e);
    }
    if place == MAIN && to.is_none() && places::addressee(text).is_none() && choice.is_none() {
        return unwoken(state, from, (MAIN, None, MessageKind::Request), text, images, kind, ctx);
    }
    if let Some((post_place, label)) = to_orchestrator(state, from, to, ctx) {
        return unwoken(state, from, (&post_place, Some(&label), MessageKind::Update), text, images, kind, ctx);
    }
    let (place, who) = match seat_routed(state, from, place, to.or_else(|| places::addressee(text)), ctx) {
        Some(Ok(routed)) => routed,
        Some(Err(e)) => return Decision::refused(e),
        None => match places::addressed(state, place, to, text, ctx) {
            // A child named in the room is posted to in its own DM (fac-191).
            Ok(w) => (w.place.as_str().to_owned(), w),
            Err(e) => return Decision::refused(e),
        },
    };
    if !places::planner_sender(state, from, &who) {
        return Decision::refused("the planner reopens for its foreman, Andrei or an orchestrator");
    }
    // Said unnamed into the other seat's room or DM, it is still their exchange (fac-633).
    let place = exchange(state, from, &who).unwrap_or(place);
    if delivery == Some(Delivery::Now)
        && let Some(e) = crate::decide::buzz::stopping(state, from, Some(&who.key), true)
    {
        return crate::decide::buzz::buzzed(from, cmd, e);
    }
    if let Err(e) = super::images::allowed(state, from, images).and_then(|()| chosen(state, from, &who, choice)) {
        return Decision::refused(e);
    }
    let label = label(state, &place, &who);
    let mut p = Poster::for_ctx(state, ctx);
    let seq = super::posted(&mut p, (&place, from.clone(), Some(&label)), MessageKind::Request, text, (images, kind));
    if let Some(m) = p.last_message().filter(|_| choice.is_some()) {
        m.choice = choice;
    }
    // His words are only ever a message: no engine question is answered by reading them (fac-335).
    if delivery.is_some() || !p.deliver(&who, from, seq) {
        p.wake(&who, from);
    }
    if delivery == Some(Delivery::Now) {
        super::interrupts::cut(&mut p, &who.key);
    }
    heard(&mut p, from, &who, kind);
    let mut d = Decision::ok(json!({"say": {"seq": seq, "to": who.key.as_str()}}));
    p.finish(&mut d);
    d
}

// In the challenge place a DM's seat is named whole, so the room's foreman never reads its words (fac-633).
fn label(state: &State, place: &str, who: &places::Who) -> String {
    let to_dm = places::card_of(state, place).is_some_and(|c| place == places::challenge_place(&c))
        && matches!(places::kind(state, &who.place), places::PlaceKind::Dm(..));
    let label = if to_dm { who.key.as_str() } else { who.key.as_str().rsplit('/').find(|p| *p != "dm").unwrap_or("") };
    label.to_owned()
}

// An answer's chosen option names one the addressed seat's open ask offers (fac-696): an orchestrator's, one the
// ask given to it offers, as its message answers only that.
fn chosen(state: &State, from: &Sender, who: &places::Who, choice: Option<u32>) -> Result<(), String> {
    let Some(n) = choice else { return Ok(()) };
    let offers = |c: &factory_types::ask::Choices| usize::try_from(n).is_ok_and(|n| (1..=c.options.len()).contains(&n));
    let open = state.asks.values().filter(|a| {
        a.open
            && a.kind == AskKind::Answer
            && a.agent.as_ref() == Some(&who.key)
            && (a.given || !from.is_orchestrator())
    });
    if open.filter_map(|a| a.choices.as_ref()).any(offers) {
        Ok(())
    } else {
        let whose = if from.is_orchestrator() { "open ask given you" } else { "open ask" };
        Err(format!("{}'s {whose} offers no option {n}", who.key))
    }
}

// A seat's words for Andrei, its plain words in main or a name that once posted for him, are refused (fac-390).
fn to_andrei(state: &State, from: &Sender, place: &str, named: Option<&str>) -> Option<String> {
    let Sender::Agent(key) = from else { return None };
    if !named.map_or(place == MAIN, places::for_andrei) {
        return None;
    }
    let child = places::who_of(state, key)
        .is_some_and(|me| matches!(places::kind(state, &me.place), places::PlaceKind::Child(..)));
    Some(places::hand_only(child))
}

// A seat's `say --to orchestrator` is placed as its reply's message to it is: posted where it is, waking nobody itself.
fn to_orchestrator(state: &State, from: &Sender, to: Option<&str>, ctx: Ctx<'_>) -> Option<(String, String)> {
    let Sender::Agent(key) = from else { return None };
    let me = places::who_of(state, key)?;
    match places::route(state, &me, to?, ctx) {
        places::Route::Post { place, to } if to == "orchestrator" => Some((place, to)),
        places::Route::Post { .. } | places::Route::Agent { .. } | places::Route::Unplaced { .. } => None,
    }
}

// A seat's word to its parent, or to its card's other seat, is placed as its reply's message is: a child's in its
// parent's room, which its parent reads (fac-255); the foreman's and planner's in the challenge place (fac-633).
fn seat_routed(
    state: &State,
    from: &Sender,
    place: &str,
    to: Option<&str>,
    ctx: Ctx<'_>,
) -> Option<Result<(String, places::Who), String>> {
    let Sender::Agent(key) = from else { return None };
    let me = places::who_of(state, key)?;
    let to = to?;
    let card = places::card_of(state, &me.place)?;
    let child = matches!(places::kind(state, place), places::PlaceKind::Child(_, k) if k == *key)
        && matches!(to, "foreman" | "planner");
    let routed = places::route(state, &me, to, ctx);
    match routed {
        places::Route::Agent { place: at, who, .. }
            if child || (at == places::challenge_place(&card) && places::card_of(state, place) == Some(card)) =>
        {
            Some(Ok((at, who)))
        }
        places::Route::Post { to, .. } if child => Some(Err(format!("`{to}` isn't the card's foreman"))),
        places::Route::Unplaced { why, .. } if child => Some(Err(why)),
        places::Route::Agent { .. } | places::Route::Post { .. } | places::Route::Unplaced { .. } => None,
    }
}

// The challenge place, when a full card's foreman (its room's or DM's) and its planner are the sender and recipient.
fn exchange(state: &State, from: &Sender, who: &places::Who) -> Option<String> {
    let Sender::Agent(key) = from else { return None };
    let me = places::who_of(state, key)?;
    let card = places::card_of(state, &me.place)?;
    let pair = matches!((me.role, who.role), (Role::Foreman, Role::Planner) | (Role::Planner, Role::Foreman));
    let seat = |w: &places::Who| {
        matches!(
            places::kind(state, &w.place),
            places::PlaceKind::Room(_) | places::PlaceKind::Spec(_) | places::PlaceKind::Dm(..)
        )
    };
    let theirs = places::card_of(state, &who.place).as_ref() == Some(&card);
    let two = state.cards.get(&card).is_some_and(places::two_rooms);
    (pair && theirs && two && seat(&me) && seat(who)).then(|| places::challenge_place(&card))
}

// Posted, waking nobody: plain words in main (fac-119), or a seat's to its orchestrator.
fn unwoken(
    state: &State,
    from: &Sender,
    (place, to, kind): (&str, Option<&str>, MessageKind),
    text: &str,
    images: &[String],
    intent: Option<Intent>,
    ctx: Ctx<'_>,
) -> Decision {
    if let Err(e) = super::images::allowed(state, from, images) {
        return Decision::refused(e);
    }
    let mut p = Poster::for_ctx(state, ctx);
    let seq = super::posted(&mut p, (place, from.clone(), to), kind, text, (images, intent));
    let mut d = Decision::ok(json!({"say": {"seq": seq, "to": to}}));
    p.finish(&mut d);
    d
}

fn heard(p: &mut Poster<'_>, from: &Sender, who: &places::Who, kind: Option<Intent>) {
    let Some(card) = places::card_of(p.state, &who.place) else { return };
    if !(*from == Sender::Andrei || from.is_orchestrator()) {
        return;
    }
    // Only words in the holder's own room withdraw its recap: the build room's withdraw nothing (fac-633).
    if who.place.as_str() == places::room_of(&card, who.role)
        && let Some(mut c) = p.card(&card).filter(|c| {
            who.role == crate::lifecycle::facts::holder(c)
                && crate::lifecycle::brief::withdraws(p.state, c, who, from, kind)
        })
        && crate::lifecycle::brief::redraft(p, &mut c)
    {
        p.put_card(c);
    }
}
