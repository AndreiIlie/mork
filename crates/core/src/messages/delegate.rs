//! Delegation gives seat questions and non-visual plan approvals to the project's orchestrator automatically;
//! actionable engine asks stay Andrei's and request mechanical help (fac-474). Asks already given stay given.

use factory_types::action::{ActionStatus, ActionWhat};
use factory_types::ask::{Ask, AskKind};
use factory_types::card::Card;
use factory_types::orchestrator::{OrchestratorAsk, OrchestratorEvent, OrchestratorWhat};
use factory_types::reply::Intent;
use factory_types::state::{Message, MessageKind, Sender};
use factory_types::{CardId, Command, ProjectId};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::{Change, State};

/// Sets the card's delegation level, kept across restarts and later runs.
pub(super) fn toggle(state: &State, card: &CardId, on: bool, self_sustained: bool, ctx: Ctx<'_>) -> Decision {
    if !on && self_sustained {
        return Decision::refused("choose either --off or --self-sustained");
    }
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(c) = state.cards.get(&id) else {
        return Decision::refused(format!("no card `{card}` in the state: a started card is delegated"));
    };
    if on && !c.stage.is_live() {
        return Decision::refused(format!("{id} has ended: its specification is no longer delegated"));
    }
    let data = json!({"card": id, "delegated": on, "self_sustained": self_sustained});
    let mut d = Decision::ok(json!({"delegate": data}));
    d.journal("delegate", Some(id.as_str()), data);
    let mut p = Poster::for_ctx(state, ctx);
    let next = Card { delegated: on, self_sustained, ..c.clone() };
    if c.stage.is_live() && c.delegation() != next.delegation() {
        let text = format!(
            "Andrei set {id}'s delegation level to {}.\n{}",
            next.delegation(),
            crate::agents::prompt::delegation_rule(&next)
        );
        super::requests::tell_agent(&mut p, &id, crate::lifecycle::facts::specifier(c), "Delegation changed", &text);
    }
    let rewake = c.delegated && !c.self_sustained && on && self_sustained;
    if on {
        crate::reviews::triage::release_on_delegate(&mut p, next.clone(), ctx);
    } else {
        p.put_card(next.clone());
    }
    p.finish(&mut d);
    if on {
        give_existing(state, &next, rewake, &mut d, ctx);
    }
    d
}

// Seat questions and non-visual plan approvals are auto-given on delegated cards (fac-669, fac-689).
fn auto_givable(a: &Ask, card: &Card) -> bool {
    a.kind == AskKind::Answer || (a.kind == AskKind::ApproveBrief && !card.visual)
}

// Turning delegation on gives open ungiven seat/plan asks; delegated -> self-sustained re-wakes already-given ones.
fn give_existing(state: &State, c: &Card, rewake: bool, d: &mut Decision, ctx: Ctx<'_>) {
    let mut seq = next_stream(state, &d.changes);
    for a in state.asks.values().filter(|a| a.open && a.card.as_ref() == Some(&c.id) && auto_givable(a, c)) {
        if !a.given {
            d.journal("give", Some(c.id.as_str()), json!({"ask": a.id.as_str()}));
            d.changes.extend(super::streams::given(a, seq, ctx.now, c.project.clone(), |k| state.agents.get(k)));
            seq += 1;
        } else if rewake {
            let what = OrchestratorWhat::Given(super::streams::ask(a, |k| state.agents.get(k)));
            d.changes.push(Change::AddOrchestratorEvent(OrchestratorEvent {
                seq,
                time: ctx.now,
                project: c.project.clone(),
                what,
            }));
            seq += 1;
        }
    }
}

/// Automatically gives newly opened seat and non-visual plan asks on delegated cards, without desktop notices.
pub fn give_opened(state: &State, d: &mut Decision, now: i64) {
    let mut to_give = Vec::new();
    for c in &d.changes {
        let Change::AddOrchestratorEvent(OrchestratorEvent {
            what: OrchestratorWhat::AskOpened(opened), project, ..
        }) = c
        else {
            continue;
        };
        let Some(a) = last_put(&d.changes, opened).filter(|a| a.open && !a.given) else { continue };
        let Some(card) = a.card.as_ref().and_then(|id| card(state, &d.changes, id)).filter(|c| c.delegated) else {
            continue;
        };
        if auto_givable(a, card) {
            to_give.push((project.clone(), Ask { notice: false, ..a.clone() }));
        }
    }
    for (seq, (project, a)) in (next_stream(state, &d.changes)..).zip(to_give) {
        d.intents.retain(|i| !matches!(i, crate::decide::Intent::Notify { ask } if *ask == a.id));
        d.journal("give", a.card.as_ref().map(|c| c.as_str()), json!({"ask": a.id.as_str()}));
        let given = super::streams::given(&a, seq, now, project, |k| super::streams::decided(state, &d.changes, k));
        d.changes.extend(given);
    }
}

