//! Stopping, resuming, closing and ending a card (4.13).

use mork_types::action::{ActionStatus, ActionWhat, BoardEdit};
use mork_types::ask::AskKind;
use mork_types::card::{Card, Mode, Stage};
use mork_types::state::{AgentStatus, MessageKind, Sender};
use mork_types::work::Purpose;
use mork_types::{AgentKey, CardId};
use serde_json::json;

use crate::decide::{Ctx, Decision, Intent};
use crate::messages::asks;
use crate::post::Poster;
use crate::state::State;

// A seat ending with its closing turn is no longer its card's: no stop, resume or supersede reaches it.
fn agents_of(p: &Poster<'_>, id: &CardId) -> Vec<AgentKey> {
    let of = |k: &AgentKey| k.as_str().split('/').next() == Some(id.as_str());
    p.state.agents.keys().filter(|k| of(k) && p.agent(k).is_some_and(|a| a.retiring.is_none())).cloned().collect()
}

/// Stops a card in a decision: every job ends, its queued turns and pending actions are
/// cancelled, and it is paused (4.13.1).
pub fn stop_in(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    let id = c.id.clone();
    c.paused = true;
    p.put_card(c);
    for k in &agents_of(p, &id) {
        if let Some(mut a) = p.agent(k) {
            a.paused = true;
            p.put_agent(a);
        }
    }
    stop_jobs(p, &id, ctx);
    let pending: Vec<_> =
        p.all_actions().filter(|a| a.status == ActionStatus::Pending && a.what.card() == Some(&id)).cloned().collect();
    for mut a in pending {
        a.cancelled = true;
        p.put_action(a);
    }
    p.journal("card.stop", Some(id.as_str()), json!({}));
}

/// Marks a running job stopped at most once in a decision.
pub(crate) fn stop_job(p: &mut Poster<'_>, mut j: mork_types::state::Job) {
    if j.stop_requested || p.intents.iter().any(|i| matches!(i, Intent::StopJob { id } if *id == j.id)) {
        return;
    }
    j.stop_requested = true;
    p.intent(Intent::StopJob { id: j.id.clone() });
    p.other.push(crate::state::Change::PutJob(Box::new(j)));
}

/// Stops a card's running jobs, its agents' and its own: whatever they answer is not applied.
pub fn stop_jobs(p: &mut Poster<'_>, id: &CardId, ctx: Ctx<'_>) {
    stop_owed(p, id, ctx);
    let agents = agents_of(p, id);
    let jobs: Vec<_> = p
        .state
        .jobs
        .values()
        .filter(|j| j.settled.is_none() && !j.stop_requested && Some(&j.id) != p.ending.as_ref())
        .filter(|j| agents.contains(&j.owner) || j.purpose.as_ref().and_then(|x| x.card()) == Some(id))
        .cloned()
        .collect();
    for j in jobs {
        stop_job(p, j);
    }
}

pub(super) fn stop_owed(p: &mut Poster<'_>, id: &CardId, ctx: Ctx<'_>) {
    let owed: Vec<_> = p
        .state
        .owed_work
        .iter()
        .filter(|(key, w)| {
            w.card() == Some(id)
                && !p.other.iter().any(|c| matches!(c, crate::state::Change::RemoveOwedWork { key: k } if k == *key))
        })
        .map(|(_, w)| w.clone())
        .collect();
    for purpose in owed {
        p.other.push(crate::state::Change::RemoveOwedWork { key: purpose.key() });
        crate::decide::jobs::stop_owed(p, &purpose, ctx);
    }
}

/// A review card's run superseded by its next one (fac-98): its jobs stopped and its agents' queued
/// turns dropped, nobody paused as a stop would, and the run ended; the next run's turns are its own.
pub fn superseded(p: &mut Poster<'_>, c: Card, ctx: Ctx<'_>) {
    for k in agents_of(p, &c.id) {
        let Some(mut a) = p.agent(&k) else { continue };
        a.owed = None;
        a.not_before = None;
        a.wake_after = false;
        p.put_agent(a);
    }
    end(p, c, true, ctx);
}

/// A review card's next run starts its kept seats unpaused, as it starts the card: the run before's
/// stop ended with it, and a message lifts no pause its idle seats were left with.
pub fn new_run(p: &mut Poster<'_>, id: &CardId) {
    crate::agents::closing::resume(p, id);
    for k in agents_of(p, id) {
        let Some(mut a) = p.agent(&k).filter(|a| a.paused) else { continue };
        a.paused = false;
        p.put_agent(a);
    }
}

// Only a live card stops, resumes or closes.
fn live(state: &State, id: &str) -> Result<Card, Decision> {
    let c = super::card(state, id)?;
    if !c.stage.is_live() {
        return Err(Decision::refused(format!("{id} isn't live: it is {}", c.stage.as_str())));
    }
    Ok(c)
}

