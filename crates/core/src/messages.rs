//! Messages and asks, and what the operator hands the projects' orchestrators: where a message goes, who it wakes,
//! the asks it raises and the answers delegated to an orchestrator.

pub mod announce;
pub mod asks;
pub mod auto;
pub mod delegate;
pub mod images;
pub mod interrupts;
pub mod places;
pub mod replies;
pub mod requests;
pub mod routes;
pub mod say;
pub mod streams;
pub mod wakes;

use mork_types::Command;
use mork_types::reply::{Intent, Role};
use mork_types::state::{Message, MessageKind, Place, Sender};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::{Change, State};

/// Routes message commands, explicit ask assignments, advice and review requests, or delegation levels (fac-474).
pub fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if matches!(cmd, Command::Say { .. }) {
        say::say(state, from, cmd, ctx)
    } else if let Command::Ack { ask } = cmd {
        asks::ack(state, ask, ctx)
    } else if let Command::Auto { project, off } = cmd {
        auto::toggle(project, off.unwrap_or(false), ctx)
    } else if let Command::Give { ask } = cmd {
        streams::give(state, ask, ctx.config.mork.project.as_ref(), ctx.now)
    } else if let Command::Opinion { ask } = cmd {
        streams::opinion(state, ask, ctx.config.mork.project.as_ref(), ctx.now)
    } else if let Command::Delegate { card, off, self_sustained } = cmd {
        delegate::toggle(state, card, !off.unwrap_or(false), self_sustained.unwrap_or(false), ctx)
    } else if let Command::OrchestratorReview { section, target } = cmd {
        streams::review(state, *section, target, ctx)
    } else {
        Decision::refused(format!("`{}` isn't a message's command", cmd.name()))
    }
}

/// A message in a place, alone in its decision.
pub fn message(
    state: &State,
    place: &str,
    from: Sender,
    to: Option<&str>,
    kind: MessageKind,
    text: &str,
    now: i64,
) -> Message {
    let headline = text.lines().next().unwrap_or("").to_owned();
    let from_role = from.clone();
    Message {
        seq: state.next_seq,
        place: place.into(),
        time: now,
        from,
        to: to.map(str::to_owned),
        kind,
        headline,
        text: text.to_owned(),
        number: None,
        hand: None,
        images: Vec::new(),
        findings: Vec::new(),
        role: author_role(state, &from_role),
        intent: None,
        delegated: None,
        entries: None,
        choice: None,
    }
}

/// An agent author's role, as the state has it when it posts (M5.26).
pub fn author_role(state: &State, from: &Sender) -> Option<Role> {
    match from {
        Sender::Agent(k) => state.agents.get(k).map(|a| a.role),
        Sender::Operator | Sender::Engine | Sender::Orchestrator(_) | Sender::Pa => None,
    }
}

/// A line from the engine in a place, to the operator.
pub fn engine_says(state: &State, place: &str, text: &str, key: Option<&str>, ctx: Ctx<'_>) -> Decision {
    let m = message(state, place, Sender::Engine, Some("operator"), MessageKind::Engine, text, ctx.now);
    let mut d = Decision::default();
    d.journal("engine.says", None, json!({"place": place, "text": text, "key": key}));
    if !state.places.contains_key(place) {
        d.changes.push(Change::PutPlace(Place { id: place.into(), next_number: 1 }));
    }
    d.changes.push(Change::AddMessage(m));
    d
}

// Posts a message with its images and the kind its sender said it is (fac-18); `kind`, whether it woke anyone.
// Its seq.
fn posted(
    p: &mut Poster<'_>,
    (place, from, to): (&str, Sender, Option<&str>),
    kind: MessageKind,
    text: &str,
    (images, intent): (&[String], Option<Intent>),
) -> u64 {
    let seq = p.post(place, from, to, kind, "", text);
    if let Some(m) = p.last_message() {
        m.images = images.to_vec();
        m.intent = intent;
    }
    seq
}
