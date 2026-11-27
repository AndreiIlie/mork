//! The closing turn (fac-287): a seat ending a session long enough to have learned something (fac-176) is asked first
//! for any feedback it didn't file, about its project and about Mork, either of which may be nothing.

use mork_types::action::{ActionStatus, ActionWhat};
use mork_types::config::Config;
use mork_types::note::NoteWhat;
use mork_types::state::{Agent, AgentStatus, Job, Owed, Retiring, Settlement};
use mork_types::work::Purpose;
use mork_types::{AgentKey, CardId};
use serde_json::{Value, json};

use crate::decide::{Ctx, JobEnd, SessionSeen};
use crate::post::Poster;
use crate::state::State;

/// A seat's session ends: owed its closing turn if its session earned one and is warm, it goes (`remove`) or
/// stays once that settles; else it goes or stays now.
pub fn retire(p: &mut Poster<'_>, key: &AgentKey, remove: bool, config: &Config) {
    retire_at(p, key, remove, false, config);
}

/// Retires a seat for a closing turn that runs while its card stays live, as at brief approval (fac-670).
pub fn retire_live(p: &mut Poster<'_>, key: &AgentKey, config: &Config) {
    retire_at(p, key, false, true, config);
}

fn retire_at(p: &mut Poster<'_>, key: &AgentKey, remove: bool, live: bool, config: &Config) {
    let Some(mut a) = p.agent(key) else { return };
    if let Some(r) = a.retiring.as_mut() {
        r.remove |= remove;
        r.live |= live;
        // Its card ended again: the closing turn under way is answered, not discarded.
        r.resumed = false;
        return p.put_agent(a);
    }
    let runs = running(p, key);
    if !super::lesson::owed(&a, runs) || !super::cache::warm(config, &a, runs, p.now) {
        if remove {
            removed(p, &a);
        }
        return;
    }
    a.retiring = Some(Retiring { remove, asked: false, resumed: false, live });
    a.owed = Some(Owed::Lesson);
    a.paused = false;
    a.wake_after = false;
    a.cuts = 0;
    ready(&mut a);
    let card = crate::messages::places::card_of(p.state, &a.place);
    p.journal("closing.owed", card.as_ref().map(CardId::as_str), json!({"agent": key, "remove": remove}));
    p.put_agent(a);
}

/// Whether the seat has a turn under way beyond the one this decision settles.
pub(super) fn running(p: &Poster<'_>, key: &AgentKey) -> bool {
    let ending = |j: &Job| p.ending.as_ref() == Some(&j.id);
    p.state.jobs.values().any(|j| &j.owner == key && j.purpose.is_none() && j.settled.is_none() && !ending(j))
}

/// Every seat of a card ends with it: `remove`, they go; else they stay, as a landed card's do.
pub fn card_ended(p: &mut Poster<'_>, id: &CardId, remove: bool, config: &Config) {
    crate::memory::commit(p, id);
    for k in seats_of(p, id) {
        retire(p, &k, remove, config);
    }
}

/// Drops any owed closing turn whose prompt cache has gone cold before launching (fac-670).
pub fn expire(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let keys: Vec<AgentKey> = p.state.agents.keys().cloned().collect();
    for key in keys {
        let Some(a) = p.agent(&key) else { continue };
        if a.retiring.is_none() || running(p, &key) || super::cache::warm(ctx.config, &a, false, p.now) {
            continue;
        }
        settled(p, a, &Settlement::Stopped);
    }
}

fn seats_of(p: &Poster<'_>, id: &CardId) -> Vec<AgentKey> {
    p.state.agents.keys().filter(|k| k.as_str().split('/').next() == Some(id.as_str())).cloned().collect()
}

