//! The orchestrators' streams (fac-119, fac-244): every message to the operator or an orchestrator and every ask opening or
//! closing, recorded in its transaction, hooked or not, each on one project's stream: its card's, else Mork's.

use std::collections::BTreeMap;

use mork_types::ask::{Ask, AskKind};
use mork_types::card::Card;
use mork_types::memory::GENERAL;
use mork_types::orchestrator::{OrchestratorAsk, OrchestratorEvent, OrchestratorWhat, ReviewSection};
use mork_types::reply::Intent;
use mork_types::state::{Agent, Message, MessageKind, ORCHESTRATOR, Place, Sender};
use mork_types::{AgentKey, AskId, CardId, ProjectId};
use serde_json::json;

use super::places;
use crate::decide::{Ctx, Decision};
use crate::state::{Change, State};

/// Adds a line of the stream for each message to the operator or an orchestrator the decision posts, and each
/// ask it opens or closes. Room chatter stays out: `watch` and `history` read it.
pub fn record(state: &State, d: &mut Decision, mork: Option<&ProjectId>, now: i64) {
    let mut open: BTreeMap<&AskId, bool> = BTreeMap::new();
    let mut lines = Vec::new();
    for c in &d.changes {
        if let Change::AddMessage(m) = c
            && on_stream(m)
            && let Some(project) = message_project(state, &d.changes, m, mork)
        {
            let card = card_at(state, &d.changes, &m.place).map(|c| c.id.clone());
            lines.push((project, message(m, card)));
        } else if let Change::PutAsk(a) = c {
            let was = open.get(&a.id).copied().unwrap_or_else(|| state.asks.get(&a.id).is_some_and(|x| x.open));
            open.insert(&a.id, a.open);
            // Its buttons as the decision leaves its agent: a failed turn's error raises the ask.
            let agent = |k: &AgentKey| decided(state, &d.changes, k);
            let project = || ask_project(state, &d.changes, a, mork);
            match (was, a.open) {
                (false, true) => {
                    if let Some(project) = project() {
                        lines.push((project, OrchestratorWhat::AskOpened(ask(a, agent))));
                    }
                }
                (true, false) => {
                    if let Some(project) = project() {
                        lines.push((project, OrchestratorWhat::AskClosed(ask(a, agent))));
                    }
                }
                (true, true) | (false, false) => {}
            }
        }
    }
    // After any line the decision already gives (`given`'s).
    let given =
        d.changes.iter().filter_map(|c| if let Change::AddOrchestratorEvent(e) = c { Some(e.seq + 1) } else { None });
    let first = given.fold(state.orchestrator_next, u64::max);
    for (seq, (project, what)) in (first..).zip(lines) {
        d.changes.push(Change::AddOrchestratorEvent(OrchestratorEvent { seq, time: now, project, what }));
    }
}

/// A message's stream snapshot; seat and engine requests addressed to an orchestrator wake it.
pub(super) fn message(m: &Message, card: Option<CardId>) -> OrchestratorWhat {
    let request = matches!(m.from, Sender::Agent(_) | Sender::Engine)
        && m.intent == Some(Intent::Request)
        && m.to.as_deref().is_some_and(|t| t.starts_with(ORCHESTRATOR) || t == "orchestrator");
    OrchestratorWhat::Message { place: m.place.clone(), card, from: m.from.label(), text: m.text.clone(), request }
}

/// `give`: the operator gives an open ask, but a mock step's, to its project's orchestrator: a `given`
/// line on its stream, the ask marked as with it until it closes. It answers nothing.
pub(super) fn give(state: &State, id: &AskId, mork: Option<&ProjectId>, now: i64) -> Decision {
    let Some(a) = state.asks.get(id).filter(|a| a.open) else {
        return Decision::refused(format!("no open ask `{id}`"));
    };
    if a.given {
        return Decision::refused(format!("`{id}` is already given to its orchestrator"));
    }
    if !givable(a) {
        return Decision::refused(format!(
            "`{id}` waits for the operator's own `approve`, at a mock step: it is never given"
        ));
    }
    if visual_plan(state, a) {
        return Decision::refused(format!(
            "`{id}` approves a visual card's plan, its mock included: that approve is the operator's alone, so it is never given"
        ));
    }
    let Some(project) = ask_project(state, &[], a, mork) else {
        return Decision::refused(format!("`{id}` belongs to no project, so no orchestrator can take it"));
    };
    let mut d = Decision::ok(json!({"give": {"ask": id.as_str()}}));
    d.journal("give", a.card.as_ref().map(|c| c.as_str()), json!({"ask": id.as_str()}));
    d.changes.extend(given(a, state.orchestrator_next, now, project, |k| state.agents.get(k)));
    d
}

