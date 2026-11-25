//! A turn's end, applied once (2.4): its job settled, its reply checked and applied, a cut retried, a failure raised or
//! waiting on its limit, and the wake a request during it is owed.

use factory_types::ask::{AskKind, Color};
use factory_types::reply::{Outcome, Role};
use factory_types::state::{Agent, AgentStatus, Job, JobStatus, MessageKind, Owed, Sender, Settlement};
use factory_types::{JobId, SessionId};
use serde_json::{Value, json};

use super::turns::SessionSeen;
use crate::decide::{Ctx, Decision, JobEnd};
use crate::post::Poster;
use crate::state::{Change, State};

pub(crate) fn settle(job: &Job, how: Settlement, now: i64) -> Change {
    let mut j = job.clone();
    j.status = JobStatus::Settled;
    j.settled = Some(how);
    j.settled_ms = Some(now);
    Change::PutJob(Box::new(j))
}

/// The job ended: settle it, once, with what follows (2.4.1, 2.4.4). A job of no agent's turn
/// is the pipeline's.
pub fn ended(state: &State, id: &JobId, end: &JobEnd, ctx: Ctx<'_>) -> Decision {
    let Some(job) = state.jobs.get(id) else { return Decision::default() };
    if job.settled.is_some() {
        return Decision::default(); // applied once: never a second time
    }
    if job.purpose.is_some() {
        return crate::decide::jobs::job_ended(state, job, end, ctx);
    }
    let Some(agent) = state.agents.get(&job.owner).filter(|_| !job.detached) else {
        return settle_detached(state, job, end, ctx.now);
    };
    let mut p = Poster::for_ctx(state, ctx);
    p.ending = Some(job.id.clone());
    let mut a = agent.clone();
    let mut woken = matches!(a.owed, Some(Owed::Wake)) || a.wake_after;
    end_session(&mut a, end, job);
    if let Some(c) = super::costs::of_end(&mut a, end, job, ctx.now) {
        let card = crate::messages::places::card_of(state, &a.place);
        p.journal("cost", card.as_ref().map(|c| c.as_str()), c);
    }
    // A recorded stop wins (1.4.6.2).
    let end = if job.stop_requested { &JobEnd::Stopped } else { end };
    if touched_model(&a, end) {
        a.last_turn_ms = Some(ctx.now);
    }
    if let Some(how) = apart(&mut p, &a, job, end, ctx) {
        let mut d = Decision { changes: vec![settle(job, how, ctx.now)], ..Decision::default() };
        p.finish(&mut d);
        return d;
    }
    // Counted before its reply ends anything, so a seat its own turn retires has it (fac-176).
    a.ran.turns = a.ran.turns.saturating_add(1);
    let job = job.clone();
    took(&mut a, &job, end);
    let how = match end {
        JobEnd::Stopped => crate::messages::interrupts::stopped(&mut a, &job),
        JobEnd::Cut { .. } => cut(&mut p, &mut a, &job),
        // Every failed turn: the reset rule first (1.4.10.2, M2.3).
        JobEnd::Failed { words } => failed_or_waits(&mut p, &mut a, &job, words, &SessionSeen::default(), ctx),
        JobEnd::Finished { reply: Err(words), session } => failed_or_waits(&mut p, &mut a, &job, words, session, ctx),
        JobEnd::Finished { reply: Ok(reply), .. } => match apply_reply(&mut p, &mut a, &job, reply, ctx) {
            Ok(dropped) => {
                woken &= !dropped;
                Settlement::Applied
            }
            Err(words) => failed(&mut p, &mut a, &words),
        },
    };
    let gone = super::closing::unearned(&mut a);
    let woken = missed(p.state, &mut a, &job, &how) || woken || a.wake_after;
    let woken = woken && !covered_wake(&p, &mut a);
    wake_after(&mut a, woken);
    super::closing::still_owed(p.state, &mut a);
    super::fresh::turn_ended(&p, &mut a);
    crate::lifecycle::rebrief::ended_stale(p.state, &mut a, &job);
    p.journal("job.settled", None, json!({"job": id.as_str(), "how": how}));
    p.put_agent(a.clone());
    if gone {
        super::closing::removed(&mut p, &a);
    }
    let mut d = Decision { changes: vec![settle_read(&mut p, &a, job, how)], ..Decision::default() };
    p.finish(&mut d);
    d
}