/// Whether an owed turn may run: a done or closed card's seats run none but their closing turn, and that
/// once their clone is gone, so it doesn't lose its folder mid-turn (fac-146, fac-147).
pub(super) fn may_run(state: &State, a: &Agent) -> bool {
    match &a.retiring {
        None => !crate::messages::places::ended(state, &a.place),
        // A seat kept by its card is asked once the card has ended, unless exempt (fac-670).
        Some(r) if !r.remove && !r.live && live(state, &a.place) => false,
        Some(_) => !clone_going(state, a.key.as_str().split('/').next().unwrap_or_default()),
    }
}

fn live(state: &State, place: &str) -> bool {
    crate::messages::places::card_of(state, place).and_then(|c| state.cards.get(&c)).is_some_and(|c| c.stage.is_live())
}

/// Whether a closed card's clone is still going: its last bundle, owed or running, the head it
/// takes, then the clone's removal (4.13.6), each of which ends, so nothing waits on it for good.
pub fn clone_going(state: &State, card: &str) -> bool {
    let bundle = |w: &Purpose| matches!(w, Purpose::Bundle { card: c, .. } if c.as_str() == card);
    let running = state.jobs.values().any(|j| j.settled.is_none() && j.purpose.as_ref().is_some_and(bundle));
    let acting = state.actions.values().any(|x| {
        matches!(&x.what, ActionWhat::RemoveClone { card: c, .. } | ActionWhat::Take { card: c, .. } if c.as_str() == card)
            && !x.cancelled
            && matches!(x.status, ActionStatus::Pending | ActionStatus::Started)
    });
    running || acting || state.owed_work.values().any(bundle)
}

/// Whether a card's clone removal is held after a failure (fac-591): a new run waits for its retry, which keeps the
/// clone's mock aside before the clone goes.
pub fn removal_held(state: &State, card: &str) -> bool {
    state.actions.values().any(|x| {
        matches!(&x.what, ActionWhat::RemoveClone { card: c, .. } if c.as_str() == card)
            && !x.cancelled
            && x.status == ActionStatus::Held
    })
}

/// Whether the turn this seat is owed is its closing turn, which the feedback's schema answers; any
/// other owed turn answers its role's.
pub fn ending_turn(a: &Agent) -> bool {
    a.retiring.is_some() && !matches!(a.owed, Some(Owed::Wake | Owed::Retry { .. }))
}

fn resumable(r: &Retiring) -> bool {
    !r.remove && !r.live
}

/// Whether a seat stays for a next round: not retiring, or in a closing turn `resume` keeps.
pub(crate) fn kept(a: &Agent) -> bool {
    a.retiring.as_ref().is_none_or(resumable)
}

/// A card's next round (fac-275): its kept seats are no longer ending; a closing turn not yet asked is dropped,
/// one under way is discarded when it settles; a seat its card released (`remove`) goes on ending.
pub fn resume(p: &mut Poster<'_>, id: &CardId) {
    for key in seats_of(p, id) {
        let Some(mut a) = p.agent(&key) else { continue };
        let Some(r) = a.retiring.as_mut().filter(|r| resumable(r)) else { continue };
        // Only the ending turn's own reply is discarded: a turn it ran before ends as an ordinary one.
        if r.asked && running(p, &key) {
            r.resumed = true;
        } else {
            a.retiring = None;
            // A request the closing turn held is owed now; that turn, or a correction of it, is not.
            a.owed = a.wake_after.then_some(Owed::Wake);
            a.wake_after = false;
            a.cuts = 0;
            if matches!(a.status, AgentStatus::Waiting { .. }) {
                a.status = AgentStatus::Idle;
                a.not_before = None;
            }
        }
        p.journal("closing.resumed", Some(id.as_str()), json!({"agent": key}));
        p.put_agent(a);
    }
}

/// Its closing turn launches: the next turn to settle is that one.
pub(super) fn launched(a: &mut Agent) {
    if let Some(r) = a.retiring.as_mut().filter(|_| matches!(a.owed, Some(Owed::Survey | Owed::Lesson))) {
        r.asked = true;
    }
}

