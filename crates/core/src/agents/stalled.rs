//! A stalled non-review card nudges the seat it waits on, once per stall: its seats last said `working`, its children
//! `working`, finished or blocked, and nothing is running, owed or awaited (fac-379, fac-380, fac-415, fac-587).

use factory_types::AgentKey;
use factory_types::action::ActionStatus;
use factory_types::card::{Card, Landing, Mode};
use factory_types::reply::{Intent as Kind, Outcome, Role};
use factory_types::state::{Agent, AgentStatus, Job, JobStatus, Message, MessageKind, ORCHESTRATOR, Sender};
use serde_json::json;

use crate::decide::Intent;
use crate::messages::requests::agent_of;
use crate::post::Poster;
use crate::state::State;

const WAKE: &str = "You last replied `working`, your children have finished, are blocked or last replied `working`, no \
    request to the orchestrator is waiting on its answer, and nothing is running or owed for this card, so nothing would \
    ever wake you. Look at where the card stands and carry it on.";

// How long a seat's request waits on its orchestrator's answer (fac-380): a wake that never reached it lapses.
const AWAITED_MS: i64 = 60 * 60 * 1000;

enum Awaited {
    None,
    Waiting,
    Expired,
}

// The turn the stall check leaves aside: a reply's own, ending, or the one a stall's nudge brought its seat.
#[derive(Clone, Copy)]
enum Aside<'a> {
    Nothing,
    Ending(&'a Job),
    Nudged(&'a AgentKey),
}

impl Aside<'_> {
    fn ending(self, key: &AgentKey) -> bool {
        matches!(self, Self::Ending(j) if j.owner == *key)
    }

    fn nudged(self, key: &AgentKey) -> bool {
        matches!(self, Self::Nudged(k) if k == key)
    }

    fn job(self, j: &Job) -> bool {
        match self {
            Self::Nothing => false,
            Self::Ending(e) => e.id == j.id,
            Self::Nudged(k) => j.owner == *k && j.purpose.is_none(),
        }
    }
}

/// Nudges the seat each live card that has stalled waits on, waking it, once per stall (fac-587).
pub fn wake(p: &mut Poster<'_>) {
    let ids: Vec<_> = p.state.cards.keys().cloned().collect();
    for id in ids {
        let Some(mut card) = p.card(&id) else { continue };
        if let Some(seq) = card.stalled {
            // The stall lasts through the nudged seat's own turn: anything else that moves the card ends it.
            let nudged = nudged(p, &card, seq);
            if !stalled(p, &card, nudged.as_ref().map_or(Aside::Nothing, Aside::Nudged)) {
                card.stalled = None;
                p.put_card(card);
            }
            continue;
        }
        if !stalled(p, &card, Aside::Nothing) {
            continue;
        }
        let who = agent_of(&id, waited_on(p, &card));
        let text = text(p, &card);
        let to = Some(who.role.as_str());
        let seq = p.post(who.place.as_str(), Sender::Engine, to, MessageKind::Request, "Stalled card", &text);
        // Never held as a request to a recap's planner is: nothing else would bring this seat a turn.
        p.wake(&who, &Sender::Engine);
        card.stalled = Some(seq);
        p.put_card(card);
        p.journal("card.stalled", Some(id.as_str()), json!({}));
    }
}

fn text(p: &Poster<'_>, card: &Card) -> String {
    let expired: Vec<_> = seats(p, card)
        .into_iter()
        .filter(|a| matches!(awaited(p, card, a), Awaited::Expired))
        .filter_map(|a| a.key.as_str().rsplit('/').find(|s| *s != "dm").map(|s| format!("`{s}`")))
        .collect();
    let clause = match expired.as_slice() {
        [] => return WAKE.to_owned(),
        [seat] => {
            format!(
                "{seat}'s request to the orchestrator has had no answer for over an hour and is no longer waited on"
            )
        }
        _ => format!(
            "requests from {} to the orchestrator have had no answer for over an hour and are no longer waited on",
            expired.join(" and ")
        ),
    };
    WAKE.replace("no request to the orchestrator is waiting on its answer", &clause)
}

// The seat a stall's nudge went to, by its message's addressee.
fn nudged(p: &Poster<'_>, c: &Card, seq: u64) -> Option<AgentKey> {
    let messages = &p.state.messages;
    let index = messages.binary_search_by_key(&seq, |m| m.seq).ok()?;
    let m = &messages[index];
    let role = [Role::Planner, Role::Foreman].into_iter().find(|r| m.to.as_deref() == Some(r.as_str()))?;
    Some(agent_of(&c.id, role).key)
}

// The seat a stalled card waits on: its foreman while its challenge of the plan waits or its baseline is built (fac-636),
// the seat owing a challenge-place reply (fac-666), else its holder.
fn waited_on(p: &Poster<'_>, c: &Card) -> Role {
    if challenged(c) || crate::lifecycle::mock::baseline(c) {
        Role::Foreman
    } else if let Some(role) = challenge_owed(p, c) {
        role
    } else {
        crate::lifecycle::facts::holder(c)
    }
}