// A detached brief turn's cut still clears its live seat's held sections (fac-629).
fn settle_detached(state: &State, job: &Job, end: &JobEnd, now: i64) -> Decision {
    let how = if job.stop_requested { Settlement::Stopped } else { Settlement::Cut };
    let mut changes = vec![settle(job, how, now)];
    if matches!(end, JobEnd::Cut { .. })
        && let Some(mut a) = state.agents.get(&job.owner).filter(|a| !a.held.is_empty()).cloned()
    {
        super::prompt::held::cut(&mut a);
        changes.push(Change::PutAgent(a));
    }
    Decision { changes, ..Decision::default() }
}

// The closing turn has its own reply (fac-287).
fn apart(p: &mut Poster<'_>, a: &Agent, job: &Job, end: &JobEnd, ctx: Ctx<'_>) -> Option<Settlement> {
    super::closing::asked(a).then(|| super::closing::ended(p, a.clone(), job, end, ctx))
}

// Only a turn that reached the model refreshed the session's prompt cache (fac-670).
fn touched_model(a: &Agent, end: &JobEnd) -> bool {
    if !a.session_started {
        return false;
    }
    match end {
        JobEnd::Finished { reply: Ok(_), .. } | JobEnd::Cut { .. } | JobEnd::Stopped => true,
        JobEnd::Finished { reply: Err(_), session } => session.answered || session.tools,
        JobEnd::Failed { .. } => false,
    }
}

fn covered_wake(p: &Poster<'_>, a: &mut Agent) -> bool {
    super::children::covered_by_verdict(p, a) || crate::lifecycle::brief::planner_just_approved(p, a)
}

// A refused reply's receipts stay provisional until its correction applies: cancelling that correction must
// never leave a message read in the chat after its news got it back (fac-406).
fn settle_read(p: &mut Poster<'_>, a: &Agent, mut job: Job, how: Settlement) -> Change {
    if let Some(of) = &job.corrects
        && owed_correction(a).as_ref() != Some(of)
        && let Some(refused) = p.state.jobs.get(of).filter(|j| j.read.is_some())
    {
        let mut j = refused.clone();
        j.read = Some(j.delivered.iter().filter(|s| a.taken.contains(s)).copied().collect());
        p.other.push(Change::PutJob(Box::new(j)));
    }
    job.read = Some(if owed_correction(a).as_ref() == Some(&job.id) {
        Vec::new()
    } else {
        job.delivered.iter().filter(|s| a.taken.contains(s)).copied().collect()
    });
    settle(&job, how, p.now)
}

// The messages delivered into a turn that its finished reply read (fac-389): known before the reply applies, so
// what it decides counts them as read.
fn took(a: &mut Agent, job: &Job, end: &JobEnd) {
    let JobEnd::Finished { reply: Ok(_), session } = end else { return };
    a.taken.extend(job.delivered.iter().filter(|s| session.taken.contains(s)));
}

// Unless its reply applied, none was; one not read wakes its agent, whose news says the turn may have read it.
fn missed(state: &State, a: &mut Agent, job: &Job, how: &Settlement) -> bool {
    if *how != Settlement::Applied {
        a.taken.retain(|s| !job.delivered.contains(s));
        if let Some(of) = job.corrects.as_ref().filter(|of| owed_correction(a).as_ref() != Some(*of)) {
            give_back(state, a, of);
        }
    }
    let missed: Vec<u64> = job.delivered.iter().filter(|s| !a.taken.contains(s)).copied().collect();
    if missed.is_empty() {
        return false;
    }
    a.missed.extend(&missed);
    // Its wake is no longer only reports, a completion or the build request, which a verdict or a step drops.
    a.report_round = None;
    a.completion_wake = false;
    a.build_wake = false;
    true
}

// A refused reply's turn read what it took, which stays taken only while a correction of it is owed: one that won't
// run gives it back to its agent's news, as a message its session may have read (fac-389).
pub(crate) fn give_back(state: &State, a: &mut Agent, of: &JobId) -> bool {
    let Some(refused) = state.jobs.get(of) else { return false };
    let back: Vec<u64> = refused.delivered.iter().filter(|s| a.taken.contains(s)).copied().collect();
    a.taken.retain(|s| !back.contains(s));
    a.missed.extend(&back);
    !back.is_empty()
}