/// Whether the turn settling now is an agent's closing turn.
pub(super) fn asked(a: &Agent) -> bool {
    a.retiring.as_ref().is_some_and(|r| r.asked)
}

// Owed its closing turn now, unless a reached limit has it wait for the reset (1.4.10.2).
fn ready(a: &mut Agent) {
    if !matches!(a.status, AgentStatus::Waiting { .. }) {
        a.status = AgentStatus::Idle;
        a.not_before = None;
    }
}

/// A closing turn settled: asked again after a limit's reset, a first cut or a refused reply; else
/// its feedback filed, and its seat goes or stays.
pub(super) fn ended(p: &mut Poster<'_>, mut a: Agent, job: &Job, end: &JobEnd, ctx: Ctx<'_>) -> Settlement {
    // Its card's round began meanwhile (fac-275): whatever this turn ends as, nothing of it is applied.
    if a.retiring.as_ref().is_some_and(|r| r.resumed) {
        if matches!(end, JobEnd::Stopped) {
            stopped(&mut a, job);
        }
        p.journal("job.settled", None, json!({"job": job.id.as_str(), "how": Settlement::Stopped}));
        settled(p, a, &Settlement::Stopped);
        return Settlement::Stopped;
    }
    if let Some(how) = again(p, &mut a, end, ctx) {
        p.journal("job.settled", None, json!({"job": job.id.as_str(), "how": how}));
        p.put_agent(a);
        return how;
    }
    let how = match end {
        JobEnd::Stopped => {
            stopped(&mut a, job);
            Settlement::Stopped
        }
        JobEnd::Cut { .. } => Settlement::Cut,
        JobEnd::Failed { words } | JobEnd::Finished { reply: Err(words), .. } => {
            Settlement::Failed { words: words.clone() }
        }
        JobEnd::Finished { reply: Ok(reply), .. } => match answered(p, &a, reply, ctx) {
            Ok(()) => {
                a.ran.closed = a.ran.turns;
                Settlement::Applied
            }
            Err(reason) if job.corrects.is_none() => {
                p.journal("reply.refused", None, json!({"job": job.id.as_str(), "reason": reason}));
                a.status = AgentStatus::Idle;
                a.owed = Some(Owed::Correction { of: job.id.clone(), reason });
                p.put_agent(a);
                return Settlement::Applied;
            }
            Err(reason) => {
                p.journal("reply.refused", None, json!({"job": job.id.as_str(), "reason": reason}));
                Settlement::Failed { words: format!("reply refused twice: {reason}") }
            }
        },
    };
    p.journal("job.settled", None, json!({"job": job.id.as_str(), "how": how}));
    settled(p, a, &how);
    how
}

// A non-interrupted or paused stop holds the seat paused in `settled`, keeping any held request for resume (fac-670).
fn stopped(a: &mut Agent, job: &Job) {
    if !job.interrupted || a.paused {
        a.status = AgentStatus::Stopped;
    }
}

// A limit reached waits for its reset, and a first cut runs once more (1.4.6.4): the closing turn
// is owed again, asked anew.
fn again(p: &Poster<'_>, a: &mut Agent, end: &JobEnd, ctx: Ctx<'_>) -> Option<Settlement> {
    let how = match end {
        JobEnd::Failed { words } => waits(p, a, words, &SessionSeen::default(), ctx)?,
        JobEnd::Finished { reply: Err(words), session } => waits(p, a, words, session, ctx)?,
        JobEnd::Cut { .. } if a.cuts == 0 => {
            a.cuts = 1;
            a.status = AgentStatus::Idle;
            Settlement::Cut
        }
        JobEnd::Cut { .. } | JobEnd::Stopped | JobEnd::Finished { reply: Ok(_), .. } => return None,
    };
    a.owed = Some(Owed::Lesson);
    if let Some(r) = a.retiring.as_mut() {
        r.asked = false;
    }
    Some(how)
}

