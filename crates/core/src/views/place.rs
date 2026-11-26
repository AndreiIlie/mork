//! One place's view (5.7, 5.8.2): a card's header, its agents and sessions, its conversation, and the commands it offers
//! now (1.5.2).

use mork_types::about::Summary;
use mork_types::card::Card;
use mork_types::config::{Config, MODEL_CATALOG, OPERATOR_MODELS};
use mork_types::reply::Role;
use mork_types::seen::AgentList;
use mork_types::state::{Agent, AgentStatus};
use mork_types::view::{AgentRow, CacheState, ModelChoice, PlaceKind, PlaceView};
use mork_types::{CardId, JobId, PlaceId};

use super::conversation::conversation;
use super::header::header;
use super::outside::Outside;
use super::status::{agent_light, doing};
use crate::agents::models;
use crate::decide::Ctx;
use crate::state::State;

/// One place's view: its latest `tail` messages, or all of them, the latest when they are more
/// than a line carries (the engine bounds what it sends of them).
pub fn place(state: &State, place: &str, tail: Option<u64>, outside: &Outside, ctx: Ctx<'_>) -> PlaceView {
    let kind = kind(state, place);
    let card = match &kind {
        PlaceKind::Card { card }
        | PlaceKind::Spec { card }
        | PlaceKind::Challenge { card }
        | PlaceKind::Dm { card, .. }
        | PlaceKind::Child { card, .. } => state.cards.get(card),
        PlaceKind::Main | PlaceKind::Other => None,
    };
    let agents = state
        .agents
        .values()
        .filter(|a| a.place == place)
        .map(|a| AgentRow {
            steps: outside.agents.get(&a.key).map(|s| s.steps.clone()).unwrap_or_default(),
            turn: own_turn(state, a, outside),
            ..row(state, a, outside, ctx)
        })
        .collect();
    let (messages, earlier, pinned) = conversation(state, place, tail);
    PlaceView {
        place: PlaceId::new(place),
        revision: state.revision,
        card: card.map(|c| header(state, c, outside, ctx)),
        decisions: card
            .filter(|c| c.id.as_str() == place)
            .map(|c| super::overview::standing(state, c))
            .unwrap_or_default(),
        start: super::offers::start_defaults(state, place, ctx),
        agents,
        messages,
        earlier,
        turns: Vec::new(),
        pinned,
        offers: super::offers::actions(state, place, ctx),
        input_to: input_to(state, place, &kind),
        plan: card.and_then(|c| c.plan.clone()),
        notes: card.map(|c| notes(state, c)).unwrap_or_default(),
        sessions: card.map(|c| sessions(state, c, outside, ctx)).unwrap_or_default(),
        outputs: card.and_then(|c| outside.outputs.get(&c.id).cloned()).unwrap_or_default(),
        lists: lists(state, place, outside),
        summaries: match (&kind, card) {
            (PlaceKind::Card { .. } | PlaceKind::Spec { .. }, Some(c)) => summaries(state, c),
            _ => Vec::new(),
        },
        review: card.and_then(|c| super::findings::folded_review(state, c)).map(|rc| folded(state, rc, outside, ctx)),
        triage: matches!(kind, PlaceKind::Card { .. })
            .then(|| card.and_then(|c| super::findings::triage_for_card(state, c)))
            .flatten(),
        overview: matches!(kind, PlaceKind::Card { .. })
            .then(|| card.map(|c| super::overview::overview(state, c, outside, ctx.config)))
            .flatten(),
        kind,
    }
}

// A folded review's seats, typed apart (fac-696): its foreman by its own key, its reviewers, then any other seat.
fn folded(state: &State, rc: &Card, outside: &Outside, ctx: Ctx<'_>) -> mork_types::review::FoldedReview {
    let foreman = format!("{}/foreman", rc.id);
    let (own, rest): (Vec<_>, Vec<_>) =
        sessions(state, rc, outside, ctx).into_iter().partition(|a| a.agent.as_str() == foreman);
    let (reviewers, others) = rest.into_iter().partition(|a| a.role == Role::Reviewer);
    mork_types::review::FoldedReview {
        card: rc.id.clone(),
        round: super::findings::current_round(state, rc).unwrap_or(1),
        foreman: own.into_iter().next(),
        reviewers,
        others,
    }
}