pub(crate) fn owed_correction(a: &Agent) -> Option<JobId> {
    let Some(Owed::Correction { of, .. } | Owed::Retry { corrects: Some(of), .. }) = &a.owed else { return None };
    Some(of.clone())
}

fn failed_or_waits(
    p: &mut Poster<'_>,
    a: &mut Agent,
    job: &Job,
    words: &str,
    session: &SessionSeen,
    ctx: Ctx<'_>,
) -> Settlement {
    match super::usage::limit_reset(p.state, a, session, ctx) {
        Some(until) => {
            wait_for_limit(p.state, a, job, until);
            Settlement::Failed { words: words.to_owned() }
        }
        None => failed(p, a, words),
    }
}

// Its news is given back so a message retries it (1.4.10.1, 4.13.3).
fn failed(p: &mut Poster<'_>, a: &mut Agent, words: &str) -> Settlement {
    a.status = AgentStatus::Error { words: words.to_owned() };
    a.seen = a.seen.min(a.seen_before);
    if let Some(card) = crate::messages::places::card_of(p.state, &a.place) {
        let text = if super::fresh::session_missing(words) {
            super::fresh::missing(a)
        } else {
            format!("{} is in error: {}", a.key, words.lines().next().unwrap_or(""))
        };
        let room = crate::messages::places::room_of(&card, a.role);
        let mut r = crate::messages::asks::Raise::new(&room, AskKind::Error, Color::Red, text);
        r.agent = Some(a.key.clone());
        crate::messages::asks::raise(p, r);
    }
    Settlement::Failed { words: words.to_owned() }
}

fn end_session(a: &mut Agent, end: &JobEnd, job: &Job) {
    match end {
        JobEnd::Finished { session, .. } => note_session(a, session, job),
        JobEnd::Cut { session } => {
            note_session(a, session, job);
            super::prompt::held::cut(a);
        }
        JobEnd::Failed { .. } | JobEnd::Stopped => {}
    }
}

pub(super) fn note_session(a: &mut Agent, s: &SessionSeen, job: &Job) {
    a.session_started |= s.started;
    a.ran.tools |= s.tools;
    if let Some(t) = &s.thread {
        a.session = Some(SessionId::new(t.clone()));
    }
    if s.started {
        super::fresh::started(a);
    }
    super::prompt::held::seen(a, job, s);
}

// A request during the turn is never lost: it wakes the agent now, or after the correction, retry or closing turn it owes.
// A turn in error (a correction failed, or refused again) leaves it owed as a wake: no further correction (M2.4).
fn wake_after(a: &mut Agent, woken: bool) {
    match (&a.owed, &a.status) {
        (Some(Owed::Wake), _) => a.wake_after = false,
        (Some(Owed::Survey | Owed::Lesson | Owed::Retry { .. } | Owed::Correction { .. }), _) => a.wake_after = woken,
        (None, AgentStatus::Idle | AgentStatus::Error { .. }) if woken => {
            // As a request arriving now would (`Poster::wake`): the error is answered by it.
            a.status = AgentStatus::Idle;
            a.cuts = 0;
            a.owed = Some(Owed::Wake);
            a.wake_after = false;
        }
        (None, _) => a.wake_after = false,
    }
}

fn wait_for_limit(state: &State, a: &mut Agent, job: &Job, until: i64) {
    a.status = AgentStatus::Waiting { until };
    a.not_before = Some(until);
    a.owed = Some(retry_of(state, a, job, false));
}

// A retry keeps its turn's head and news (M4.2) and what it corrected, and runs its request again: an opening one's
// first try's, named, its cut note kept once any try was cut (fac-626); a correction its own prompt.
fn retry_of(state: &State, a: &Agent, job: &Job, cut: bool) -> Owed {
    let request = job.corrects.is_none().then(|| super::turns::request_of(state, job));
    let opening = request.filter(|r| r.opening);
    let cut = cut || (opening.is_some() && job.stdin.starts_with(super::turns::CUT_NOTE));
    let prompt = &request.unwrap_or(job).stdin;
    Owed::Retry {
        prompt: if cut { super::turns::cut_noted(prompt) } else { prompt.clone() },
        resume: a.session_started,
        corrects: job.corrects.clone(),
        head: job.head.clone(),
        seen: job.seen,
        request: opening.map(|r| r.id.clone()),
    }
}

