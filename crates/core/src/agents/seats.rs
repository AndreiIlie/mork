//! A seat's own place and pause (3.3, 4.13.7): the operator's DM with it opened, its turn stopped, resumed, or held when
//! it couldn't run.

use mork_types::reply::Role;
use mork_types::state::{Agent, AgentStatus, Job, Owed, Place, Settlement};
use mork_types::{AgentKey, CardId, JobId};
use serde_json::json;

use super::turns::CUT_NOTE;
use crate::decide::{Ctx, Decision, Intent};
use crate::messages::places;
use crate::post::{Poster, new_agent};
use crate::state::{Change, State};

/// `dm`: opens a DM between the operator and one of a card's agents (3.3).
pub(super) fn dm(state: &State, card: &CardId, r: Role, ctx: Ctx<'_>) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let card = id.as_str();
    if !places::CARD_ROLES.contains(&r) {
        return Decision::refused(format!(
            "no DM with `{}`: a card's DMs are with its foreman or reviewer",
            r.as_str()
        ));
    }
    let who = match places::dm_agent(state, &id, r) {
        Ok(w) => w,
        Err(e) => return Decision::refused(e),
    };
    let mut d = Decision::ok(json!({"dm": {"place": who.place}}));
    if !state.places.contains_key(who.place.as_str()) {
        d.changes.push(Change::PutPlace(Place { id: who.place.clone(), next_number: 1 }));
    }
    if !state.agents.contains_key(&who.key) {
        d.changes.push(Change::PutAgent(new_agent(&who)));
        d.journal("dm", Some(card), json!({"place": who.place, "at": ctx.now}));
    }
    d
}

fn running_job(state: &State, key: &AgentKey) -> Option<JobId> {
    state.jobs.values().find(|j| &j.owner == key && j.settled.is_none() && !j.stop_requested).map(|j| j.id.clone())
}

fn stopping_job<'a>(state: &'a State, key: &AgentKey) -> Option<&'a Job> {
    state.jobs.values().find(|j| &j.owner == key && j.settled.is_none() && j.stop_requested)
}

/// `stop` with an agent: its running turn is stopped (4.13.7).
pub fn stop(state: &State, key: &AgentKey, ctx: Ctx<'_>) -> Decision {
    let Some(agent) = state.agents.get(key) else { return Decision::refused(format!("no agent `{key}`")) };
    let mut d = Decision::ok(json!({"stop": {"agent": key.as_str()}}));
    let mut a = agent.clone();
    if let Some(of) = super::ends::owed_correction(&a) {
        super::ends::give_back(state, &mut a, &of);
    }
    a.owed = None;
    a.not_before = None;
    if let Some(id) = running_job(state, key) {
        if let Some(job) = state.jobs.get(&id) {
            let mut j = job.clone();
            j.stop_requested = true;
            d.changes.push(Change::PutJob(Box::new(j)));
            d.intents.push(Intent::StopJob { id: id.clone() });
            d.journal("stop", None, json!({"agent": key.as_str(), "job": id.as_str()}));
        }
    } else {
        a.status = AgentStatus::Stopped;
        d.journal("stop", None, json!({"agent": key.as_str()}));
    }
    a.paused = true;
    // Through a poster: a child stopped before its turn ran no longer holds its foreman's reports (fac-528).
    let mut p = Poster::for_ctx(state, ctx);
    p.put_agent(a);
    p.finish(&mut d);
    d
}

/// `resume` with an agent: its cut or stopped turn runs again with the cut note (4.13.7); a failed one's agent is
/// owed a fresh turn (fac-509).
pub fn resume(state: &State, key: &AgentKey, ctx: Ctx<'_>) -> Decision {
    let Some(agent) = state.agents.get(key) else { return Decision::refused(format!("no agent `{key}`")) };
    let mut a = agent.clone();
    resumed(state, &mut a);
    let mut d = Decision::ok(json!({"resume": {"agent": key.as_str()}}));
    d.journal("resume", None, json!({"agent": key.as_str()}));
    // A stop can cancel the wake before the foreman has read its children's reports (fac-528).
    let mut p = Poster::for_ctx(state, ctx);
    p.put_agent(a);
    p.finish(&mut d);
    d
}