fn stop(state: &State, id: &str, ctx: Ctx<'_>) -> Decision {
    let c = match live(state, id) {
        Ok(c) => c,
        Err(d) => return d,
    };
    let mut p = Poster::for_ctx(state, ctx);
    let project = c.project.clone();
    stop_in(&mut p, c, ctx);
    // Out of its train while it is stopped: what was built on it is made again (fac-68).
    crate::queue::train::maintain(&mut p, project.as_str(), ctx);
    let mut d = Decision::ok(json!({"stop": {"card": id}}));
    p.finish(&mut d);
    d
}

// `resume <card>`: it picks up where the stop left it (4.13.2).
fn resume(state: &State, id: &str, ctx: Ctx<'_>) -> Decision {
    let mut c = match live(state, id) {
        Ok(c) => c,
        Err(d) => return d,
    };
    let mut p = Poster::for_ctx(state, ctx);
    let cid = c.id.clone();
    c.paused = false;
    let waited = c.gates.checks_waiting.take();
    let rerun = c.failed_job.take();
    p.put_card(c.clone());
    for k in agents_of(&p, &cid) {
        let Some(mut a) = p.agent(&k) else { continue };
        crate::agents::seats::resumed(state, &mut a);
        p.put_agent(a);
    }
    let cancelled: Vec<_> = p.all_actions().filter(|a| a.cancelled && a.what.card() == Some(&cid)).cloned().collect();
    for mut a in cancelled {
        a.cancelled = false;
        let aid = a.id.clone();
        p.put_action(a);
        p.intent(Intent::RunAction { id: aid });
    }
    let checks_waiting = waited.is_some();
    // A planner's recovery asks are in the spec room (fac-633).
    let rooms = [id.to_owned(), crate::messages::places::spec_room(&cid)];
    let blocked = blocked_seats(&p, &rooms);
    for room in &rooms {
        asks::close(&mut p, room, &[AskKind::Error, AskKind::Cut, AskKind::Checks, AskKind::Stuck, AskKind::Blocked]);
    }
    wake_blocked(&mut p, &blocked);
    // The merge train's run that held it runs again as its failed job, on the same candidate
    // (fac-10, fac-68); a head's untrusted checks, as the gate's (4.4.5, M5.4).
    let landing = c.stage == Stage::Merging && c.landing.is_some();
    if let Some(purpose) = &rerun {
        crate::queue::members::retry(&mut p, &cid, purpose, ctx);
    } else if checks_waiting && !landing {
        super::gates::start_checks(&mut p, c.clone(), ctx);
    }
    if landing {
        crate::queue::train::maintain(&mut p, c.project.as_str(), ctx);
    }
    // A review clone's failure its suite's end cleared, or its suite's held run took the place of, is made again.
    if !matches!(rerun, Some(Purpose::Clone { .. } | Purpose::Install { .. })) {
        crate::reviews::gate::reclone(&mut p, &cid);
    }
    crate::queue::joins::resumed(&mut p, &cid);
    p.journal("card.resume", Some(id), json!({}));
    let mut d = Decision::ok(json!({"resume": {"card": id}}));
    p.finish(&mut d);
    d
}

// A seat that ended `blocked` stays `Idle`: closing its ask without waking it leaves it waiting silently (fac-590).
fn blocked_seats(p: &Poster<'_>, rooms: &[String]) -> Vec<AgentKey> {
    let mut out = Vec::new();
    for a in p.all_asks() {
        if a.open
            && a.kind == AskKind::Blocked
            && rooms.iter().any(|r| r == a.place.as_str())
            && let Some(key) = &a.agent
            && !out.contains(key)
        {
            out.push(key.clone());
        }
    }
    out
}

fn wake_blocked(p: &mut Poster<'_>, agents: &[AgentKey]) {
    for key in agents {
        let Some(mut a) = p.agent(key).filter(|a| a.retiring.is_none()) else { continue };
        if a.owed.is_some() || matches!(a.status, AgentStatus::Working { .. }) {
            continue;
        }
        let Some(who) = crate::messages::places::who_of(p.state, key) else { continue };
        // No turn runs or is owed, so any leftover `wake_after` would queue a spurious second turn on settlement.
        if a.wake_after {
            a.wake_after = false;
            p.put_agent(a);
        }
        p.post(
            who.place.as_str(),
            Sender::Engine,
            Some(who.role.as_str()),
            MessageKind::Request,
            "Resumed",
            "Resumed after `blocked`. Carry on.",
        );
        p.wake(&who, &Sender::Engine);
    }
}