// The step lists above a place's input (fac-128): of the agents whose place it is, each while the
// turn its list is of runs; a child's shows in its own DM, not in the card's room (fac-150).
fn lists(state: &State, place: &str, outside: &Outside) -> Vec<AgentList> {
    let running = |a: &Agent, job: &JobId| matches!(&a.status, AgentStatus::Working { job: j } if j == job);
    state
        .agents
        .values()
        .filter(|a| a.place == place)
        .filter_map(|a| {
            let list = outside.agents.get(&a.key)?.list.as_ref()?;
            let shown = running(a, &list.job) && !list.steps.is_empty();
            shown.then(|| AgentList { agent: a.key.clone(), steps: list.steps.clone() })
        })
        .collect()
}

// An agent's row but its steps and turn, which only its own place's rows show.
fn row(state: &State, a: &Agent, outside: &Outside, ctx: Ctx<'_>) -> AgentRow {
    let mut setting = models::setting(state, ctx.config, a);
    let mut pending = models::pending(state, ctx.config, a);
    let mut choices = models::choices_for(state, ctx.config, a, models::By::Operator);
    let settings = setting.iter_mut().chain(&mut pending).map(|s| &mut s.model);
    for choice in choices.iter_mut().chain(settings) {
        set_display_label(ctx.config, choice);
    }
    AgentRow {
        parent: a.parent.clone(),
        agent: a.key.clone(),
        role: a.role,
        label: setting.as_ref().map(|s| s.model.label.clone()).unwrap_or_else(|| a.seat.to_string()),
        effort: setting.as_ref().map(|s| s.effort.as_str().to_owned()).unwrap_or_default(),
        light: agent_light(state, &a.key, outside),
        doing: doing(state, &a.key, outside),
        context: outside.agents.get(&a.key).and_then(|s| s.context).or_else(|| {
            crate::agents::costs::stored_context(ctx.config, a).and_then(|c| c.window.map(|w| (c.used, w)))
        }),
        steps: Vec::new(),
        setting,
        pending,
        choices,
        efforts: models::efforts_for(state, ctx.config, a, models::By::Operator),
        turn: None,
        cache: cache_state(ctx.config, a),
    }
}

// Pick aliases depend on the chooser. The same model needs one label throughout the room.
fn set_display_label(config: &Config, choice: &mut ModelChoice) {
    let mut catalog = MODEL_CATALOG.iter().chain(OPERATOR_MODELS);
    if let Some((_, _, label)) = catalog.find(|(_, model, _)| *model == choice.model) {
        choice.label = (*label).to_owned();
        if let Some(initial) = choice.label.get_mut(..1) {
            initial.make_ascii_uppercase();
        }
        return;
    }
    let mut picks =
        config.picks.iter().flat_map(|p| [&p.researcher, &p.reviewer.inside, &p.reviewer.outside]).flatten();
    let configured = picks.find(|p| p.cli == choice.cli && p.model == choice.model && !p.label.is_empty());
    choice.label.clone_from(configured.map_or(&choice.model, |p| &p.label));
}

fn cache_state(config: &mork_types::config::Config, a: &Agent) -> Option<CacheState> {
    if matches!(a.status, AgentStatus::Working { .. }) {
        Some(CacheState::Running)
    } else {
        crate::agents::cache::cold_at(config, a).map(|cold_at_ms| CacheState::Idle { cold_at_ms })
    }
}

fn sessions(state: &State, card: &Card, outside: &Outside, ctx: Ctx<'_>) -> Vec<AgentRow> {
    seats(state, &card.id).into_iter().map(|a| row(state, a, outside, ctx)).collect()
}

