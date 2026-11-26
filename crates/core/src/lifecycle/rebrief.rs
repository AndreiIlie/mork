//! `brief <card> [words]` (fac-638): an approved full card sent back to brief in the same run by the operator or its own
//! foreman, its whole submission withdrawn and its build held until its new plan's approve.

use mork_types::CardId;
use mork_types::card::{Card, Mode};
use mork_types::reply::Role;
use mork_types::state::{Agent, AgentStatus, Job, Owed, Sender};
use serde_json::json;

use super::reopen::Back;
use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

/// `brief <card> [words]`, past the buzz: withdraws the card's submission and asks its specification's holder for a
/// new plan, carrying the words.
pub(super) fn back(state: &State, from: &Sender, card: &CardId, words: Option<&str>, ctx: Ctx<'_>) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(c) = state.cards.get(&id).cloned() else { return Decision::refused(format!("no card `{card}`")) };
    if let Err(why) = briefable(state, &c) {
        return Decision::refused(why);
    }
    let mut p = Poster::for_ctx(state, ctx);
    let was = c.stage;
    let c = super::reopen::withdraw(&mut p, c, Back::Brief, ctx);
    let boundary = p.next_seq;
    drop_old_work(&mut p, &id, boundary);
    let asked = ask(&mut p, &c, from, words.map(str::trim).filter(|w| !w.is_empty()));
    if let Some(mut c) = p.card(&id) {
        c.rebrief.iter_mut().for_each(|r| r.asked = Some(asked));
        p.put_card(c);
    }
    let by = from.label();
    p.journal("card.rebriefed", Some(id.as_str()), json!({"from": was.as_str(), "by": by, "seq": asked}));
    let mut d = Decision::ok(json!({"brief": {"card": id.as_str()}}));
    p.finish(&mut d);
    d
}

/// Why the card can't go back to brief now, or `Ok`: a live full card with an approved plan, past brief, no mock open,
/// until its final landing starts.
pub fn briefable(state: &State, c: &Card) -> Result<(), String> {
    let id = &c.id;
    if c.mode != Mode::Full {
        return Err(format!("{id} isn't a full card: it has no plan to agree again"));
    }
    if !c.stage.is_live() {
        return Err(format!("{id} has ended: it is {}", c.stage.as_str()));
    }
    if back_at(c) {
        return Err(format!("{id} is already back at brief"));
    }
    if super::facts::agreeing(c) || c.approval.is_none() {
        return Err(format!("{id} has no approved plan yet: it is at {}", c.stage.as_str()));
    }
    if super::mock::open(c) {
        return Err(format!("{id}'s mock is open: its approve comes first"));
    }
    if super::reopen::landing_started(state, c) {
        return Err(format!("{id}'s final landing has started: it can't go back to brief"));
    }
    Ok(())
}

/// Whether the card is back at brief: sent there after its plan's approval, its new plan not approved yet.
pub fn back_at(c: &Card) -> bool {
    c.rebrief.is_some() && super::facts::agreeing(c)
}

/// Whether a recap counts: back at brief, only one of a turn whose news reached the request to agree the new plan.
pub fn read_request(c: &Card, job: &Job) -> bool {
    let asked = c.rebrief.filter(|_| super::facts::agreeing(c)).and_then(|r| r.asked);
    asked.is_none_or(|seq| job.seen.is_some_and(|seen| seq < seen))
}

fn drop_old_work(p: &mut Poster<'_>, card: &CardId, asked: u64) {
    let key = crate::messages::requests::agent_of(card, Role::Foreman).key;
    let Some(mut a) = p.agent(&key).filter(|a| a.retiring.is_none()) else { return };
    let retry_seen = match &a.owed {
        Some(Owed::Retry { seen, .. }) => Some(*seen),
        Some(Owed::Correction { of, .. }) => p.state.jobs.get(of).map(|j| j.seen),
        _ if matches!(a.status, AgentStatus::Stopped | AgentStatus::Error { .. } | AgentStatus::Cut) => {
            crate::agents::seats::last_job(p.state, &key).map(|j| j.seen)
        }
        _ => None,
    };
    if retry_seen.is_some_and(|seen| before(asked, seen)) {
        discard(p.state, &mut a);
        p.put_agent(a);
    }
}

fn before(asked: u64, seen: Option<u64>) -> bool {
    seen.is_none_or(|seen| seen <= asked)
}

/// An old build may finish, but its retry must give way to the new brief's requests.
pub fn ended_stale(state: &State, a: &mut Agent, job: &Job) {
    if a.role != Role::Foreman || a.retiring.is_some() {
        return;
    }
    let card = crate::messages::places::card_of(state, &a.place).and_then(|id| state.cards.get(&id));
    let Some(c) = card.filter(|c| a.place.as_str() == c.id.as_str()) else { return };
    if c.rebrief.and_then(|r| r.asked).is_some_and(|asked| before(asked, job.seen)) {
        discard(state, a);
    }
}

fn discard(state: &State, a: &mut Agent) {
    let corrected = crate::agents::ends::owed_correction(a);
    let returned = corrected.is_some_and(|of| crate::agents::ends::give_back(state, a, &of));
    let wake = a.wake_after
        || matches!(a.owed, Some(Owed::Wake))
        || returned
        || crate::agents::prompt::unread_request(state, a);
    (a.owed, a.last_prompt, a.not_before, a.wake_after, a.cuts) = (wake.then_some(Owed::Wake), None, None, false, 0);
    if !matches!(a.status, AgentStatus::Working { .. } | AgentStatus::Stopped) {
        a.status = AgentStatus::Idle;
    }
}

// The request to the specification's holder, in its own room: the planner's, or a no-planner card's foreman's.
fn ask(p: &mut Poster<'_>, c: &Card, from: &Sender, words: Option<&str>) -> u64 {
    let id = &c.id;
    let who = if *from == Sender::Operator { "the operator" } else { "its foreman" };
    let said = words.map(|w| format!("\n\nThe words it was sent back with:\n\n{w}")).unwrap_or_default();
    let next = if c.no_planner {
        "Once nothing needs clarifying, recap it as at its first brief and end `brief-ready`: it is offered at once."
    } else {
        "Once nothing needs clarifying, recap it as at its first brief and end `brief-ready`: the foreman challenges it, then it is offered under the card's delegation level."
    };
    let text = format!(
        "{id} is back at brief: {who} sent it back after its plan's approval. Its submission is withdrawn and its build held until a new plan's approve; its commits stay. Agree its new plan as the `agreeing-a-specification` skill says, from its approved plan, the decisions on its card and what its build and review found: the plan you recap replaces the approved one whole. Only a recap from a turn that read this request counts. {next} Its approve continues the build from the new plan, with no mock step.{said}"
    );
    crate::messages::requests::ask_agent(p, id, super::facts::holder(c), "Agree the new plan", &text)
}