/// `opinion`: one advice request on the ask's orchestrator's stream; the ask stays open and the operator's.
pub(super) fn opinion(state: &State, id: &AskId, mork: Option<&ProjectId>, now: i64) -> Decision {
    let Some(a) = state.asks.get(id).filter(|a| a.open) else {
        return Decision::refused(format!("no open ask `{id}`"));
    };
    if a.given {
        return Decision::refused(format!("`{id}` is already given to its orchestrator"));
    }
    let Some(project) = ask_project(state, &[], a, mork) else {
        return Decision::refused(format!(
            "`{id}` belongs to no project, so no orchestrator can give an opinion on it"
        ));
    };
    let mut d = Decision::ok(json!({"opinion": {"ask": id.as_str()}}));
    d.journal("opinion", a.card.as_ref().map(|c| c.as_str()), json!({"ask": id.as_str()}));
    let what = OrchestratorWhat::Opinion {
        ask: ask(a, |k| state.agents.get(k)),
        text: "The operator asks for your opinion on this ask. Give it to him in your own chat; answer nothing to the seat. The ask stays his.".to_owned(),
    };
    d.changes.push(Change::AddOrchestratorEvent(OrchestratorEvent {
        seq: state.orchestrator_next,
        time: now,
        project,
        what,
    }));
    d
}

/// `orchestrator.review <section> <target>`: one `review` line on that project's orchestrator's stream, read live or at
/// its next hook. Memory sends record their time, and `general` goes to Mork's orchestrator.
pub(super) fn review(state: &State, section: ReviewSection, target: &str, ctx: Ctx<'_>) -> Decision {
    let project = if target == GENERAL {
        if section != ReviewSection::Memories {
            return Decision::refused(format!("`{GENERAL}` is the Memory section's alone: name a project"));
        }
        let Some(mork) = ctx.config.mork.project.as_ref() else {
            return Decision::refused(format!(
                "`{GENERAL}` has no orchestrator to send it to: `[mork] project` isn't configured"
            ));
        };
        mork.clone()
    } else if ctx.config.projects.contains_key(&ProjectId::new(target)) {
        ProjectId::new(target)
    } else {
        return Decision::refused(format!("no project `{target}` to send for review"));
    };
    let mut d = Decision::ok(json!({"orchestrator.review": {"section": section, "target": target}}));
    d.journal("orchestrator.review", None, json!({"section": section, "target": target}));
    d.changes.push(review_event(section, target, project, state.orchestrator_next, ctx.now));
    if section == ReviewSection::Memories {
        d.changes.push(crate::memory::reviewed(state, target, ctx.now));
    }
    d
}

fn review_event(section: ReviewSection, target: &str, project: ProjectId, seq: u64, now: i64) -> Change {
    let what =
        OrchestratorWhat::Review { section, target: Some(target.to_owned()), text: review_text(section, target) };
    Change::AddOrchestratorEvent(OrchestratorEvent { seq, time: now, project, what })
}

// What the `review` line asks of the orchestrator; the Memory section's procedure lives in the skill alone (fac-598).
fn review_text(section: ReviewSection, target: &str) -> String {
    match section {
        ReviewSection::Memories => format!(
            "The operator asks you to review the {target} memory alone, by your mork-cards skill's section \"A project's memory, notes and backlog\", and to report to him what matters."
        ),
        ReviewSection::Notes => format!(
            "The operator asks you to review {target}'s notes alone: read them (`mork ctl notes {target}`: survey answers, code smells, feedback, lessons, handoffs and proposals, each tagged by kind and card; a handoff's files are in its `dir`), clean them up (`notes.delete {target} <ids>`), and report to him what matters."
        ),
    }
}

/// Whether an ask may be given to an orchestrator: all but a mock step's,
/// The operator's alone (fac-258).
pub(super) fn givable(a: &Ask) -> bool {
    a.kind != AskKind::Mock
}

// A visual card's plan is approved by the operator alone, whatever its delegation (fac-636).
fn visual_plan(state: &State, a: &Ask) -> bool {
    a.kind == AskKind::ApproveBrief && a.card.as_ref().and_then(|c| state.cards.get(c)).is_some_and(|c| c.visual)
}

/// An open ask given to its orchestrator: its `given` line, numbered `seq`, on `project`'s stream, and the
/// ask marked as with it.
pub(super) fn given<'s>(
    a: &Ask,
    seq: u64,
    now: i64,
    project: ProjectId,
    agent: impl Fn(&AgentKey) -> Option<&'s Agent>,
) -> [Change; 2] {
    [
        Change::AddOrchestratorEvent(OrchestratorEvent {
            seq,
            time: now,
            project,
            what: OrchestratorWhat::Given(ask(a, agent)),
        }),
        Change::PutAsk(Ask { given: true, ..a.clone() }),
    ]
}