/// `close <card> <why>` (4.13.6).
pub(super) fn close(state: &State, id: &str, why: &str, ctx: Ctx<'_>) -> Decision {
    let c = match live(state, id) {
        Ok(c) => c,
        Err(d) => return d,
    };
    let mut p = Poster::for_ctx(state, ctx);
    let cid = c.id.clone();
    stop_in(&mut p, c.clone(), ctx);
    let mut c = p.card(&cid).unwrap_or(c);
    if let Some(run) = c.lookback.as_ref().filter(|r| r.trigger != mork_types::lookback::Trigger::Manual) {
        super::lookback::rearm(&mut p, &c, run);
    }
    c.stage = Stage::Closed;
    c.closed = Some(why.to_owned());
    c.released = !c.carries.is_empty() && super::carried::closed_on_board(p.state, &cid);
    crate::queue::candidates::drop_candidate(&mut p, &mut c);
    crate::reviews::triage::withdraw(&mut p, &mut c);
    c.landing = None;
    let forge = ctx.config.forge_label(&c.project);
    let pr = c.pr;
    p.put_card(c);
    crate::queue::joins::closed(&mut p, &cid);
    for a in asks::open_of_card(&p, id) {
        asks::close_one(&mut p, &a.id);
    }
    crate::agents::closing::card_ended(&mut p, &cid, true, ctx.config);
    // The head is taken into the mirror first; the clone goes once it is (4.13.6, R8).
    p.intent(Intent::StartJob {
        purpose: mork_types::work::Purpose::Bundle { card: cid.clone(), base_in: None, recover: None, refresh: None },
    });
    if let Some(n) = pr {
        p.post(
            id,
            Sender::Engine,
            Some("operator"),
            MessageKind::Engine,
            "",
            &format!("PR {n} stays as it is on {forge}."),
        );
    }
    p.journal("card.closed", Some(id), json!({"why": why}));
    if let Some(closed) = p.card(&cid) {
        crate::reviews::end::end(&mut p, &closed, ctx);
    }
    super::overlaps::clear(&mut p, &cid);
    let mut d = Decision::ok(json!({"close": {"card": id}}));
    p.finish(&mut d);
    d
}

/// `done <card>`: ends a report or review card, closing a report on its board (4.4.2).
pub(super) fn done(state: &State, from: &Sender, id: &str, ctx: Ctx<'_>) -> Decision {
    let c = match super::card(state, id) {
        Ok(c) => c,
        Err(d) => return d,
    };
    if !matches!(c.mode, Mode::Report | Mode::Review) || !c.stage.is_live() {
        return Decision::refused(format!(
            "`done` ends a live report or review card; {id} is a {} card at {}",
            crate::lifecycle::facts::mode_word(c.mode),
            c.stage.as_str()
        ));
    }
    if let Some(of) = crate::reviews::gate::subject_of(&c) {
        return Decision::refused(format!("{id} is {of}'s review gate: it ends when {of} lands or is closed"));
    }
    let mut p = Poster::for_ctx(state, ctx);
    if c.mode == Mode::Report
        && let Some(bc) = state.board.get(&c.id).filter(|bc| bc.status != bc.done_status)
    {
        let edit = BoardEdit::Close { done: true, who: from.label(), note: "report done".into() };
        let what = crate::board::writes::change(state, &c.project, &bc.file, edit, Some(c.id.clone()), p.now);
        p.act(what);
    }
    if p.all_asks().any(|a| a.open && a.kind == mork_types::ask::AskKind::Done && a.card.as_ref() == Some(&c.id)) {
        super::lookback::completed(&mut p, &c);
    }
    ended(&mut p, c, ctx);
    let mut d = Decision::ok(json!({"done": {"card": id}}));
    p.finish(&mut d);
    d
}

/// A report or review card ended: done, an entry's run under way given up and what was built on it made again
/// without it (fac-111), its asks closed, its seats ended with it but kept, and its clone removed.
pub fn ended(p: &mut Poster<'_>, c: Card, ctx: Ctx<'_>) {
    let (id, project) = (c.id.clone(), c.project.clone());
    end(p, c, false, ctx);
    crate::agents::closing::card_ended(p, &id, false, ctx.config);
    crate::queue::train::maintain(p, project.as_str(), ctx);
}

// A card its next head may reopen (`reopens`) keeps its review's proofs, clean or not (fac-276).
fn end(p: &mut Poster<'_>, mut c: Card, reopens: bool, ctx: Ctx<'_>) {
    stop_jobs(p, &c.id, ctx);
    crate::queue::candidates::drop_candidate(p, &mut c);
    let keep_proofs = reopens && c.mode == Mode::Review;
    c.landing = None;
    c.stage = Stage::Done;
    let (cid, run) = (c.id.clone(), c.run);
    p.put_card(c);
    for a in asks::open_of_card(p, cid.as_str()) {
        asks::close_one(p, &a.id);
    }
    p.act(ActionWhat::RemoveClone { card: cid.clone(), run, keep_proofs });
    super::overlaps::clear(p, &cid);
}

/// `stop` or `resume`, of a card or of an agent: exactly one of them.
pub(super) fn stop_or_resume(
    state: &State,
    stopping: bool,
    card: Option<&CardId>,
    agent: Option<&AgentKey>,
    ctx: Ctx<'_>,
) -> Decision {
    match (card, agent, stopping) {
        (None, Some(a), true) => crate::agents::seats::stop(state, a, ctx),
        (None, Some(a), false) => crate::agents::seats::resume(state, a, ctx),
        (Some(c), None, true) => stop(state, &c.as_str().to_lowercase(), ctx),
        (Some(c), None, false) => resume(state, &c.as_str().to_lowercase(), ctx),
        (Some(_), Some(_), _) | (None, None, _) => Decision::invalid("give exactly one of `card` and `agent`"),
    }
}
