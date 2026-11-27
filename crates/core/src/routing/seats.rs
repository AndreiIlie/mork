use mork_types::card::{Card, Mode};
use mork_types::config::{RoutingMode, SeatPhase, Standing};
use mork_types::reply::Role;
use mork_types::state::{Agent, Choice};
use serde_json::Value;

use super::choose::{Inputs, Request};
use crate::decide::{Ctx, Decision};
use crate::state::{Change, State};

pub(crate) fn role(name: &str, phase: SeatPhase) -> &str {
    match (name, phase) {
        ("foreman", SeatPhase::Report) => "foreman.report",
        ("foreman", _) => "foreman.work",
        _ => name,
    }
}

fn standing(state: &State, card: &Card, a: &Agent) -> Standing {
    let review = matches!(a.seat.as_str(), "review-foreman" | "reviewer") && card.mode == Mode::Review;
    let phase =
        if card.mode == Mode::Report && a.seat.as_str() == "foreman" { SeatPhase::Report } else { SeatPhase::Work };
    Standing { phase, review: review.then(|| crate::reviews::gate::kind(state, card)) }
}

// A single decision may create the card and both seats. Its planner's effective choice must be visible to its foreman.
pub(crate) fn created(state: &State, d: &mut Decision, ctx: Ctx<'_>) {
    if !d.changes.iter().any(|c| matches!(c, Change::PutAgent(_))) {
        return;
    }
    let mut changes = std::mem::take(&mut d.changes);
    let cards = changes
        .iter()
        .filter_map(|c| {
            let Change::PutCard(card) = c else { return None };
            Some((&card.id, card.as_ref()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut updates = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        let Change::PutAgent(a) = change else { continue };
        let Some((id, _)) = a.key.as_str().split_once('/') else { continue };
        let card = cards.get(&mork_types::CardId::new(id)).copied().or_else(|| state.cards.get(id));
        let Some(card) = card.filter(|c| c.stage.is_live()) else { continue };
        let at = standing(state, card, a);
        if (ctx.config.routing_mode(a.seat.as_str(), at) == RoutingMode::Fixed && a.role != Role::Planner)
            || crate::lifecycle::lookback::automatic(card)
        {
            continue;
        }
        let new_run = state.cards.get(id).is_none_or(|c| c.run != card.run);
        let new_seat = new_run || !state.agents.contains_key(&a.key);
        let moved = state.cards.get(id).is_some_and(|old| standing(state, old, a).phase != at.phase);
        if new_seat || moved {
            updates.push((i, card, a.clone(), at));
        }
    }
    updates.sort_by_key(|(_, _, a, _)| a.role != Role::Planner);
    let mut selected = std::collections::BTreeMap::new();
    let mut applied = Vec::new();
    for (i, card, mut a, at) in updates {
        let partner = partner(state, card, &a, &selected, ctx);
        if let Some(row) = route(state, card, &mut a, at, partner.as_deref(), ctx) {
            d.journal("route", Some(card.id.as_str()), row);
        }
        if a.role == Role::Planner
            && let Some(model) = effective(&a, at, ctx)
        {
            selected.insert(card.id.clone(), model);
        }
        applied.push((i, a));
    }
    for (i, a) in applied {
        changes[i] = Change::PutAgent(a);
    }
    d.changes = changes;
}

fn effective(a: &Agent, at: Standing, ctx: Ctx<'_>) -> Option<String> {
    a.pending
        .as_ref()
        .or(a.choice.as_ref())
        .filter(|c| c.phase == Some(at.phase))
        .map(|c| c.model.clone())
        .or_else(|| ctx.config.seat_in(a.seat.as_str(), at).map(|s| s.model))
}

fn partner(
    state: &State,
    card: &Card,
    a: &Agent,
    selected: &std::collections::BTreeMap<mork_types::CardId, String>,
    ctx: Ctx<'_>,
) -> Option<String> {
    if a.role == Role::Foreman && card.mode == Mode::Full && !card.no_planner {
        return selected
            .get(&card.id)
            .cloned()
            .or_else(|| {
                let key = format!("{}/planner", card.id);
                state.agents.get(key.as_str()).and_then(|a| effective(a, Standing::WORK, ctx))
            })
            .or_else(|| ctx.config.seat("planner").map(|s| s.model));
    }
    if a.role != Role::Reviewer {
        return None;
    }
    let id = crate::reviews::gate::reviewed_card_id(state, card)?;
    let subject = state.cards.get(id)?;
    let (head, model) = subject.gates.builder.as_ref()?;
    let reviewed = card.review.as_ref()?.open()?.submitted.as_ref()?;
    (head == reviewed).then(|| model.clone())
}

fn route(
    state: &State,
    card: &Card,
    a: &mut Agent,
    at: Standing,
    partner: Option<&str>,
    ctx: Ctx<'_>,
) -> Option<Value> {
    let name = a.seat.as_str();
    let mode = ctx.config.routing_mode(name, at);
    let explicit = a.pending.iter().chain(&a.choice).any(|c| c.phase == Some(at.phase));
    if mode == RoutingMode::Fixed || mode == RoutingMode::Auto && explicit {
        return None;
    }
    let seat = ctx.config.seat_in(name, at)?;
    let leaf = if a.role == Role::Reviewer { a.topic.as_deref().map(super::lens) } else { Some(card.project.as_str()) };
    let req =
        Request { agent: a.key.as_str(), run: card.run, phase: at.phase, role: role(name, at.phase), leaf, partner };
    let rec = Inputs::new(&state.limits, ctx).choose(&req, &seat, ctx.config.router_list(name, at).1, false);
    let row = rec.row(&req, mode);
    if mode == RoutingMode::Auto {
        let choice = rec.best().map(|c| c.offer.choice(at.phase)).unwrap_or_else(|| Choice {
            cli: seat.cli,
            model: seat.model,
            effort: seat.effort,
            by: "router".into(),
            phase: Some(at.phase),
        });
        if a.session_started {
            a.pending = Some(choice);
        } else {
            a.choice = Some(choice);
        }
    }
    Some(row)
}
