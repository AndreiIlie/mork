//! A full card's foreman challenges each recap and owns the build after approval.

use std::fmt::Write as _;

use factory_types::AgentKey;
use factory_types::ask::{AskKind, Color};
use factory_types::card::{Card, Mode, Stage};
use factory_types::config::Config;
use factory_types::reply::{Intent, Outcome, Reply, Role};
use factory_types::state::{Agent, Job, Message, MessageKind, Sender};
use serde_json::json;

use crate::agents::prompt::held::Sections;
use crate::decide::Ctx;
use crate::messages::places::{self, Who};
use crate::messages::requests::agent_of;
use crate::post::Poster;
use crate::state::State;

const CHALLENGE: &str = "Challenge the plan";

/// Asks the card's foreman to challenge the standing plan in the challenge place, superseding any earlier request.
/// Mutates `c` in place for its caller to stage and returns whether a challenge now waits (fac-616, fac-644).
pub fn challenge(p: &mut Poster<'_>, c: &mut Card, config: &Config) -> bool {
    if !full(c) || c.no_planner || c.draft.is_none() {
        return false;
    }
    let card = c.id.clone();
    crate::messages::asks::close(p, &places::spec_room(&card), &[AskKind::ApproveBrief]);
    let text = format!(
        "Challenge {card}'s plan, given in your prompt's The challenge section: the specification and its code's shape. Read the card and the code for \
         the facts and what to reuse, and test the plan against your knowledge, best practices and outside \
         sources. Write, edit and commit nothing. Send every consequential objection to `planner` as one request, \
         strongest first, with what you would do instead, then end `working`. With none, end `working` with no \
         request to the planner, an empty `ask` and no red hand: the engine then offers the plan. Your reply \
         counts for this plan alone: a newer recap is challenged again.{}",
        mocked(c, config)
    );
    let seq = crate::messages::requests::ask_challenge(p, &card, CHALLENGE, &text);
    c.challenge = Some(seq);
    p.journal("challenge.asked", Some(card.as_str()), json!({"seq": seq}));
    true
}

// A visual card's recap is its plan and its mock together, so the challenge judges both (fac-636).
fn mocked(c: &Card, config: &Config) -> String {
    if !crate::lifecycle::mock::briefing(c) {
        return String::new();
    }
    let at = crate::lifecycle::mock::link(c, config).map_or_else(
        || format!("in the clone's `{}/`, which nothing serves", crate::lifecycle::mock::FOLDER),
        |link| format!("served at {link}"),
    );
    format!("\n\n{} is a visual card: the plan is its plan and its mock together, the mock {at}.", c.id)
}

fn full(c: &Card) -> bool {
    c.mode == Mode::Full
}

/// The challenge waiting on a brief card's foreman, by its request's sequence.
pub(crate) fn waiting(c: &Card, key: &AgentKey) -> Option<u64> {
    let foreman = full(c) && c.stage == Stage::Brief && *key == agent_of(&c.id, Role::Foreman).key;
    c.challenge.filter(|_| foreman)
}

/// `waiting`, on an agent's card as the state has it.
pub(crate) fn waiting_on(state: &State, a: &Agent) -> Option<u64> {
    waiting(state.cards.get(&places::card_of(state, &a.place)?)?, &a.key)
}

/// The waiting challenge this turn's session read: its news reached past the request, and a new session is given
/// it again (`fresh::replace`). Bound as a review verdict is: a retry or reload keeps the turn's news cursor.
pub(crate) fn bound(c: &Card, key: &AgentKey, job: &Job) -> Option<u64> {
    waiting(c, key).filter(|s| job.seen.is_some_and(|seen| *s < seen))
}

/// Takes the brief foreman's reply to the challenge its turn read, mutating `c` in place for its caller to stage
/// and returning whether it answered a waiting challenge (fac-644).
pub fn replied(p: &mut Poster<'_>, c: &mut Card, who: &Who, r: &Reply, job: &Job, ctx: Ctx<'_>) -> bool {
    let Some(seq) = bound(c, &who.key, job) else { return false };
    if !matches!(r.outcome, Outcome::Working | Outcome::None) {
        return false;
    }
    let objected = objected(p.state, who, r, job);
    if !objected && (!r.hand.ask.trim().is_empty() || r.hand.color == Color::Red) {
        return false;
    }
    c.challenge = None;
    p.journal("challenge.answered", Some(c.id.as_str()), json!({"seq": seq, "objections": objected}));
    if !objected {
        crate::lifecycle::brief::offer(p, c, ctx);
    }
    true
}