/// The card's recorded seats in room order: its spec room's, its build room's, its DMs', then its children (fac-105).
pub(super) fn seats<'a>(state: &'a State, card: &CardId) -> Vec<&'a Agent> {
    let mut own: Vec<&Agent> = state
        .agents
        .values()
        .filter(|a| {
            !crate::agents::children::is_child(a)
                && crate::messages::places::card_of(state, a.place.as_str()).as_ref() == Some(card)
        })
        .collect();
    let spec = crate::messages::places::spec_room(card);
    own.sort_by_key(|a| (a.place.as_str() != spec, a.place.as_str() != card.as_str()));
    let children = crate::agents::children::of_card(state, card);
    own.extend(children.iter().filter_map(|w| state.agents.get(&w.key)));
    own
}

// An agent's latest turn, only when its job is the agent's own: a child's never shows in its
// parent's row (fac-96).
fn own_turn(state: &State, a: &Agent, outside: &Outside) -> Option<mork_types::seen::TurnView> {
    let t = outside.agents.get(&a.key)?.turn.as_ref()?;
    state.jobs.get(&t.job).is_some_and(|j| j.owner == a.key).then(|| (**t).clone())
}

// The card's notes as its board has them now, as its agents' prompts read the card (fac-94).
fn notes(state: &State, c: &Card) -> Vec<String> {
    let board = state.board.get(&c.id);
    let description = board.map_or(&c.description, |b| &b.description);
    crate::board::notes::apart(state, board, description).0
}
// The room's summaries (fac-594): its card's and each carried card's board text, notes apart; an empty one shows nothing.
fn summaries(state: &State, c: &Card) -> Vec<Summary> {
    let own = (&c.id, &c.title, &c.description);
    let carried = c.carries.iter().map(|k| (&k.id, &k.title, &k.description));
    std::iter::once(own)
        .chain(carried)
        .filter_map(|(id, title, description)| {
            let board = state.board.get(id);
            let (title, description) = board.map_or((title, description), |b| (&b.title, &b.description));
            let text = crate::board::notes::apart(state, board, description).1;
            (!text.is_empty()).then(|| Summary { card: id.clone(), title: title.clone(), text })
        })
        .collect()
}

fn input_to(state: &State, place: &str, kind: &PlaceKind) -> String {
    match kind {
        // Plain words in main reach nobody (fac-119).
        PlaceKind::Main => String::new(),
        PlaceKind::Card { .. } => Role::Foreman.as_str().into(),
        PlaceKind::Spec { .. } => Role::Planner.as_str().into(),
        // Nobody says anything in a challenge place (fac-633).
        PlaceKind::Challenge { .. } => String::new(),
        PlaceKind::Dm { role, .. } => role.as_str().into(),
        PlaceKind::Child { agent, .. } => agent.as_str().rsplit('/').next().unwrap_or_default().into(),
        PlaceKind::Other => {
            state.agents.values().find(|a| a.place == place).map(|a| a.role.as_str().to_owned()).unwrap_or_default()
        }
    }
}

/// The view's kind of a place.
pub fn kind(state: &State, place: &str) -> PlaceKind {
    match crate::messages::places::kind(state, place) {
        crate::messages::places::PlaceKind::Main => PlaceKind::Main,
        crate::messages::places::PlaceKind::Room(card) => PlaceKind::Card { card },
        crate::messages::places::PlaceKind::Spec(card) => PlaceKind::Spec { card },
        crate::messages::places::PlaceKind::Challenge(card) => PlaceKind::Challenge { card },
        crate::messages::places::PlaceKind::Dm(card, role) => PlaceKind::Dm { card, role },
        crate::messages::places::PlaceKind::Child(card, agent) => {
            let name = agent.as_str().rsplit('/').next().unwrap_or_default();
            let role = crate::agents::children::role_of(name);
            PlaceKind::Child { card, role, agent }
        }
        crate::messages::places::PlaceKind::Other(_) => PlaceKind::Other,
    }
}
