//! A visual card's baseline step (fac-636, fac-688): before agreeing the specification, the foreman's baseline seat
//! reproduces today's pages in the mock's folder, which the specification holder then changes at brief.

use mork_types::AgentKey;
use mork_types::card::{Card, Mock, MockStep, Mode, Stage};
use mork_types::config::Config;
use mork_types::reply::{Outcome, Reply, Role};
use mork_types::state::{Agent, Job, Sender};
use serde_json::json;

use super::mock::{self, FOLDER};
use crate::decide::Ctx;
use crate::messages::places::{self, Who};
use crate::messages::requests::{agent_of, ask_agent, tell_agent};
use crate::post::Poster;
use crate::state::State;

/// The engine's request to the foreman for the baseline: only a `built` from a turn that read the latest ends the step.
pub const HEADLINE: &str = "Build the baseline";
// Posted to a holder already at brief when its baseline ends: a recap counts only from a turn launched after it.
const READY: &str = "The baseline is ready";

/// Whether a card starts its mock at a baseline step: any visual full card.
pub(super) fn wanted(c: &Card) -> bool {
    c.visual && c.mode == Mode::Full
}

/// Opens the card's mock at its baseline step, its host's port taken, and asks the foreman for the baseline.
pub(super) fn open(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    let id = c.id.clone();
    let port = c.mock.and_then(|m| m.port).or_else(|| super::start::port(p, ctx));
    c.mock = Some(Mock { step: MockStep::Baseline, port, ..Mock::default() });
    p.put_card(c.clone());
    ask_agent(p, &id, Role::Foreman, HEADLINE, &request(&c, ctx.config));
    p.journal("mock.opened", Some(id.as_str()), json!({"port": port, "baseline": true}));
}

fn request(c: &Card, config: &Config) -> String {
    let id = &c.id;
    let (wait, after) = if c.no_planner {
        ("before agreeing the specification", "lets you begin the visual brief")
    } else {
        ("the planner's turns wait for it", "lets the planner on")
    };
    format!(
        "{id} is a visual card: its mock is made in the spec conversation, before the plan's approve, on a baseline of \
         the pages it changes as they stand today. Build that baseline now: {wait}. Read the \
         card and the code and name those pages. Then spawn the card's baseline seat, `mork ctl spawn {id} baseline \
         '<task>'`, without a model or effort so its routing mode applies, and task it to reproduce each page as it \
         stands today in the clone's `{FOLDER}/`, {}, as the `visual-mocks` skill's \"Visual mode\" says. \
         It keeps any mock already in the folder, an earlier run's restored one included, and fills only the surfaces \
         it lacks. Write nothing yourself: the baseline seat alone writes the folder. Once its report says the \
         baseline is done, end `built`: that ends the baseline step, releases the seat and {after}, and \
         submits nothing. When the card has no existing page to reproduce, spawn nothing and end `built` at once, \
         saying so.",
        mock::served(c, config)
    )
}

/// A visual card planner's first request's mock paragraph: its turns wait for the baseline, then the mock is its own.
pub(super) fn first_note(c: &Card, config: &Config) -> String {
    if !wanted(c) || c.no_planner {
        return String::new();
    }
    format!(
        "\n\n{} is a visual card: before your first turn the foreman's baseline seat reproduces the pages it changes, \
         as they stand today, in your clone's `{FOLDER}/`, {}. {}",
        c.id,
        mock::served(c, config),
        mock::at_brief_rule(c)
    )
}

/// Whether `who`'s reply is the foreman's `built` ending the card's baseline step.
pub(super) fn ends(p: &Poster<'_>, card: &Card, who: &Who, r: &Reply, job: &Job) -> bool {
    who.role == Role::Foreman
        && who.place.as_str() == card.id.as_str()
        && r.outcome == Outcome::Built
        && card.stage == Stage::Brief
        && mock::baseline(card)
        && mock::read_request(p, who.place.as_str(), HEADLINE, card.first_seq, job)
}

/// Ends the baseline step on its foreman's `built` from a turn that read the request: the mock is the specification
/// holder's, the baseline seats are released. Whether the reply was that `built`, which submits nothing.
pub(super) fn built(p: &mut Poster<'_>, card: &Card, (who, r, job): (&Who, &Reply, &Job), ctx: Ctx<'_>) -> bool {
    if !ends(p, card, who, r, job) {
        return false;
    }
    let mut c = card.clone();
    if let Some(m) = c.mock.as_mut() {
        m.step = MockStep::Open;
    }
    p.put_card(c.clone());
    crate::agents::children::release_baselines(p, &c.id, false, ctx.config);
    p.journal("mock.baseline-ended", Some(c.id.as_str()), json!({}));
    notify_ready(p, &c, ctx.config);
    true
}

fn notify_ready(p: &mut Poster<'_>, c: &Card, config: &Config) {
    let ready_text = format!(
        "{}'s baseline is ready in your clone's `{FOLDER}/`, {}. {} A recap from a turn that began before this \
         request isn't offered. Go on agreeing the specification.",
        c.id,
        mock::served(c, config),
        mock::at_brief_rule(c)
    );
    if c.no_planner {
        if asked_agree(p, c) {
            ask_agent(p, &c.id, Role::Foreman, READY, &ready_text);
        } else {
            let text = format!("{}{}\n\n{ready_text}", super::start::first(c), super::start::carrying(c));
            ask_agent(p, &c.id, Role::Foreman, super::start::AGREE_HEADLINE, &text);
        }
        return;
    }
    // A planner yet to run takes its first request now; one already at brief is told the mock is now its own.
    let planner = agent_of(&c.id, Role::Planner).key;
    let ran = p.state.jobs.values().any(|j| j.owner == planner && j.seen.is_some_and(|s| s > c.first_seq));
    if ran || p.agent(&planner).is_some_and(|a| a.session.is_some() || a.session_started) {
        ask_agent(p, &c.id, Role::Planner, READY, &ready_text);
    }
}

