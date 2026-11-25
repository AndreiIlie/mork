use factory_types::ActionId;
use factory_types::action::{Action, ActionStatus};
use factory_types::door::{CALLER_WENT, status};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

/// The running call's public queue facts, without its query, parameters or resolved plan.
#[derive(Debug, Clone)]
pub struct Waiting {
    pub action: ActionId,
    pub requester: String,
    pub op: String,
    pub elapsed_ms: u64,
    pub timeout_ms: u64,
    pub ahead: usize,
}

/// A waiting line names only the blocker and the calls ahead.
pub fn waiting(w: &Waiting) -> String {
    format!(
        "waiting behind {} ({}; {}; running {} ms; time limit {} ms); {} calls wait ahead",
        w.action, w.requester, w.op, w.elapsed_ms, w.timeout_ms, w.ahead,
    )
}

/// A request that exhausted its own queue wait ran nothing and can be asked again.
pub fn busy(id: &ActionId, w: Option<&Waiting>) -> Value {
    let wait = w.map(waiting).unwrap_or_else(|| "waiting for its target to be free".into());
    json!({"door": {"action": id, "status": status::BUSY, "why": format!("{wait}; nothing ran; ask again")}})
}

/// Whether the action was dropped after its final waiting request exhausted its wait.
pub fn was_busy(a: &Action) -> bool {
    a.status == ActionStatus::Dropped
        && a.result.as_ref().and_then(|r| r.get("status")).and_then(Value::as_str) == Some(status::BUSY)
}

/// Drops only an unstarted call: its final waiter timed out or a read's final caller went.
pub fn dropped(state: &State, id: &ActionId, busy: Option<&str>, ctx: Ctx<'_>) -> Decision {
    let Some(mut a) = state.actions.get(id).filter(|a| a.status == ActionStatus::Pending).cloned() else {
        return Decision::default();
    };
    if super::parts(&a).is_none() {
        return Decision::default();
    }
    // A dropped read is settled, never a cancelled pending action for `resume` to re-run (fac-632).
    a.cancelled = false;
    let why = busy.map(str::to_owned).unwrap_or_else(|| format!("{CALLER_WENT} before it ran"));
    if busy.is_some() {
        a.result = Some(json!({"status": status::BUSY, "why": why}));
    }
    let mut p = Poster::for_ctx(state, ctx);
    p.journal("action.dropped", a.what.card().map(|c| c.as_str()), json!({"id": id, "words": why}));
    super::approval::refuse(&mut p, a, &why);
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}
