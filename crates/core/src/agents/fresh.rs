//! `fresh` (fac-25): an agent's session ended and a new one started in its stead, at its next
//! turn, with the startup's material and a line naming the session it replaces.

use mork_types::state::{Agent, Fresh, Owed, Sender};
use mork_types::{AgentKey, SessionId};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::messages::places::{self, MAIN};
use crate::post::Poster;
use crate::state::{Change, State};

/// Consumes a fix or join only on the wake that reads it, after any in-flight turn settles.
pub(super) fn at_boundary(state: &State, a: &mut Agent, ctx: Ctx<'_>) {
    if !matches!(a.owed, Some(Owed::Wake)) {
        return;
    }
    let Some(seq) = a.fix_or_join.take() else { return };
    if seq < a.seen || !super::prompt::resumes(a) {
        return;
    }
    let Some(card) = state.cards.get(a.place.as_str()) else { return };
    if card.mode != mork_types::card::Mode::Full {
        return;
    }
    if a.ran.compacted || above_threshold(ctx.config, a) {
        let fresh = Fresh {
            replaces: a.session.clone(),
            by: Sender::Engine.label(),
            at: ctx.now,
            pending: false,
            vendor: None,
        };
        replace(state, a, fresh);
    }
}

fn above_threshold(config: &mork_types::config::Config, a: &Agent) -> bool {
    let percent = config.mork.fresh_context_percent;
    let Some(context) = super::costs::stored_context(config, a) else { return false };
    percent > 0
        && context
            .window
            .is_some_and(|window| u128::from(context.used) * 100 > u128::from(window) * u128::from(percent))
}

/// `fresh <agent>`: a key (`ex-550/foreman`), or a name for main's agent (`echo`), whoever
/// sends it (3.6.3). A turn running ends in the old session; the next starts the new one.
pub(super) fn fresh(state: &State, from: &Sender, name: &str, ctx: Ctx<'_>) -> Decision {
    let name = name.trim();
    if name.is_empty() {
        return Decision::refused("name the agent: `fresh ex-550/foreman`");
    }
    let key = if name.contains('/') { AgentKey::new(name) } else { AgentKey::new(format!("{MAIN}/{name}")) };
    let Some(agent) = state.agents.get(&key) else {
        let hint = if name.contains('/') { String::new() } else { format!(": a card's agent is `<card>/{name}`") };
        return Decision::refused(format!("no agent `{key}`{hint}"));
    };
    if agent.session.is_none() && !agent.session_started {
        return Decision::refused(format!("`{key}` has no session to replace"));
    }
    let running = state.jobs.values().any(|j| j.owner == key && j.settled.is_none());
    let by = match from {
        Sender::Agent(k) => k.as_str().to_owned(),
        Sender::Operator | Sender::Engine => "operator".to_owned(),
        Sender::Orchestrator(_) | Sender::Pa => from.label(),
    };
    let f = Fresh { replaces: agent.session.clone(), by, at: ctx.now, pending: running, vendor: None };
    let mut a = agent.clone();
    if running {
        a.fresh = Some(f);
    } else {
        replace(state, &mut a, f);
    }
    let card = places::card_of(state, &a.place);
    let mut d = Decision::ok(json!({"fresh": {"agent": key.as_str(), "replaces": agent.session, "pending": running}}));
    d.journal(
        "fresh",
        card.as_ref().map(|c| c.as_str()),
        json!({"agent": key.as_str(), "replaces": agent.session, "pending": running}),
    );
    d.changes.push(Change::PutAgent(a));
    d
}

/// The session is replaced: the next turn starts a new one. A retry or a correction owed for the
/// old one becomes a fresh turn, its news given back, since its prompt was the old session's.
pub(super) fn replace(state: &State, a: &mut Agent, f: Fresh) {
    let challenge = super::required::waiting_on(state, a);
    replace_reading(state, a, f, challenge);
}

// `replace`, `challenge` the waiting challenge's request: the new session never read it, so it is news again.
fn replace_reading(state: &State, a: &mut Agent, f: Fresh, challenge: Option<u64>) {
    if let Some(of) = super::ends::owed_correction(a) {
        super::ends::give_back(state, a, &of);
    }
    // The new session read none of its news.
    a.missed.clear();
    a.session = None;
    a.session_started = false;
    a.needs_specification = false;
    a.held.clear();
    a.last_prompt = None;
    a.context = None;
    a.last_turn_ms = None;
    a.ran = Default::default();
    if matches!(a.owed, Some(Owed::Retry { .. } | Owed::Correction { .. })) {
        a.owed = Some(Owed::Wake);
        a.seen = a.seen.min(a.seen_before);
    }
    if let Some(seq) = challenge {
        a.seen = a.seen.min(seq);
        a.taken.retain(|s| *s != seq);
    }
    a.fresh = Some(Fresh { pending: false, ..f });
}

/// A turn ended: a `fresh` waiting for it replaces the session it ran in, after its reply moved the card.
pub(crate) fn turn_ended(p: &Poster<'_>, a: &mut Agent) {
    if let Some(f) = a.fresh.clone().filter(|f| f.pending) {
        let replaces: Option<SessionId> = a.session.clone().or(f.replaces.clone());
        let card = places::card_of(p.state, &a.place).and_then(|c| p.card(&c));
        let challenge = card.and_then(|c| super::required::waiting(&c, &a.key));
        replace_reading(p.state, a, Fresh { replaces, ..f }, challenge);
    }
}

/// The new session started: its start said what it replaces, once.
pub(super) fn started(a: &mut Agent) {
    if a.fresh.as_ref().is_some_and(|f| !f.pending) {
        a.fresh = None;
    }
}

/// Whether a turn failed for its CLI not finding the session it resumes (fac-140): Claude's words
/// for it, Codex's, or the engine's for agy's.
pub fn session_missing(words: &str) -> bool {
    let missing =
        ["No conversation found with session ID", "no rollout found for thread id", "agy found no conversation"];
    missing.iter().any(|w| words.contains(w))
}

/// An error ask's words for a session its CLI can't find, in place of the CLI's own.
pub(super) fn missing(a: &Agent) -> String {
    let session = a.session.as_ref().map_or_else(String::new, |s| format!(" `{s}`"));
    let cli = a.choice.as_ref().map_or("its CLI", |c| super::models::cli_name(c.cli));
    format!(
        "{} can't go on: {cli} can't find its session{session}. `fresh` starts it a new session on {cli}, which \
         reads the card first; `resume` then runs its turn.",
        a.key
    )
}

/// The line a fresh session's start gives: the session it replaces, and where its turns are.
pub(super) fn line(a: &Agent) -> Option<String> {
    let f = a.fresh.as_ref().filter(|f| !f.pending)?;
    let (y, mo, d, h, mi, _) = crate::state::names::civil_from_ms(f.at);
    let old = f.replaces.as_ref().map_or_else(|| "the session before".to_owned(), |s| format!("session `{s}`"));
    let at = format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02} UTC");
    let place = format!("{}'s conversation", a.place);
    let how = match f.vendor {
        Some(cli) => format!("{}'s switch to {}", f.by, super::models::cli_name(cli)),
        None => format!("`fresh` from {}", f.by),
    };
    Some(format!(
        "This session is fresh: it replaces {old}, which {how} ended at {at}. Its turns are in {place}, before then."
    ))
}