// A request said during the turn, which the challenge place keeps, counts as one in the reply does; a message with no
// kind went as a request.
fn objected(state: &State, who: &Who, r: &Reply, job: &Job) -> bool {
    let me = Sender::Agent(who.key.clone());
    let seen = job.seen.unwrap_or(u64::MAX);
    let Some(card) = places::card_of(state, &who.place) else { return false };
    let planner = |to: &str| {
        let to = to.trim().to_lowercase();
        to == "planner" || to == format!("{card}/planner")
    };
    let request = |m: &Message| m.intent.map_or(m.kind == MessageKind::Request, |i| i == Intent::Request);
    let said = state
        .messages_in(&places::challenge_place(&card))
        .any(|m| m.seq >= seen && m.from == me && m.to.as_deref().is_some_and(planner) && request(m));
    said || r.messages.iter().any(|m| planner(&m.to) && m.kind.is_none_or(|k| k == Intent::Request))
}

const BUILD_RULE: &str = "You build every deliverable. The `launch` command has been removed.";

pub(crate) fn build_rule(state: &State, a: &Agent, sections: &mut Sections<'_>, out: &mut String) {
    if a.role != Role::Foreman {
        return;
    }
    let Some(card) = places::card_of(state, &a.place).and_then(|id| state.cards.get(&id)) else { return };
    if full(card)
        && a.place.as_str() == card.id.as_str()
        && !crate::lifecycle::facts::agreeing(card)
        && sections.sends("build-rule", BUILD_RULE)
    {
        let _ = write!(out, "\n\n## Your build\n\n{BUILD_RULE}\n");
    }
}

/// The brief foreman's challenge and, if requested, the waiting plan.
pub(crate) fn section(state: &State, a: &Agent, (s, plan): (&mut Sections<'_>, bool), out: &mut String) {
    let Some(card) = places::card_of(state, &a.place).and_then(|c| state.cards.get(&c)) else { return };
    if !matches!(card.stage, Stage::Starting | Stage::Brief) || !full(card) {
        return;
    }
    if a.role != Role::Foreman || a.place.as_str() != card.id.as_str() || card.no_planner {
        return;
    }
    let rule = foreman_rule(card);
    out.push_str(if s.sends("challenge", &rule) { &rule } else { "\n## The challenge\n\n" });
    let _ = writeln!(out, "Where it stands: {}.", foreman_now(card));
    if let Some(d) = card.draft.as_deref().filter(|_| plan && card.challenge.is_some()) {
        the_plan(d.trim(), s, out);
    }
}

// The plan the challenge is of: whole, or as a diff of the one its session holds, nothing when equal (fac-623).
fn the_plan(plan: &str, s: &mut Sections<'_>, out: &mut String) {
    let old = s.holds("plan");
    s.stage("plan", plan);
    let Some(old) = old.filter(|old| *old != plan) else {
        if old.is_none() {
            let _ = write!(out, "\nThe plan:\n\n{plan}\n");
        }
        return;
    };
    let diff = crate::maintenance::material::diff(&format!("{old}\n"), &format!("{plan}\n"));
    let mut fence = "```".to_owned();
    while diff.contains(&fence) {
        fence.push('`');
    }
    let _ = write!(
        out,
        "\nThe plan changed since your session last read it. Removed lines start with `-`, added lines with `+`; surrounding lines give context.\n\n{fence}diff\n{diff}{fence}\n"
    );
}

fn foreman_now(card: &Card) -> &'static str {
    if card.stage == Stage::Starting {
        "no plan yet: the planner is drafting the first, and its recap is challenged"
    } else if card.challenge.is_some() {
        "a challenge waits: answer the latest \"Challenge the plan\" request"
    } else {
        "no challenge waits: the planner is settling the plan, and its next recap is challenged"
    }
}

fn foreman_rule(card: &Card) -> String {
    format!(
        "\n## The challenge\n\n{} is agreeing its specification: its planner holds it, with Andrei in the card's \
         spec room, and you challenge each plan it recaps before the plan is offered. Your messages to `planner` \
         and its to you pass in the card's challenge place, which no window shows. Read the card and the code. \
         Write, edit and commit nothing. Andrei's words in this build room reach you at every \
         stage: answer them, and send what would change the specification to `planner` as a request. Send your \
         objections to `planner` as one request, strongest first, and end `working`. \
         With none, end `working` with no request to the planner, an empty `ask` and no red hand. \
         If the planner refuses an objection you still hold, tell it so as a request with your reasons: the planner \
         raises both positions in an ask, one objection at a time, at every delegation level. The plan's \
         approve hands you its build in this session.\n\n",
        card.id
    )
}