fn waits(p: &Poster<'_>, a: &mut Agent, words: &str, session: &SessionSeen, ctx: Ctx<'_>) -> Option<Settlement> {
    let until = super::usage::limit_reset(p.state, a, session, ctx)?;
    a.status = AgentStatus::Waiting { until };
    a.not_before = Some(until);
    Some(Settlement::Failed { words: words.to_owned() })
}

/// Its closing turn is done, answered or not, and its seat goes or stays.
pub(super) fn settled(p: &mut Poster<'_>, mut a: Agent, how: &Settlement) {
    p.journal("closing.ended", None, json!({"agent": a.key, "how": how}));
    if a.retiring.take().is_some_and(|r| r.remove) {
        removed(p, &a);
    } else {
        // A request its closing turn held is owed now unless its card ended (fac-146); a seat stopped in
        // it, or on a card stopped meanwhile, which no stop reaches while it closes, stays held.
        let card = crate::messages::places::card_of(p.state, &a.place).and_then(|c| p.card(&c));
        let stopped = matches!(a.status, AgentStatus::Stopped);
        a.paused = (stopped && a.paused) || card.as_ref().is_some_and(|c| c.paused);
        a.owed = (a.wake_after && card.is_none_or(|c| c.stage.is_live())).then_some(Owed::Wake);
        // A stopped closing turn is over, never retried on resume (fac-670).
        a.status = AgentStatus::Idle;
        a.last_prompt = None;
        if a.owed.is_none() {
            a.not_before = None;
        }
        a.wake_after = false;
        super::fresh::turn_ended(p, &mut a);
        p.put_agent(a);
    }
}

/// Removes an ended seat.
pub(crate) fn removed(p: &mut Poster<'_>, a: &Agent) {
    p.remove_agent(&a.key);
}

/// A lesson owed while its seat's own turn ran stands once that turn is counted only if it earned
/// it (fac-176); else the seat ends as it would have, gone when this says so.
pub(super) fn unearned(a: &mut Agent) -> bool {
    if a.retiring.is_none() || asked(a) || super::lesson::owed(a, false) {
        return false;
    }
    a.owed = None;
    a.retiring.take().is_some_and(|r| r.remove)
}

/// A turn it ran before its closing turn ended: that is still owed, after a reached limit's reset if
/// it reached one, and a request that turn owed waits for it.
pub(super) fn still_owed(state: &State, a: &mut Agent) {
    if a.retiring.as_ref().is_some_and(|r| !r.asked) {
        a.wake_after |= matches!(a.owed, Some(Owed::Wake));
        if let Some(of) = super::ends::owed_correction(a) {
            a.wake_after |= super::ends::give_back(state, a, &of);
        }
        a.owed = Some(Owed::Lesson);
        ready(a);
    }
}

// Its answers journalled, posted nowhere, and each feedback filed as a note under its project: the seat's own card's,
// or Mork's (fac-593).
fn answered(p: &mut Poster<'_>, a: &Agent, reply: &Value, ctx: Ctx<'_>) -> Result<(), String> {
    let (feedback, mork) = super::lesson::check(reply, ctx.config.mork.project.is_some())?;
    let card = crate::messages::places::card_of(p.state, &a.place);
    let said = json!({"agent": a.key, "feedback": feedback.is_some(), "mork_feedback": mork.is_some()});
    p.journal("closing.answered", card.as_ref().map(CardId::as_str), said);
    let (project, card) = crate::memory::origin(p.state, a);
    if let Some(text) = feedback {
        crate::memory::put(p, project, card.clone(), Some(a.key.clone()), NoteWhat::Feedback { text });
    }
    if let (Some(text), Some(project)) = (mork, ctx.config.mork.project.clone()) {
        crate::memory::put(p, Some(project), card, Some(a.key.clone()), NoteWhat::Feedback { text });
    }
    Ok(())
}