fn next_stream(state: &State, changes: &[Change]) -> u64 {
    changes
        .iter()
        .filter_map(|c| if let Change::AddOrchestratorEvent(e) = c { Some(e.seq + 1) } else { None })
        .fold(state.orchestrator_next, u64::max)
}

/// Posts a waking request for each actionable engine ask opened on a delegated card, after its stream line.
pub fn request_opened(state: &State, d: &mut Decision, now: i64) {
    let next_message = d
        .changes
        .iter()
        .filter_map(|c| if let Change::AddMessage(m) = c { Some(m.seq + 1) } else { None })
        .fold(state.next_seq, u64::max);
    let next_stream = next_stream(state, &d.changes);
    for (i, (project, mut m)) in requests(state, &d.changes, now).into_iter().enumerate() {
        m.seq = next_message + i as u64;
        let id = CardId::new(m.place.as_str());
        d.changes.push(Change::AddOrchestratorEvent(OrchestratorEvent {
            seq: next_stream + i as u64,
            time: now,
            project,
            what: super::streams::message(&m, Some(id.clone())),
        }));
        d.journal("message", Some(id.as_str()), json!({"place": m.place, "seq": m.seq, "from": "engine", "to": m.to}));
        d.intents.push(crate::decide::Intent::WriteRecord { card: id });
        d.changes.push(Change::AddMessage(m));
    }
}

fn requests(state: &State, changes: &[Change], now: i64) -> Vec<(ProjectId, Message)> {
    let mut out = Vec::new();
    for c in changes {
        let Change::AddOrchestratorEvent(OrchestratorEvent {
            what: OrchestratorWhat::AskOpened(opened), project, ..
        }) = c
        else {
            continue;
        };
        let Some(a) = last_put(changes, opened).filter(|a| a.open && !a.given) else { continue };
        let Some(card) = a.card.as_ref().and_then(|id| card(state, changes, id)).filter(|c| c.delegated) else {
            continue;
        };
        let commands = answers(state, changes, a, opened, card);
        if commands.is_empty() {
            continue;
        }
        let text = format!(
            "Handle engine ask {}: {}\n\nThe ask stays Andrei's and is not given to you. Answer it with a command, not a message. Commands for `factory ctl --json`:\n{}",
            opened.ask,
            opened.text,
            commands.join("\n")
        );
        let mut m = super::message(
            state,
            card.id.as_str(),
            Sender::Engine,
            Some("orchestrator"),
            MessageKind::Request,
            &text,
            now,
        );
        m.headline = "Handle an engine ask".to_owned();
        m.intent = Some(Intent::Request);
        out.push((project.clone(), m));
    }
    out
}

fn answers(state: &State, changes: &[Change], a: &Ask, opened: &OrchestratorAsk, card: &Card) -> Vec<String> {
    if !matches!(
        a.kind,
        AskKind::Error | AskKind::Cut | AskKind::Stuck | AskKind::Checks | AskKind::Held | AskKind::Alarm
    ) {
        return Vec::new();
    }
    let action = a.action.as_ref().and_then(|id| {
        changes
            .iter()
            .rev()
            .find_map(|c| if let Change::PutAction(x) = c { (x.id == *id).then_some(&**x) } else { None })
            .or_else(|| state.actions.get(id))
    });
    if action
        .is_some_and(|a| matches!(a.what, ActionWhat::Merge { .. } | ActionWhat::MoveBase { .. } | ActionWhat::Door(_)))
    {
        return Vec::new();
    }
    opened
        .answers
        .iter()
        .filter(|o| {
            if matches!(o.command, Command::Resume { .. } | Command::Fresh { .. }) {
                card.stage.is_live()
            } else if matches!(o.command, Command::ActionRetry { .. } | Command::ActionDrop { .. }) {
                action.is_some_and(|a| a.status == ActionStatus::Held)
            } else {
                matches!(o.command, Command::Ack { .. })
                    && (a.kind == AskKind::Alarm && a.action.is_none()
                        || a.kind == AskKind::Error && !card.stage.is_live())
            }
        })
        .map(|o| json!(o.command).to_string())
        .collect()
}

fn last_put<'c>(changes: &'c [Change], opened: &OrchestratorAsk) -> Option<&'c Ask> {
    changes.iter().rev().find_map(|c| if let Change::PutAsk(a) = c { (a.id == opened.ask).then_some(a) } else { None })
}

fn card<'s>(state: &'s State, changes: &'s [Change], id: &CardId) -> Option<&'s Card> {
    let put = changes
        .iter()
        .rev()
        .find_map(|c| if let Change::PutCard(x) = c { (x.id == *id).then_some(&**x) } else { None });
    put.or_else(|| state.cards.get(id))
}