fn asked_agree(p: &Poster<'_>, c: &Card) -> bool {
    let room = places::room_of(&c.id, super::facts::holder(c));
    p.state
        .messages_in(&room)
        .any(|m| m.seq >= c.first_seq && m.from == Sender::Engine && m.headline == super::start::AGREE_HEADLINE)
}

/// Withholds a visual card's recap made while its baseline is built, or by a turn launched before it was ready,
/// telling its specification holder why: whether it was withheld.
pub(super) fn recap_withheld(p: &mut Poster<'_>, c: &Card, who: &Who, job: &Job) -> bool {
    let holder = super::facts::holder(c);
    if !c.visual || who.role != holder {
        return false;
    }
    let ready = |m: &&mork_types::state::Message| {
        m.seq >= c.first_seq
            && m.from == Sender::Engine
            && (m.headline == READY || (c.no_planner && m.headline == super::start::AGREE_HEADLINE))
    };
    let room = places::room_of(&c.id, holder);
    let last = p.state.messages_in(&room).filter(ready).last().map(|m| m.seq);
    let early = last.is_some_and(|seq| job.seen.is_none_or(|seen| seen <= seq));
    let text = if mock::baseline(c) {
        if c.no_planner {
            "End the baseline step with `built`, then follow the visual brief to draft the mock and recap."
        } else {
            "Once the foreman ends the baseline step, follow the visual brief to draft the mock and recap."
        }
    } else if early {
        "This turn began before the baseline was ready, so follow the visual brief in your next turn to draft the mock and recap."
    } else {
        return false;
    };
    tell_agent(p, &c.id, holder, "recap not offered: the baseline first", text);
    p.journal("brief.withheld", Some(c.id.as_str()), json!({"baseline": true}));
    true
}

/// Whether a seat's turn waits for the mock's folder: one writer at a time between the baseline seat and the card's
/// mock holder (its planner, or its room foreman on a no-planner card once the mock is open).
pub fn waits(state: &State, a: &Agent) -> bool {
    if a.retiring.is_some() || !matches!(a.role, Role::Planner | Role::Foreman | Role::Baseline) {
        return false;
    }
    let Some(c) = places::card_of(state, &a.place).and_then(|id| state.cards.get(&id)) else { return false };
    if a.role == Role::Foreman && (!c.no_planner || a.place.as_str() != c.id.as_str() || !mock::writes(c)) {
        return false;
    }
    if a.role == Role::Planner && mock::baseline(c) {
        return true;
    }
    // By key, as `mocker::ending` does: a released seat's turn may still run, a replaced baseline seat's included.
    let baseline = |name: &str| crate::agents::children::role_of(name) == Some(Role::Baseline);
    let mocker = |name: &str| {
        a.role == Role::Baseline
            && ((!c.no_planner && name == "planner") || (c.no_planner && mock::writes(c) && name == "foreman"))
    };
    let writes = |owner: &AgentKey| {
        owner != &a.key
            && owner
                .as_str()
                .split_once('/')
                .is_some_and(|(card, name)| card == c.id.as_str() && (baseline(name) || mocker(name)))
    };
    state.jobs.values().any(|j| j.settled.is_none() && j.purpose.is_none() && writes(&j.owner))
}

/// Visual mode turned on at brief: a waiting recap withdrawn, the mock opened at its baseline step, the foreman asked.
pub(super) fn turned_on(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    super::brief::redraft(p, &mut c);
    open(p, c, ctx);
}

/// Visual mode turned off at brief: the mock skipped, its baseline seats released and stopped, a holder held by the
/// baseline asked to go on, and a delegated card's waiting plan re-offered without the mock link so its ask is given.
pub(super) fn turned_off(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    let id = c.id.clone();
    let under_way = mock::baseline(&c);
    let mock = c.mock.as_mut().filter(|m| matches!(m.step, MockStep::Baseline | MockStep::Open));
    let ended = mock.map(|m| m.step = MockStep::Skipped).is_some();
    let waiting = c.delegated && c.recap;
    let holder = super::facts::holder(&c);
    let first_agree = under_way && c.no_planner && !asked_agree(p, &c);
    let agree_body = first_agree.then(|| format!("{}{}", super::start::first(&c), super::start::carrying(&c)));
    p.put_card(c.clone());
    crate::agents::children::release_baselines(p, &id, true, ctx.config);
    if ended {
        p.journal("mock.ended", Some(id.as_str()), json!({"step": MockStep::Skipped}));
    }
    if under_way {
        let text = match agree_body {
            Some(body) => format!(
                "The operator turned {id}'s visual mode off while its baseline was built: there is no mock.\n\n{body}"
            ),
            None => format!(
                "The operator turned {id}'s visual mode off while its baseline was built: there is no mock. Go on agreeing \
                 the specification."
            ),
        };
        ask_agent(p, &id, holder, super::start::AGREE_HEADLINE, &text);
    }
    if waiting {
        super::brief::offer(p, &mut c, ctx, None);
        p.put_card(c);
    }
}
