//! What wakes a project's orchestrator (fac-265): one line Mork writes for each stream event it relays and for a
//! card that ended. The haiku's prompt is made of these lines alone, so no seat's words may be in one.

use mork_types::ProjectId;
use mork_types::action::{ActionStatus, ActionWhat};
use mork_types::card::Card;
use mork_types::note::{Note, NoteWhat};
use mork_types::orchestrator::OrchestratorWhat;

use crate::state::State;

/// The line for `what`, or none: an ask given to the orchestrators (carrying its card's delegation level when on),
/// an `opinion`, a section sent to review and a seat's or engine's request (fac-474). Only engine text is carried.
pub fn line(state: &State, what: &OrchestratorWhat) -> Option<String> {
    Some(match what {
        OrchestratorWhat::Given(a) => given_line(state, a),
        OrchestratorWhat::Opinion { ask, text } => format!("{} Ask in {}.", one_line(text), ask.place),
        OrchestratorWhat::Review { text, .. } => one_line(text),
        OrchestratorWhat::Message { from, place, request: true, .. } => format!("request from {from} in {place}"),
        OrchestratorWhat::Message { .. } | OrchestratorWhat::AskOpened(_) | OrchestratorWhat::AskClosed(_) => {
            return None;
        }
    })
}

fn given_line(state: &State, a: &mork_types::orchestrator::OrchestratorAsk) -> String {
    match a.card.as_ref().and_then(|id| state.cards.get(id)).filter(|c| c.delegated) {
        Some(c) => format!("ask given to you ({}): {}", c.delegation(), a.ask),
        None => format!("ask given to you: {}", a.ask),
    }
}

/// Whether a card that ended has settled (fac-287): its seats' closing turns ended and its clone,
/// its handoff copied first, gone. Only then does its line say all it left.
pub fn settled(state: &State, card: &Card) -> bool {
    let id = card.id.as_str();
    let closing = state.agents.values().any(|a| a.retiring.is_some() && a.key.as_str().split('/').next() == Some(id));
    // A held removal is its handoff not yet copied: retried it runs again, dropped or done by hand it ends.
    let held = state.actions.values().any(|x| {
        matches!(&x.what, ActionWhat::RemoveClone { card: c, run, .. } if *c == card.id && *run == card.run)
            && matches!(x.status, ActionStatus::Held)
    });
    !closing && !held && !crate::agents::closing::clone_going(state, id)
}

/// The line for a card whose run ended, done or closed, and what it left among the notes `notes` lists under its
/// project (fac-592), Mork's a seat of another project's card filed not counted; no title, a seat's words.
pub fn ended(state: &State, card: &Card, mork: Option<&ProjectId>) -> String {
    let mut counts = [0usize; 5];
    let kept = |n: &&Note| n.card.as_ref() == Some(&card.id) && crate::memory::under(n, mork) == Some(&card.project);
    for n in state.notes.values().filter(kept) {
        // A survey answer is kept from before the closing turn: no card ends leaving one now.
        let kind = match n.what {
            NoteWhat::Feedback { .. } => 0,
            NoteWhat::Lesson { .. } => 1,
            NoteWhat::Smell { .. } => 2,
            NoteWhat::Handoff { .. } => 3,
            NoteWhat::Proposal { .. } => 4,
            NoteWhat::Survey { .. } => continue,
        };
        counts[kind] += 1;
    }
    let names = [
        ("a feedback", "feedbacks"),
        ("a lesson", "lessons"),
        ("a code smell", "code smells"),
        ("a handoff", "handoffs"),
        ("a proposal", "proposals"),
    ];
    let left: Vec<String> = counts
        .iter()
        .zip(names)
        .filter(|(n, _)| **n > 0)
        .map(|(n, (one, many))| if *n == 1 { one.to_owned() } else { format!("{n} {many}") })
        .collect();
    let line = format!("{} is {}", card.id, card.stage.as_str());
    match left.split_last() {
        None => line,
        Some((last, [])) => format!("{line}, left {last}"),
        Some((last, rest)) => format!("{line}, left {} and {last}", rest.join(", ")),
    }
}

// An engine's text, whole, on one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
