//! Door reads whose outcome is unclear (fac-184): settled `failed`, never held, and a query that may still
//! run on its server refuses the same one until its kill is taken or its time limit has passed.

use mork_types::ActionId;
use mork_types::action::{Action, ActionWhat};
use mork_types::door::{CUT, NEVER_BLIND, is_read, status};
use mork_types::door_call::DoorCall;
use mork_types::door_plan::Session;
use mork_types::state::{MessageKind, Sender};
use serde_json::{Value, json};

use super::parts;
use crate::decide::{Ctx, Decision};
use crate::messages::places::MAIN;
use crate::post::Poster;
use crate::state::State;

/// A door read, whose outcome unclear fails it rather than holding it.
pub fn read(a: &Action) -> bool {
    matches!(&a.what, ActionWhat::Door(c) if is_read(&c.op))
}

/// Settled `failed`, never held: asking again doesn't find it, so asking again runs anew. One whose query
/// may still run on its server says where, and the same query waits for it to end (fac-184).
pub fn cut(p: &mut Poster<'_>, mut a: Action, words: &str, running: Option<Session>, ctx: Ctx<'_>) {
    let why = words.replace(NEVER_BLIND, "its outcome is unknown");
    let text = match (&a.what, running) {
        (ActionWhat::Door(c), Some(s)) => format!(
            "{CUT}: its query may still be running on {} (connection {}), and the same query is refused until \
             it has ended there: {why}",
            c.target, s.session
        ),
        _ => format!("{CUT}: {why}"),
    };
    if let Some(s) = running {
        unstopped(p, &mut a, s);
    }
    crate::decide::actions::done(p, a, json!({"status": status::FAILED, "text": text, "cut": true}), ctx);
}

/// The same request's read from the same sender whose query may still run on its server (fac-184):
/// its action and session, until its kill is taken or the session's own time limit has passed.
pub fn running(state: &State, call: &DoorCall, now: i64) -> Option<(ActionId, Session)> {
    state.actions.values().find_map(|a| {
        let (c, d) = parts(a)?;
        let s = d.running.filter(|s| now < s.until_ms)?;
        super::alike(c, call).then(|| (a.id.clone(), s))
    })
}

/// A cut read whose server didn't take the kill of its query: it may still run there, on `s`. The
/// same query is refused until it has ended, and the operator is told once, with nothing to answer.
pub fn unstopped(p: &mut Poster<'_>, a: &mut Action, s: Session) {
    let ActionWhat::Door(call) = &a.what else { return };
    let call = call.clone();
    let Some(d) = a.door.as_mut() else { return };
    d.running = Some(s);
    let query = serde_json::from_str::<Value>(&call.params)
        .ok()
        .and_then(|v| v.get("query").and_then(Value::as_str).map(|q| q.trim().lines().next().unwrap_or("").to_owned()))
        .unwrap_or_default();
    let text = format!(
        "{}: connection {} didn't take the kill of a read's query, which may still run there: {query}",
        call.target, s.session
    );
    p.post(MAIN, Sender::Engine, Some("operator"), MessageKind::Engine, "", &text);
    p.journal(
        "door.unstopped",
        call.card.as_ref().map(|c| c.as_str()),
        json!({"id": a.id, "target": call.target, "session": s}),
    );
}

/// The kill of an unstopped read's query, tried again (fac-184): ended, the same query runs anew.
pub fn killed(state: &State, id: &str, ended: bool, ctx: Ctx<'_>) -> Decision {
    let Some(mut a) = state.actions.get(id).cloned() else { return Decision::default() };
    let Some((call, d)) = parts(&a).map(|(c, d)| (c.clone(), d.clone())) else { return Decision::default() };
    let Some(s) = d.running else { return Decision::default() };
    let mut p = Poster::for_ctx(state, ctx);
    p.journal(
        "door.killed",
        call.card.as_ref().map(|c| c.as_str()),
        json!({"id": id, "target": call.target, "session": s.session, "ended": ended}),
    );
    if ended {
        if let Some(d) = a.door.as_mut() {
            d.running = None;
        }
        p.put_action(a);
    }
    let mut dec = Decision::default();
    p.finish(&mut dec);
    dec
}
