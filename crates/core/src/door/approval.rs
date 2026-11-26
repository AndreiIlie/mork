//! The operator's once/no on an outside door call (fac-1, fac-124): approval binds to the exact plan and is
//! consumed by the dispatch it starts; a call is never run again, and one that never ran is never done.

use mork_types::AskId;
use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::ask::{AskKind, Color};
use mork_types::door_call::{Approval, DoorCall, Phase, Verdict};
use mork_types::state::Sender;
use serde_json::{Value, json};

use super::parts;
use super::policy::ALLOWED;
use crate::decide::{Ctx, Decision, Intent};
use crate::messages::asks::{self, Raise};
use crate::post::Poster;
use crate::state::State;

/// The place for an approval or held ask, keeping the requester in its own place for the result.
pub(crate) fn ask_place<'a>(state: &State, call: &'a DoorCall) -> &'a str {
    call.card
        .as_ref()
        .filter(|id| state.cards.get(*id).is_some_and(crate::messages::places::two_rooms))
        .map_or(call.place.as_str(), |id| id.as_str())
}

pub(super) fn raise(p: &mut Poster<'_>, a: &Action, call: &DoorCall, validator: &Verdict, reason: &str) -> AskId {
    let text = format!("{} wants {} on {}: {reason}", call.from.label(), call.op, call.target);
    let mut r = Raise::new(ask_place(p.state, call), AskKind::Door, Color::Yellow, text);
    r.detail = format!(
        "allowlist: {} — {}\nparams: {}\nruns: {}\n/door.once {{ask}} runs it once; /door.no refuses it.",
        validator.verdict, validator.reason, call.params, call.plan
    );
    r.agent = match &call.from {
        Sender::Agent(k) => Some(k.clone()),
        Sender::Operator | Sender::Engine | Sender::Orchestrator(_) | Sender::Pa => None,
    };
    r.action = Some(a.id.clone());
    let id = asks::raise(p, r);
    // The ask names itself in its own detail.
    if let Some(mut x) = p.get_ask(&id) {
        x.detail = x.detail.replace("{ask}", &id);
        p.put_ask(x);
    }
    id
}

fn open_ask(state: &State, ask: &str) -> Result<Action, Decision> {
    let a = state.asks.get(ask).filter(|a| a.open && a.kind == AskKind::Door);
    let a = a.ok_or_else(|| Decision::refused(format!("no open door ask `{ask}`")))?;
    let act = a.action.as_ref().and_then(|id| state.actions.get(id.as_str())).cloned();
    act.filter(|x| parts(x).is_some_and(|(_, d)| d.phase == Phase::Asked))
        .ok_or_else(|| Decision::refused(format!("`{ask}` waits for nothing")))
}

/// `door.once`: the operator's approval of this call, bound to its plan, for one dispatch.
pub fn once(state: &State, ask: &str, ctx: Ctx<'_>) -> Decision {
    let mut a = match open_ask(state, ask) {
        Ok(a) => a,
        Err(d) => return d,
    };
    let Some((call, d)) = parts(&a).map(|(c, d)| (c.clone(), d.clone())) else { return Decision::default() };
    let mut p = Poster::for_ctx(state, ctx);
    asks::close_one(&mut p, ask);
    let mut door = d;
    door.phase = Phase::Approved;
    door.approval = Some(Approval { at_ms: ctx.now, plan: call.plan.clone(), consumed_ms: None });
    a.door = Some(Box::new(door));
    a.status = ActionStatus::Pending;
    a.words = None;
    let id = a.id.clone();
    p.journal("door.once", call.card.as_ref().map(|c| c.as_str()), json!({"id": id, "ask": ask}));
    p.put_action(a);
    p.intent(Intent::RunAction { id: id.clone() });
    let mut dec = Decision::ok(json!({"door.once": {"ask": ask, "action": id}}));
    p.finish(&mut dec);
    dec
}

/// `door.no`: refused; its requester is told.
pub fn no(state: &State, ask: &str, ctx: Ctx<'_>) -> Decision {
    let a = match open_ask(state, ask) {
        Ok(a) => a,
        Err(d) => return d,
    };
    let mut p = Poster::for_ctx(state, ctx);
    asks::close_one(&mut p, ask);
    let id = a.id.clone();
    refuse(&mut p, a, "The operator said no");
    p.journal("door.no", None, json!({"id": id, "ask": ask}));
    let mut dec = Decision::ok(json!({"door.no": {"ask": ask, "action": id}}));
    p.finish(&mut dec);
    dec
}

/// A call refused: dropped with why, nothing run.
pub fn refuse(p: &mut Poster<'_>, mut a: Action, why: &str) {
    if let Some(d) = a.door.as_mut() {
        d.phase = Phase::Refused;
    }
    a.status = ActionStatus::Dropped;
    a.words = Some(why.to_owned());
    a.done_ms = Some(p.now);
    p.put_action(a);
}

/// Revalidates the exact plan and allowlist, then permits clearance or consumes an unused once.
/// Runs in the transaction that starts dispatch.
pub fn start_guard(a: &mut Action, prepared: Option<&Value>, now: i64) -> Result<(), String> {
    let ActionWhat::Door(call) = &a.what else { return Ok(()) };
    let now_plan = prepared.and_then(|v| v.get("plan")).and_then(Value::as_str);
    if now_plan != Some(call.plan.as_str()) {
        let why = prepared.and_then(|v| v.get("why")).and_then(Value::as_str);
        return Err(why.unwrap_or("the target's configuration changed since it was checked: ask again").to_owned());
    }
    let door = a.door.as_mut().ok_or("the call has no authorization state")?;
    let now_verdict: Option<Verdict> =
        prepared.and_then(|v| v.get("verdict")).and_then(|v| serde_json::from_value(v.clone()).ok());
    // A changed allowlist cannot reuse the previous clearance or approval.
    if now_verdict.as_ref() != Some(&door.validator) {
        return Err("the target's allowlist changed since it was checked: ask again".into());
    }
    match door.phase {
        Phase::Cleared if door.validator.verdict == ALLOWED => Ok(()),
        Phase::Approved => match door.approval.as_mut() {
            Some(ap) if ap.consumed_ms.is_none() && ap.plan == call.plan => {
                ap.consumed_ms = Some(now);
                Ok(())
            }
            Some(_) => Err("The operator's once was used already: it runs once".into()),
            None => Err("it has no approval".into()),
        },
        Phase::Cleared | Phase::Checking | Phase::Asked | Phase::Refused => Err("it isn't cleared to run".into()),
    }
}

/// A started call found not dispatched after a crash, on the dispatcher's or the engine's
/// durable word: the operator's once wasn't used, and serves the one dispatch still to come.
pub fn not_dispatched(a: &mut Action) {
    if let Some(ap) = a.door.as_mut().and_then(|d| d.approval.as_mut()) {
        ap.consumed_ms = None;
    }
}

/// A door call is never run again, and one that never ran isn't marked done.
pub fn refusal(a: &Action, how: &str) -> Option<String> {
    let (_, d) = parts(a)?;
    let never_ran = d.phase == Phase::Asked;
    match how {
        "retry" => Some(format!(
            "{} is a door call: it is never run again; its requester asks again, or the operator answers its ask",
            a.id
        )),
        "done" if never_ran => Some(format!("{} never ran: answer its ask with door.once or door.no", a.id)),
        _ => None,
    }
}