/// The one resume of an agent, alone or with its card (4.13.2, 4.13.7; M1.9, M1.10, M3.14): a failed turn's runs
/// afresh (fac-509).
pub fn resumed(state: &State, a: &mut Agent) {
    let key = a.key.clone();
    a.paused = false;
    let last = a.last_prompt.clone().unwrap_or_default();
    let previous = last_job(state, &key);
    let stopped_last = previous.and_then(|j| j.settled.as_ref()) == Some(&Settlement::Stopped);
    let stopping = stopping_job(state, &key);
    match &a.status {
        // No prompt kept to run again (version 5 kept none of reply schema 1's): a fresh turn,
        // its news given back.
        AgentStatus::Stopped | AgentStatus::Cut
            if a.last_prompt.is_none() && (stopped_last || matches!(a.status, AgentStatus::Cut)) =>
        {
            fresh(a);
        }
        AgentStatus::Stopped | AgentStatus::Cut if stopped_last || matches!(a.status, AgentStatus::Cut) => {
            a.owed = Some(retry(state, a, format!("{CUT_NOTE}\n\n{last}"), previous));
        }
        // Work still owed (a wake a missing wall held back) runs as it was owed (M1.10).
        AgentStatus::Error { .. } if a.owed.is_some() => {}
        // As a message would: a fresh turn reads today's news once, the failed one's given back (fac-509).
        AgentStatus::Error { .. } => fresh(a),
        // A stop still settling: its turn reruns with the cut note once it has (M1.9), never a closing turn.
        AgentStatus::Working { .. } if stopping.is_some() && a.retiring.is_none() => {
            let prompt =
                super::turns::cut_noted(stopping.map_or(&last, |j| super::turns::request_of(state, j).request_text()));
            a.owed = Some(retry(state, a, prompt, stopping));
        }
        AgentStatus::Stopped
        | AgentStatus::Cut
        | AgentStatus::Idle
        | AgentStatus::Working { .. }
        | AgentStatus::Waiting { .. } => {}
    }
    // Resumed, a usage limit's wait ends now.
    if matches!(a.status, AgentStatus::Waiting { .. }) {
        a.not_before = None;
    }
    if !matches!(a.status, AgentStatus::Working { .. }) {
        a.status = AgentStatus::Idle;
    }
    a.cuts = 0;
}

// Version 5 keeps no prompt to run again: the turn is a fresh one, its news given back.
fn fresh(a: &mut Agent) {
    a.owed = Some(Owed::Wake);
    a.seen = a.seen.min(a.seen_before);
}

// A retry keeps the head and news its turn was asked about (M4.2), correcting nothing; its request is that
// turn's when it runs that request's prompt, its session's opening one (fac-626).
fn retry(state: &State, a: &Agent, prompt: String, job: Option<&Job>) -> Owed {
    // A refused correction's turn runs as the one it corrected, when that turn's prompt is the one run (fac-626).
    let ran = job.map(|j| j.corrects.as_ref().and_then(|c| state.jobs.get(c)).unwrap_or(j));
    let request = ran.filter(|j| j.corrects.is_none()).map(|j| super::turns::request_of(state, j));
    let opening = request.filter(|r| r.opening && prompt.ends_with(r.request_text()));
    let asked = opening.and(ran).or(job);
    Owed::Retry {
        resume: a.session_started,
        corrects: None,
        head: asked.and_then(|j| j.head.clone()),
        seen: asked.and_then(|j| j.seen),
        request: opening.map(|r| r.id.clone()),
        prompt,
    }
}

pub(crate) fn last_job<'s>(state: &'s State, key: &AgentKey) -> Option<&'s Job> {
    state.jobs.values().filter(|j| &j.owner == key).max_by_key(|j| j.spec.created_ms)
}

/// No wall, or a turn not made (fac-146): the agent shows why, paused, its owed turn kept for a
/// message or a resume, so it isn't tried again every tick. A closing turn's seat ends unasked instead.
pub fn held(state: &State, key: &AgentKey, reason: &str, kind: &str, ctx: Ctx<'_>) -> Decision {
    let Some(a) = state.agents.get(key) else { return Decision::default() };
    if a.retiring.is_some() {
        let mut p = Poster::for_ctx(state, ctx);
        super::closing::settled(&mut p, a.clone(), &Settlement::Failed { words: reason.to_owned() });
        let mut d = Decision::default();
        p.finish(&mut d);
        return d;
    }
    if matches!(&a.status, AgentStatus::Error { words } if words == reason) && a.paused {
        return Decision::default();
    }
    let mut a = a.clone();
    a.status = AgentStatus::Error { words: reason.to_owned() };
    a.paused = true;
    let mut d = Decision::default();
    d.journal(kind, None, json!({"agent": key.as_str(), "reason": reason}));
    // Through a poster: a child held so no longer holds its foreman's reports (fac-528).
    let mut p = Poster::for_ctx(state, ctx);
    p.put_agent(a);
    p.finish(&mut d);
    d
}