// A cut turn is retried once, on its own, unless paused; a second cut in a row raises a
// `cut` ask and runs nothing more (1.4.6.4).
fn cut(p: &mut Poster<'_>, a: &mut Agent, job: &Job) -> Settlement {
    a.cuts += 1;
    if a.cuts >= 2 {
        a.status = AgentStatus::Cut;
        a.owed = None;
        let place = crate::messages::places::card_of(p.state, &a.place)
            .map_or(a.place.to_string(), |c| crate::messages::places::room_of(&c, a.role));
        let mut r = crate::messages::asks::Raise::new(
            &place,
            AskKind::Cut,
            Color::Red,
            format!("{}'s turn was cut twice in a row; nothing more runs until it is resumed.", a.key),
        );
        r.agent = Some(a.key.clone());
        crate::messages::asks::raise(p, r);
    } else {
        a.status = AgentStatus::Idle;
        a.owed = Some(retry_of(p.state, a, job, true));
    }
    Settlement::Cut
}

// Only malformed replies, wrong-role outcomes, incomplete findings, untyped messages, invalid images or ineffective
// foreman outcomes leaving a card waiting are refused: once a correction, twice an error (1.4.9, fac-423).
fn apply_reply(p: &mut Poster<'_>, a: &mut Agent, job: &Job, reply: &Value, ctx: Ctx<'_>) -> Result<bool, String> {
    let state = p.state;
    let who =
        crate::messages::places::Who { key: a.key.clone(), seat: a.seat.clone(), role: a.role, place: a.place.clone() };
    let checked = if a.role == Role::Echo {
        super::reply::check_echo(reply).map(Accepted::Echo)
    } else {
        super::reply::check(a.role, reply, job).and_then(|r| {
            crate::messages::images::reply_allowed(job, &r)?;
            let routes = crate::messages::replies::routes(state, &who, &r, ctx);
            crate::lifecycle::replies::check_outcome(p, &who, &r, job, a, &routes)?;
            Ok(Accepted::Reply(Box::new(r), routes))
        })
    };
    match checked {
        Err(reason) if job.corrects.is_none() => {
            a.status = AgentStatus::Idle;
            a.owed = Some(Owed::Correction { of: job.id.clone(), reason: reason.clone() });
            p.journal("reply.refused", None, json!({"job": job.id.as_str(), "reason": reason}));
            Ok(false)
        }
        Err(reason) => {
            p.journal("reply.refused", None, json!({"job": job.id.as_str(), "reason": reason}));
            Err(format!("reply refused twice: {reason}"))
        }
        Ok(accepted) => {
            a.status = AgentStatus::Idle;
            a.cuts = 0;
            let mut dropped = false;
            match accepted {
                Accepted::Echo(text) => {
                    p.post(&a.place, Sender::Agent(a.key.clone()), None, MessageKind::Update, "", &text);
                }
                Accepted::Reply(r, routes) => {
                    p.put_agent(a.clone());
                    let gate = crate::lifecycle::replies::raises_gate(p, &who, &r);
                    let round = super::children::job_round(p, &who, job);
                    crate::messages::replies::apply(p, &who, &r, &routes, (gate, round), ctx);
                    // Its request to itself woke the poster's copy, which `a` replaces (M2.7), as did its cause (fac-277).
                    let woken = p.agent(&a.key);
                    a.wake_after |= woken.as_ref().is_some_and(|x| matches!(x.owed, Some(Owed::Wake)));
                    a.report_round = woken.as_ref().and_then(|x| x.report_round);
                    a.completion_wake = woken.as_ref().is_some_and(|x| x.completion_wake);
                    a.build_wake = woken.is_some_and(|x| x.build_wake);
                    p.put_agent(a.clone());
                    crate::lifecycle::replies::carried_on(p, &who, &r);
                    crate::lifecycle::replies::outcome(p, &who, &r, job, ctx);
                    if let Some(after) = p.agent(&a.key) {
                        *a = after;
                    }
                    a.replied = Some(r.outcome);
                    dropped = r.outcome == Outcome::ReportReady && super::children::report_submitted(p, a);
                    dropped |= r.outcome == Outcome::Built && super::children::build_answered(p, a);
                }
            }
            Ok(dropped)
        }
    }
}

enum Accepted {
    Echo(String),
    Reply(Box<factory_types::reply::Reply>, Vec<crate::messages::places::Route>),
}
