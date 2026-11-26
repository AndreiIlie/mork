//! A door request arriving resolved (fac-1, fac-124): the same one asked again is answered by the first,
//! an allowed one is put to run, and an outside one raises the operator's ask.

use mork_types::ActionId;
use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::door::{CALLER_WENT, WAITING, status};
use mork_types::door_call::{DoorCall, DoorState, Phase, Verdict};
use mork_types::door_plan::Session;
use mork_types::state::Sender;
use serde_json::json;

use super::approval::raise;
use super::policy::ALLOWED;
use super::reads::running;
use super::{answer, parts, settled, was_cut};
use crate::decide::{Ctx, Decision, Intent};
use crate::post::Poster;
use crate::state::State;

/// A resolved request deduplicates, dispatches when allowed, or raises its approval ask.
pub fn asked(state: &State, call: &DoorCall, validator: &Verdict, stopped: &[ActionId], ctx: Ctx<'_>) -> Decision {
    if let Some((id, s)) = running(state, call, ctx.now) {
        return refused_running(call, &id, s);
    }
    if let Some(a) = same(state, call, stopped) {
        return Decision::ok(answer(&a));
    }
    let mut p = Poster::for_ctx(state, ctx);
    let what = ActionWhat::Door(Box::new(call.clone()));
    let id = p.next_action_id();
    let base = what.key();
    let taken = p.all_actions().any(|a| a.key == base) || p.state.unloaded.actions.contains_key(&base);
    let key = if taken { format!("{base}@{id}") } else { base };
    let allowed = validator.verdict == ALLOWED;
    let mut a = Action {
        id: id.clone(),
        what,
        key,
        status: if allowed { ActionStatus::Pending } else { ActionStatus::Held },
        result: None,
        words: (!allowed).then(|| WAITING.into()),
        attempts: 0,
        created_ms: ctx.now,
        done_ms: None,
        cancelled: false,
        door: Some(Box::new(DoorState {
            phase: if allowed { Phase::Cleared } else { Phase::Asked },
            validator: validator.clone(),
            checker: None,
            ask: None,
            approval: None,
            check_cost: None,
            told: None,
            running: None,
        })),
        unwritten: None,
        withdrawn: false,
    };
    let card = call.card.as_ref().map(|c| c.as_str());
    p.journal("action", card, json!({"id": id, "key": a.key}));
    p.journal(
        "door.asked",
        card,
        json!({"id": id, "from": call.from, "target": call.target, "op": call.op, "allowlist": validator}),
    );
    if allowed {
        p.intent(Intent::RunAction { id });
    } else {
        let ask = raise(&mut p, &a, call, validator, &validator.reason);
        if let Some(d) = a.door.as_mut() {
            d.ask = Some(ask);
        }
    }
    let answer = answer(&a);
    p.put_action(a);
    let mut d = Decision::ok(answer);
    p.finish(&mut d);
    d
}

/// A request refused before any action (`unsupported`, `not allowed`): journaled, answered.
pub fn refused(from: &Sender, target: &str, op: &str, word: &str, why: &str) -> Decision {
    let mut d = Decision::ok(json!({"door": {"status": word, "why": why}}));
    d.journal("door.refused", None, json!({"from": from, "target": target, "op": op, "status": word, "why": why}));
    d
}

// Refused while its read before may still run: asking again retried its kill, which wasn't taken.
fn refused_running(call: &DoorCall, id: &ActionId, s: Session) -> Decision {
    let why = format!(
        "this query, asked before as {id}, may still be running on {} (connection {}): its server didn't take \
         its kill. Asking it again tries the kill again; it runs anew once the kill is taken, or after {} UTC, \
         when the server's own time limit has ended it",
        call.target,
        s.session,
        // To the minute after: by then it has passed.
        crate::board::writes::date(s.until_ms + 59_999),
    );
    refused(&call.from, &call.target, &call.op, status::NOT_ALLOWED, &why)
}

// A read stopped because its caller went, or being stopped: it answers no one else (fac-183).
fn caller_went(a: &Action, stopped: &[ActionId]) -> bool {
    stopped.contains(&a.id)
        || a.result.as_ref().is_some_and(|r| r["caller_went"] == true)
        || a.words.as_deref().is_some_and(|w| w.starts_with(CALLER_WENT))
}

// The same request in the same turn, or its sender's unresolved one, but a cut read (fac-184).
// A file.get's file is in its job's io/, outside any other's wall: only that job shares it.
fn same(state: &State, call: &DoorCall, stopped: &[ActionId]) -> Option<Action> {
    let alike = |a: &Action| {
        !caller_went(a, stopped)
            && !super::queue::was_busy(a)
            && parts(a).is_some_and(|(c, _)| {
                super::alike(c, call)
                    && (call.op != "file.get" || c.job == call.job)
                    && ((call.turn.is_some() && c.turn == call.turn) || !settled(a))
                    && !was_cut(a)
            })
    };
    state.actions.values().find(|a| alike(a)).cloned()
}
