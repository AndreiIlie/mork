//! Where things stand, in the lights and words the views and `factory status` show: a card's light (4.4.4), its
//! stage's word, an agent's state and what it does.

use factory_types::AgentKey;
use factory_types::ask::Color;
use factory_types::card::{Card, Stage};
use factory_types::view::{AgentLight, Doing};

use super::findings::folded_review;
use super::outside::Outside;
use super::pipeline::short_word;
use crate::state::State;

/// The card's light: none when done or closed; red with a red ask open; yellow with any other
/// ask open; green merging with nothing waiting; grey otherwise. Includes a folded review's asks.
pub fn card_light(state: &State, card: &Card) -> Option<Color> {
    if !card.stage.is_live() {
        return None;
    }
    let folded = folded_review(state, card).map(|rc| &rc.id);
    let open: Vec<Color> = state
        .asks
        .values()
        .filter(|a| a.open && (a.card.as_ref() == Some(&card.id) || (folded.is_some() && a.card.as_ref() == folded)))
        .map(|a| a.color)
        .collect();
    Some(if open.contains(&Color::Red) {
        Color::Red
    } else if !open.is_empty() {
        Color::Yellow
    } else if card.stage == Stage::Merging {
        Color::Green
    } else {
        Color::Grey
    })
}

pub fn color_word(c: Color) -> &'static str {
    match c {
        Color::Grey => "grey",
        Color::Green => "green",
        Color::Yellow => "yellow",
        Color::Red => "red",
    }
}

/// `factory status` (1.7): one line per live card in start order; the engine's answer and the offline read of the state
/// both print these (M5.24).
pub fn status_lines(state: &State) -> Vec<String> {
    let mut live: Vec<&Card> = state.cards.values().filter(|c| c.stage.is_live()).collect();
    live.sort_by_key(|c| (c.started_ms, c.id.to_string()));
    live.into_iter()
        .map(|c| {
            let asks = state.asks.values().filter(|a| a.open && a.card.as_ref() == Some(&c.id)).count();
            let paused = if c.paused { " (paused)" } else { "" };
            let light = card_light(state, c).map_or("none", color_word);
            let carries: String = c.carries.iter().map(|k| format!(" {}", k.id)).collect();
            let carries = if carries.is_empty() { carries } else { format!("  carries{carries}") };
            format!("{}  {}{paused}  {light}  asks {asks}  {}{carries}", c.id, c.stage.as_str(), c.title)
        })
        .collect()
}

/// What an agent row says it does (5.7.4).
pub fn doing(state: &State, agent: &AgentKey, outside: &Outside) -> Doing {
    let Some(a) = state.agents.get(agent) else { return Doing::Idle };
    match agent_light(state, agent, outside) {
        AgentLight::Working => {
            let seen = outside.agents.get(agent);
            let step = seen.and_then(|s| s.doing.clone().or_else(|| s.steps.last().cloned())).unwrap_or_default();
            Doing::Working { step }
        }
        AgentLight::Asking => Doing::WaitingOnYou,
        AgentLight::Error => Doing::Error { words: error_words(&a.status) },
        AgentLight::Waiting if outside.slot_waits.contains(agent) => Doing::WaitingForSlot,
        AgentLight::Waiting | AgentLight::Idle => Doing::Idle,
    }
}

fn error_words(status: &factory_types::state::AgentStatus) -> String {
    use factory_types::state::AgentStatus;
    match status {
        AgentStatus::Error { words } => words.clone(),
        AgentStatus::Cut => "cut twice in a row".into(),
        AgentStatus::Idle | AgentStatus::Working { .. } | AgentStatus::Stopped | AgentStatus::Waiting { .. } => {
            String::new()
        }
    }
}

/// An agent's state, as its square and icon draw it (5.6.1).
pub fn agent_light(state: &State, agent: &AgentKey, outside: &Outside) -> AgentLight {
    use factory_types::state::AgentStatus;
    let Some(a) = state.agents.get(agent) else { return AgentLight::Idle };
    let asking = state.asks.values().any(|k| k.open && k.agent.as_ref() == Some(agent));
    match &a.status {
        AgentStatus::Error { .. } | AgentStatus::Cut => AgentLight::Error,
        _ if asking => AgentLight::Asking,
        AgentStatus::Working { .. } if outside.slot_waits.contains(agent) => AgentLight::Waiting,
        AgentStatus::Working { .. } => AgentLight::Working,
        AgentStatus::Waiting { .. } => AgentLight::Waiting,
        AgentStatus::Idle | AgentStatus::Stopped if a.owed.is_some() => AgentLight::Waiting,
        AgentStatus::Idle | AgentStatus::Stopped => AgentLight::Idle,
    }
}

/// A card's short human step word for sidebar, board rows, and room rows (fac-680).
pub fn stage_word(state: &State, card: &Card) -> String {
    short_word(state, card)
}
