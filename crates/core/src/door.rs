//! The door (fac-1, fac-124): a request resolves on its target's allowlist; an allowed call runs, an outside one
//! waits for Andrei's once/no bound to its exact plan, and how a call stands is answered and told once.

pub mod approval;
pub mod asking;
mod lex;
mod mongo;
pub mod policy;
pub mod queue;
pub mod reads;
pub mod sql;
pub mod target;

use factory_types::action::{Action, ActionStatus, ActionWhat};
use factory_types::door::{WAITING, status};
use factory_types::door_call::{DoorCall, DoorState, Phase};
use factory_types::state::{MessageKind, Sender};
use factory_types::{ActionId, Command};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::{Change, State};

/// `door.once` or `door.no`, past the buzz.
pub fn command(state: &State, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::DoorOnce { ask } = cmd {
        approval::once(state, ask, ctx)
    } else if let Command::DoorNo { ask } = cmd {
        approval::no(state, ask, ctx)
    } else {
        Decision::refused(format!("`{}` isn't a door ask's command", cmd.name()))
    }
}

pub fn parts(a: &Action) -> Option<(&DoorCall, &DoorState)> {
    if let (ActionWhat::Door(c), Some(d)) = (&a.what, &a.door) { Some((c, d)) } else { None }
}

fn settled(a: &Action) -> bool {
    matches!(a.status, ActionStatus::Done | ActionStatus::Dropped)
}

// A read that ended unclear, as `failed` with "cut, ask again".
fn was_cut(a: &Action) -> bool {
    a.status == ActionStatus::Done
        && a.result.as_ref().and_then(|r| r.get("cut")).and_then(Value::as_bool) == Some(true)
}

// The same request from the same sender: asked again, it is answered by the first or waits for it (fac-184).
fn alike(c: &DoorCall, call: &DoorCall) -> bool {
    c.from == call.from && c.request() == call.request()
}

/// The door's answer for a call as it stands now.
pub fn answer(a: &Action) -> Value {
    let Some((_, d)) = parts(a) else { return json!({"door": {"status": status::UNDER_WAY}}) };
    let mut v = match (a.status, d.phase) {
        (ActionStatus::Done, _) => a.result.clone().unwrap_or_else(|| json!({"status": status::DONE})),
        (ActionStatus::Held, Phase::Asked) => {
            json!({"status": status::ASK_RAISED, "ask": d.ask, "words": WAITING})
        }
        (ActionStatus::Pending | ActionStatus::Started, _) => {
            json!({"status": status::UNDER_WAY})
        }
        (ActionStatus::Held, _) => json!({"status": status::HELD, "words": a.words}),
        (ActionStatus::Dropped, _) if queue::was_busy(a) => {
            a.result.clone().unwrap_or_else(|| json!({"status": status::BUSY, "why": a.words}))
        }
        (ActionStatus::Dropped, _) => json!({"status": status::NOT_ALLOWED, "why": a.words}),
    };
    // A result marked done by hand (`action.done`) says so.
    if let Some(note) = v.get("by_hand").cloned() {
        v = json!({"status": status::DONE, "text": format!("marked done by Andrei: {}", note.as_str().unwrap_or(""))});
    }
    if let Some(o) = v.as_object_mut() {
        o.insert("action".into(), json!(a.id));
    }
    json!({"door": v})
}

pub fn status_of(a: &Action) -> String {
    answer(a).pointer("/door/status").and_then(Value::as_str).unwrap_or(status::UNDER_WAY).to_owned()
}

/// Whether its requester is owed news of it: only approval-gated calls and held calls (or a held
/// call settled by Andrei), once settled or held and not yet told that status.
pub fn owed_news(a: &Action) -> bool {
    let Some((_, d)) = parts(a) else { return false };
    let held = a.status == ActionStatus::Held && d.phase != Phase::Asked;
    let needs_news = d.ask.is_some()
        || held
        || d.told.as_deref() == Some(status::HELD)
        || a.result.as_ref().and_then(|r| r.get("by_hand")).is_some()
        || (d.phase == Phase::Refused && a.words.as_deref() == Some(crate::decide::actions::DROPPED_BY_ANDREI));
    needs_news && (settled(a) || held) && d.told.as_deref() != Some(status_of(a).as_str())
}

/// The requester was told how its call stands: by its answer, or now by news.
pub fn delivered(state: &State, id: &str, answered: bool, ctx: Ctx<'_>) -> Decision {
    let Some(mut a) = state.actions.get(id).cloned() else { return Decision::default() };
    let Some(call) = parts(&a).map(|(c, _)| c.clone()) else { return Decision::default() };
    let word = status_of(&a);
    let mut p = Poster::for_ctx(state, ctx);
    if !answered {
        news(&mut p, &a, &call, &word);
    }
    if let Some(d) = a.door.as_mut() {
        d.told = Some(word.clone());
    }
    p.journal(
        "door.told",
        call.card.as_ref().map(|c| c.as_str()),
        json!({"id": id, "status": word, "answered": answered}),
    );
    p.put_action(a);
    let mut dec = Decision::default();
    p.finish(&mut dec);
    dec
}

// It wakes the requester: the outcome is its next turn's news.
fn news(p: &mut Poster<'_>, a: &Action, call: &DoorCall, word: &str) {
    let headline = format!("door: {} on {}: {word}", call.op, call.target);
    let text = serde_json::to_string_pretty(&answer(a)).unwrap_or_default();
    match &call.from {
        Sender::Agent(k) => {
            let Some(who) = crate::messages::places::who_of(p.state, k) else { return };
            p.post(who.place.as_str(), Sender::Engine, Some(who.role.as_str()), MessageKind::Request, &headline, &text);
            p.wake(&who, &Sender::Engine);
        }
        Sender::Andrei | Sender::Engine | Sender::Orchestrator(_) | Sender::Pa => {
            p.post(call.place.as_str(), Sender::Engine, Some("andrei"), MessageKind::Engine, &headline, &text);
        }
    }
}

/// The changes a decision makes to door actions, by id: what the engine looks at after it.
pub fn touched(changes: &[Change]) -> Vec<ActionId> {
    changes
        .iter()
        .filter_map(|c| if let Change::PutAction(a) = c { Some(a) } else { None })
        .filter(|a| matches!(a.what, ActionWhat::Door(_)))
        .map(|a| a.id.clone())
        .collect()
}