// Unanswered challenge-place requests between planner and foreman count as owed work in the current phase (fac-666).
fn challenge_owed(p: &Poster<'_>, c: &Card) -> Option<Role> {
    if !crate::messages::places::two_rooms(c) {
        return None;
    }
    let since = challenge_floor(p, c);
    let planner = Sender::Agent(agent_of(&c.id, Role::Planner).key);
    let foreman = Sender::Agent(agent_of(&c.id, Role::Foreman).key);
    let place = crate::messages::places::challenge_place(&c.id);
    let mut pending = None;
    for m in p.state.messages_in(&place).filter(|m| m.seq >= since) {
        let req = m.intent.map_or(m.kind == MessageKind::Request, |i| i == Kind::Request);
        if m.from == Sender::Engine {
            pending = None;
        } else if m.from == planner && m.to.as_deref() == Some("foreman") {
            pending = next_pending(pending, Role::Foreman, req);
        } else if m.from == foreman && m.to.as_deref() == Some("planner") {
            pending = next_pending(pending, Role::Planner, req);
        }
    }
    pending
}

fn next_pending(pending: Option<Role>, target: Role, request: bool) -> Option<Role> {
    if request {
        Some(target)
    } else if pending.is_some_and(|r| r != target) {
        None
    } else {
        pending
    }
}

// Bounds challenge-place scans to the current brief or build phase so prior-phase requests cannot leak across approval.
fn challenge_floor(p: &Poster<'_>, c: &Card) -> u64 {
    if crate::lifecycle::facts::agreeing(c) {
        return c.rebrief.and_then(|r| r.asked).unwrap_or(c.first_seq);
    }
    p.state
        .messages_in(c.id.as_str())
        .filter(|m| m.seq >= c.first_seq && m.from == Sender::Engine)
        .filter(|m| matches!(m.headline.as_str(), "Build it" | "Continue the build"))
        .map(|m| m.seq)
        .max()
        .unwrap_or(c.first_seq)
}

fn challenged(c: &Card) -> bool {
    super::required::waiting(c, &agent_of(&c.id, Role::Foreman).key).is_some()
}

/// Whether the card would wait on this reply alone, excluding its ending turn and the foreman's last outcome.
pub(crate) fn waits_after(p: &Poster<'_>, c: &Card, a: &Agent, job: &Job) -> bool {
    waited_on(p, c) == a.role && job.delivered.iter().all(|s| a.taken.contains(s)) && stalled(p, c, Aside::Ending(job))
}

// The foreman sits out agreement until a challenge or challenge-place request waits (fac-633, fac-666).
// Baseline holds its planner's owed turn (fac-636), as an open mock holds its foreman (fac-688).
fn seats(p: &Poster<'_>, c: &Card) -> Vec<Agent> {
    let agreeing = crate::lifecycle::facts::agreeing(c);
    let baseline = crate::lifecycle::mock::baseline(c);
    let waits = !crate::messages::places::two_rooms(c) || challenged(c) || baseline;
    let owed = challenge_owed(p, c);
    let kept = |a: &Agent| {
        let working = matches!(a.status, AgentStatus::Working { .. });
        match a.role {
            Role::Planner if baseline => working,
            Role::Planner => agreeing || a.owed.is_some() || a.wake_after || working || owed == Some(Role::Planner),
            Role::Foreman if crate::lifecycle::mock::holds(p.state, a) => false,
            Role::Foreman if agreeing && !waits && a.place.as_str() == c.id.as_str() => {
                working || owed == Some(Role::Foreman)
            }
            Role::Foreman | Role::Reviewer | Role::Researcher | Role::Baseline | Role::Echo => true,
        }
    };
    p.state
        .agents
        .keys()
        .filter_map(|k| p.agent(k))
        .filter(|a| crate::messages::places::card_of(p.state, &a.place).as_ref() == Some(&c.id))
        .filter(kept)
        .collect()
}

fn stalled(p: &Poster<'_>, c: &Card, aside: Aside<'_>) -> bool {
    if !c.stage.is_live() || c.paused || c.mode == Mode::Review {
        return false;
    }
    let seats = seats(p, c);
    let waited = agent_of(&c.id, waited_on(p, c)).key;
    if !seats.iter().any(|a| a.key == waited) {
        return false;
    }
    seats.iter().all(|a| aside.ending(&a.key) || replied(c, a))
        && seats.iter().all(|a| idle(a, aside))
        && !asked(p, c)
        && !busy(p, c, &seats, aside)
        && !seats.iter().any(|a| matches!(awaited(p, c, a), Awaited::Waiting))
}

