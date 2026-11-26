//! The views (1.5.2, section 5): what is true, worked out from the state and the engine's reads; the window draws them
//! and derives nothing. The screen is every room, hand, board, memory and note; a place's own view is `place`'s.

use mork_types::ask::{AskKind, Color};
use mork_types::card::{Card, Mode};
use mork_types::view::{HandView, PlaceKind, RoomRow, Screen, TopLine, Under};
use mork_types::{AgentKey, PlaceId};

use super::findings::folded_parent;
use super::order::{by_number, live_cards};
use super::outside::Outside;
use super::place::kind;
use super::status::{agent_light, card_light, stage_word};
use crate::decide::Ctx;
use crate::state::State;

/// The screen but the place shown.
pub fn screen(state: &State, outside: &Outside, ctx: Ctx<'_>) -> Screen {
    let rooms = rooms(state, outside, ctx);
    Screen {
        revision: state.revision,
        now_ms: ctx.now,
        tz_offset_min: outside.tz_offset_min,
        top: TopLine {
            pin: outside.pin.clone(),
            usage: outside.usage.clone(),
            phase: outside.phase.clone(),
            resources: outside.resources.clone(),
        },
        rooms,
        hands: hands(state, ctx),
        boards: super::boards::boards(state, outside, ctx),
        memories: crate::memory::view(state, outside.tz_offset_min, ctx),
        notes: crate::memory::notes_view(state, ctx.config),
        projects: super::projects::projects(state, outside, ctx),
    }
}

fn rooms(state: &State, outside: &Outside, ctx: Ctx<'_>) -> Vec<RoomRow> {
    let row = |p: &str, c: Option<&Card>, under: Option<&Under>| RoomRow {
        under: under.cloned(),
        ..row(state, p, c, outside)
    };
    let mut out = vec![row(crate::messages::places::MAIN, None, None)];
    for (under, c) in grouped(state, ctx) {
        let under = Some(&under);
        // A planner card's spec room comes before its build room (fac-633).
        if crate::messages::places::two_rooms(c) {
            out.push(row(&crate::messages::places::spec_room(&c.id), Some(c), under));
        }
        out.push(row(c.id.as_str(), Some(c), under));
        for p in state.places.keys().filter(|p| matches!(kind(state, p), PlaceKind::Dm { card, .. } if card == c.id)) {
            out.push(row(p, None, under));
        }
        // Its foreman's subagents, nested under it, in the order they were spawned (fac-94).
        for w in crate::agents::children::of_card(state, &c.id) {
            out.push(row(w.place.as_str(), None, under));
        }
    }
    out
}

// Config order owns the project headings; a card's review twins stay grouped under it (fac-421, fac-680).
fn grouped<'a>(state: &'a State, ctx: Ctx<'_>) -> Vec<(Under, &'a Card)> {
    let mut out: Vec<(Under, &Card)> = live_cards(state)
        .into_iter()
        .map(|c| {
            let mut u = under(state, c);
            u.detail = state.cards.get(&u.card).map(|h| detail(state, h, ctx.config)).unwrap_or_default();
            (u, c)
        })
        .collect();
    let projects: Vec<_> = ctx.config.ordered_projects().collect();
    let at = |c: &Card| projects.iter().position(|p| **p == c.project).unwrap_or(usize::MAX);
    let start = |u: &Under, c: &Card| state.cards.get(&u.card).map_or(c.started_ms, |h| h.started_ms);
    out.sort_by(|(a, x), (b, y)| {
        at(x)
            .cmp(&at(y))
            .then(x.project.cmp(&y.project))
            .then((a.project != x.project.as_str()).cmp(&(b.project != y.project.as_str())))
            .then(start(a, x).cmp(&start(b, y)))
            .then_with(|| by_number(a.card.as_str(), b.card.as_str()))
            .then(a.review.cmp(&b.review))
            .then(x.started_ms.cmp(&y.started_ms))
            .then_with(|| by_number(x.id.as_str(), y.id.as_str()))
    });
    out
}

// A ticketed project's card PR and DevOps tasks beside its id in the rooms column (fac-362).
fn detail(state: &State, c: &Card, config: &mork_types::config::Config) -> String {
    if !crate::board::tickets::has_tickets(config, &c.project) {
        return String::new();
    }
    let tasks = crate::board::tickets::own(state, c, config).iter().map(u64::to_string).collect::<Vec<_>>();
    let pr = c.pr.map(|n| format!("PR {n}"));
    let devops = (!tasks.is_empty()).then(|| format!("DevOps {}", tasks.join(" ")));
    [pr, devops].into_iter().flatten().collect::<Vec<_>>().join(" ")
}