/// Main says a project's orchestrator hooked or unhooked: to nobody, so it isn't a line of a stream.
pub fn hooked(state: &State, project: &ProjectId, hooked: bool, now: i64) -> Decision {
    let text = if hooked {
        format!("{ORCHESTRATOR}{project} hooked: it reads what reaches the operator for {project}.")
    } else {
        format!("{ORCHESTRATOR}{project} unhooked.")
    };
    let m = super::message(state, places::MAIN, Sender::Engine, None, MessageKind::Engine, &text, now);
    let mut d = Decision::default();
    d.journal("orchestrator.hooked", None, json!({"project": project, "hooked": hooked}));
    if !state.places.contains_key(places::MAIN) {
        d.changes.push(Change::PutPlace(Place { id: places::MAIN.into(), next_number: 1 }));
    }
    d.changes.push(Change::AddMessage(m));
    d
}

fn on_stream(m: &Message) -> bool {
    m.to.as_deref().is_some_and(|t| matches!(t, "operator" | "orchestrator") || t.starts_with(ORCHESTRATOR))
}

/// The project of the card `owner` names, as the state has it.
pub(crate) fn owner_project(state: &State, owner: &str) -> Option<ProjectId> {
    placed(state, &[], None, "", Some(owner))
}

// A message to `orchestrator:<project>` is that one's; else where it is posted, or, posted in main,
// the card its agent works for.
fn message_project(state: &State, changes: &[Change], m: &Message, mork: Option<&ProjectId>) -> Option<ProjectId> {
    if let Some(p) = m.to.as_deref().and_then(|t| t.strip_prefix(ORCHESTRATOR)) {
        return Some(ProjectId::new(p));
    }
    let owner = if let Sender::Agent(k) = &m.from { k.as_str().split('/').next() } else { None };
    placed(state, changes, None, m.place.as_str(), owner).or_else(|| mork.cloned())
}

pub(super) fn ask_project(state: &State, changes: &[Change], a: &Ask, mork: Option<&ProjectId>) -> Option<ProjectId> {
    placed(state, changes, a.card.as_ref(), a.place.as_str(), None).or_else(|| mork.cloned())
}

// The project of a card, else of the card whose place it is, else of the card `owner` names.
fn placed(
    state: &State,
    changes: &[Change],
    card: Option<&CardId>,
    place: &str,
    owner: Option<&str>,
) -> Option<ProjectId> {
    let card = card.and_then(|c| card_in(state, changes, c)).or_else(|| card_at(state, changes, place));
    let card = card.or_else(|| card_in(state, changes, &CardId::new(owner?)));
    card.map(|c| c.project.clone())
}

// The card whose room, spec room, challenge place or DM a place is, as the decision leaves the cards.
fn card_at<'s>(state: &'s State, changes: &'s [Change], place: &str) -> Option<&'s Card> {
    let id = match place.split('/').collect::<Vec<_>>().as_slice() {
        [card] | [card, "spec" | "challenge"] | [card, _, "dm"] => CardId::new(*card),
        _ => return None,
    };
    card_in(state, changes, &id)
}

fn card_in<'s>(state: &'s State, changes: &'s [Change], id: &CardId) -> Option<&'s Card> {
    let put = changes
        .iter()
        .rev()
        .find_map(|c| if let Change::PutCard(x) = c { (x.id == *id).then_some(&**x) } else { None });
    put.or_else(|| state.cards.get(id))
}

/// An agent as the decision's changes leave it.
pub(super) fn decided<'s>(state: &'s State, changes: &'s [Change], k: &AgentKey) -> Option<&'s Agent> {
    for c in changes.iter().rev() {
        if let Change::PutAgent(a) = c
            && a.key == *k
        {
            return Some(a);
        }
        if let Change::RemoveAgent { key } = c
            && key == k
        {
            return None;
        }
    }
    state.agents.get(k)
}

pub(super) fn ask<'s>(a: &Ask, agent: impl Fn(&AgentKey) -> Option<&'s Agent>) -> OrchestratorAsk {
    OrchestratorAsk {
        ask: a.id.clone(),
        place: a.place.clone(),
        card: a.card.clone(),
        from: a.agent.as_ref().map(|k| k.as_str().to_owned()),
        kind: a.kind,
        text: a.text.clone(),
        answers: crate::views::offers::answers_with(a, agent),
    }
}
