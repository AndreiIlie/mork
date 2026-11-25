//! Send now (fac-78): a message cuts its recipient's running turn, and once that turn has settled
//! its session continues with the message. The card and its other agents run on.

use factory_types::AgentKey;
use factory_types::state::{Agent, AgentStatus, Job, Owed, Settlement};
use serde_json::json;

use crate::decide::Intent;
use crate::post::Poster;
use crate::state::{Change, State};

// What the turn after an interrupted one starts with.
const INTERRUPT_NOTE: &str = "Your previous turn was interrupted by a new message before it finished: what it ran and edited stands, its reply was dropped, and its news are given again below. Before redoing anything outward, check whether it already happened.";

/// `say --delivery now` reached `key`: its running turn is stopped for the continuation. An agent
/// with no turn running, or paused, has the message as an ordinary one.
pub(super) fn cut(p: &mut Poster<'_>, key: &AgentKey) {
    if p.agent(key).is_none_or(|a| a.paused) {
        return;
    }
    let Some(job) = p.state.jobs.values().find(|j| &j.owner == key && j.purpose.is_none() && running(j)) else {
        return;
    };
    let mut j = job.clone();
    j.stop_requested = true;
    j.interrupted = true;
    p.intent(Intent::StopJob { id: j.id.clone() });
    p.journal("interrupt", None, json!({"agent": key.as_str(), "job": j.id.as_str()}));
    p.other.push(Change::PutJob(Box::new(j)));
}

fn running(j: &Job) -> bool {
    j.settled.is_none() && !j.stop_requested
}

/// A stopped turn settles: an interrupted one's agent, unless paused since, is owed its next turn
/// with that turn's news given back; any other is stopped (4.13.7).
pub fn stopped(a: &mut Agent, job: &Job) -> Settlement {
    if job.interrupted && !a.paused {
        a.status = AgentStatus::Idle;
        a.seen = a.seen.min(a.seen_before);
        a.owed = Some(Owed::Wake);
    } else {
        a.status = AgentStatus::Stopped;
    }
    Settlement::Stopped
}

/// A woken turn's prompt, the interrupt note first when the agent's last turn was interrupted.
pub fn noted(state: &State, a: &Agent, prompt: String) -> String {
    let last =
        state.jobs.values().filter(|j| j.owner == a.key && j.purpose.is_none()).max_by_key(|j| j.spec.created_ms);
    if last.is_some_and(|j| j.interrupted && j.settled == Some(Settlement::Stopped)) {
        format!("{INTERRUPT_NOTE}\n\n{prompt}")
    } else {
        prompt
    }
}