// A card's heading: its own, but a review twin's is the card whose branch (`of`) or PR it reviews;
// a colleague's PR or branch has no card of ours, and heads itself in its project's code reviews.
fn under(state: &State, c: &Card) -> Under {
    let review = c.mode == Mode::Review;
    let of = c.review.as_ref().and_then(|r| r.of.clone()).filter(|_| review);
    let by_pr = || {
        let pr_of = |o: &&Card| o.pr.is_some_and(|n| crate::reviews::cards::card_id(o.project.as_str(), n) == c.id);
        state.cards.values().filter(|o| o.mode != Mode::Review && o.project == c.project).find(pr_of)
    };
    match of.or_else(|| by_pr().filter(|_| review).map(|o| o.id.clone())) {
        Some(card) => Under { project: c.project.to_string(), card, review, detail: String::new(), mode: c.mode },
        None if review => Under {
            project: format!("{} code reviews", c.project),
            card: c.id.clone(),
            review,
            detail: String::new(),
            mode: c.mode,
        },
        None => {
            Under { project: c.project.to_string(), card: c.id.clone(), review, detail: String::new(), mode: c.mode }
        }
    }
}

fn row(state: &State, place: &str, card: Option<&Card>, outside: &Outside) -> RoomRow {
    let squares =
        state.agents.values().filter(|a| a.place == place).map(|a| agent_light(state, &a.key, outside)).collect();
    let kind = kind(state, place);
    let topic = match &kind {
        PlaceKind::Child { agent, .. } => state.agents.get(agent).and_then(|a| a.topic.clone()),
        PlaceKind::Main
        | PlaceKind::Card { .. }
        | PlaceKind::Spec { .. }
        | PlaceKind::Challenge { .. }
        | PlaceKind::Dm { .. }
        | PlaceKind::Other => None,
    };
    RoomRow {
        place: PlaceId::new(place),
        kind,
        light: card.and_then(|c| card_light(state, c)),
        stage: card.map(|c| stage_word(state, c)).unwrap_or_default(),
        paused: card.is_some_and(|c| c.paused),
        squares,
        topic,
        under: None,
    }
}

fn hands(state: &State, ctx: Ctx<'_>) -> Vec<HandView> {
    let mut open: Vec<&mork_types::ask::Ask> = state.asks.values().filter(|a| a.open).collect();
    open.sort_by_key(|a| (a.color != Color::Red, a.raised_ms, a.id.clone()));
    open.into_iter()
        .filter_map(|a| {
            let seq = a.seq?;
            let message = state.messages.iter().rev().find(|m| m.seq == seq)?;
            let card = a
                .card
                .as_ref()
                .map(|id| state.cards.get(id).and_then(|c| folded_parent(state, c)).unwrap_or_else(|| id.clone()));
            let cache_key = a.agent.clone().or_else(|| {
                (a.kind == AskKind::Triage)
                    .then(|| card.as_ref().map(|c| AgentKey::new(format!("{c}/foreman"))))
                    .flatten()
            });
            Some(HandView {
                seq,
                headline: message.headline.clone(),
                message_place: message.place.clone(),
                ask: a.id.clone(),
                place: a.place.clone(),
                card,
                role: a.agent.as_ref().and_then(|k| state.agents.get(k)).map(|g| g.role),
                kind: a.kind,
                color: a.color,
                words: a.text.clone(),
                raised_ms: a.raised_ms,
                offers: super::offers::on_hand(state, &a.id, ctx),
                given: a.given,
                design: a.design,
                cold_at_ms: cache_key
                    .as_ref()
                    .and_then(|k| state.agents.get(k))
                    .filter(|g| !matches!(g.status, mork_types::state::AgentStatus::Working { .. }))
                    .and_then(|g| crate::agents::cache::cold_at(ctx.config, g)),
                decisions: a
                    .card
                    .as_ref()
                    .and_then(|id| state.cards.get(id))
                    .filter(|_| a.kind == AskKind::ApproveBrief)
                    .map(|c| super::overview::standing(state, c))
                    .unwrap_or_default(),
                choices: a.choices.clone(),
                brief: brief(state, a, ctx),
            })
        })
        .collect()
}

// An `approve-brief` hand's sections (fac-696): its card's typed sketch and the mock link its approve covers.
fn brief(state: &State, a: &mork_types::ask::Ask, ctx: Ctx<'_>) -> Option<mork_types::view::BriefView> {
    let c = a.card.as_ref().and_then(|id| state.cards.get(id)).filter(|_| a.kind == AskKind::ApproveBrief)?;
    let mock = crate::lifecycle::mock::link(c, ctx.config).filter(|_| crate::lifecycle::mock::briefing(c));
    let sketch = c.sketch.clone().unwrap_or_default();
    (mock.is_some() || c.sketch.is_some()).then_some(mork_types::view::BriefView {
        changes: sketch.changes,
        code_shape: sketch.code_shape,
        mock,
    })
}