// A child's finished or blocked reply already woke its foreman (fac-380); any other seat must have last said `working`,
// and one that never replied keeps the wake off unless a child has yet to run.
fn replied(c: &Card, a: &Agent) -> bool {
    match a.replied {
        Some(Outcome::Working) => true,
        // The recap under challenge waits on its foreman, not on its planner (fac-587).
        Some(Outcome::BriefReady) => a.role == Role::Planner && challenged(c),
        Some(Outcome::Built | Outcome::Reported | Outcome::ReviewPass | Outcome::ReviewFail | Outcome::Blocked) => {
            super::children::is_child(a)
        }
        None => super::children::is_child(a) && a.ran.turns == 0 && a.session.is_none(),
        Some(_) => false,
    }
}

// Keep matching within this run and the seat's own place (fac-380), so another seat or an earlier run cannot
// answer the latest request.
pub(crate) fn unanswered_orchestrator_request<'a>(
    state: &'a State,
    c: &Card,
    key: &AgentKey,
    place: &str,
) -> Option<&'a Message> {
    let me = Sender::Agent(key.clone());
    let orchestrator = Sender::Orchestrator(c.project.clone());
    let label = key.as_str().rsplit('/').find(|s| *s != "dm");
    let (mut asked, mut answered): (Option<&Message>, u64) = (None, 0);
    for m in state.messages_in(place).filter(|m| m.seq >= c.first_seq) {
        if m.from == me && to_orchestrator(m) && asked.is_none_or(|r| m.seq > r.seq) {
            asked = Some(m);
        } else if m.from == orchestrator && m.to.as_deref() == label && matches!(m.intent, None | Some(Kind::Response))
        {
            answered = answered.max(m.seq);
        }
    }
    asked.filter(|r| r.seq > answered)
}

fn awaited(p: &Poster<'_>, c: &Card, a: &Agent) -> Awaited {
    match unanswered_orchestrator_request(p.state, c, &a.key, a.place.as_str()) {
        Some(r) if p.now < r.time + AWAITED_MS => Awaited::Waiting,
        Some(_) => Awaited::Expired,
        None => Awaited::None,
    }
}

// The stream's rule for the one message that wakes an orchestrator (fac-286, messages::streams::record).
fn to_orchestrator(m: &Message) -> bool {
    matches!(m.from, Sender::Agent(_))
        && m.intent == Some(Kind::Request)
        && m.to.as_deref().is_some_and(|t| t.starts_with(ORCHESTRATOR) || t == "orchestrator")
}

// A wake already owed survives the ending turn; that turn itself cannot bring another. The nudged seat's own turn,
// owed or running, leaves its stall standing.
fn idle(a: &Agent, aside: Aside<'_>) -> bool {
    let own = aside.nudged(&a.key);
    let ends = matches!((&a.status, aside), (AgentStatus::Working { job }, Aside::Ending(j)) if *job == j.id && j.owner == a.key);
    let runs = own && matches!(a.status, AgentStatus::Working { .. });
    (a.status == AgentStatus::Idle || ends || runs)
        && (own || a.owed.is_none() && !a.wake_after)
        && !a.paused
        && a.retiring.is_none()
}

// An unanswered ask on the card, whoever raised it, waits on someone: the wake stays off (fac-379).
fn asked(p: &Poster<'_>, c: &Card) -> bool {
    p.all_asks().any(|a| a.open && a.card.as_ref() == Some(&c.id))
}

// Work that will advance the card keeps the wake off: jobs, queued work, actions, a slot, its review card's round,
// a suite, its place in its merge queue (fac-508), or a verified PR's next approval read, which merges it (fac-392).
fn busy(p: &Poster<'_>, c: &Card, seats: &[Agent], aside: Aside<'_>) -> bool {
    let s = p.state;
    let job = s.jobs.values().any(|j| {
        j.status != JobStatus::Settled
            && !aside.job(j)
            && (seats.iter().any(|a| a.key == j.owner) || j.purpose.as_ref().and_then(|x| x.card()) == Some(&c.id))
    });
    let queued = p.intents.iter().filter_map(|i| {
        let Intent::StartJob { purpose } = i else { return None };
        Some(purpose)
    });
    let owed = s.owed_work.values().chain(queued).any(|w| w.card() == Some(&c.id));
    let action = p.all_actions().any(|a| {
        a.what.card() == Some(&c.id) && !a.cancelled && !matches!(a.status, ActionStatus::Done | ActionStatus::Dropped)
    });
    let lease = crate::queue::leases::live(p).values().any(|l| l.card.as_ref() == Some(&c.id));
    let round = crate::queue::runs::round_open(p, c);
    let gates = c.gates.checking.is_some() || c.gates.slot_wait.is_some() || c.gates.built_pending;
    let approval = matches!(c.landing, Some(Landing::Approval { verified: true, .. }));
    job || owed
        || crate::queue::order::member(c)
        || action
        || lease
        || round
        || gates
        || approval
        || c.join.as_ref().is_some_and(|j| j.waiting)
        || c.fix_import.is_some()
        || c.refresh.as_ref().is_some_and(|r| !r.done)
}
