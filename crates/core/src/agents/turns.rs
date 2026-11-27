//! A turn's life in the state up to its end (1.4): which agents are due, its plan, its intent, its claim, and the
//! session its stream shows.

use std::collections::BTreeMap;

use mork_types::config::Config;
use mork_types::reply::Role;
use mork_types::state::{Agent, AgentStatus, Job, JobStatus, Owed};
use mork_types::{AgentKey, JobId, SessionId, card::Mode, card::Stage};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::state::{Change, State};

/// What a stream showed of its session.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionSeen {
    /// Claude's `system/init`, or Codex's `thread.started`, was seen.
    pub started: bool,
    /// Codex's thread id, when read.
    pub thread: Option<String>,
    /// Claude's stream showed a reached limit (`rejected`).
    pub limit_reached: bool,
    /// That limit's reset, in epoch seconds, when the stream gave one.
    pub limit_reset: Option<i64>,
    /// The session's cost total the stream last gave, in millionths of a dollar (Claude 7).
    pub cost_micros: Option<u64>,
    /// The CLI, model and effort the job launched with (fac-526).
    pub selection: Option<mork_types::cost::Selection>,
    /// Running usage by session or Codex thread, including child threads (fac-526).
    pub token_totals: std::collections::BTreeMap<String, mork_types::cost::Tokens>,
    /// The context window it reported, with its model (Claude 9).
    pub window: Option<(String, u64)>,
    /// The tokens its context held at the latest call it reported (fac-25).
    pub context: Option<u64>,
    /// The turn made a tool call (fac-176).
    pub tools: bool,
    /// The messages delivered into it that its model went on to read (fac-389), by seq.
    pub taken: Vec<u64>,
    /// Its model answered the turn's prompt (fac-620): what the prompt sent is in its session.
    pub answered: bool,
    /// Its session was compacted during the turn (fac-620): it holds none of what it was sent before.
    pub compacted: bool,
}

/// The note a cut turn's retry starts with (1.4.6.4).
pub(super) const CUT_NOTE: &str = "Your previous turn was cut off before it finished. Before redoing anything outward, check whether it already happened.";

/// A cut or stopped turn's prompt run again: the cut note once, before the turn's own prompt.
pub(super) fn cut_noted(prompt: &str) -> String {
    let original = prompt.strip_prefix(CUT_NOTE).map_or(prompt, str::trim_start);
    format!("{CUT_NOTE}\n\n{original}")
}

/// The agents owed a turn that may launch now: owed, unpaused, past any reset, no job unsettled, of a
/// live card (fac-146) whose turns the merge queue doesn't hold (fac-10), past its other turn holds.
pub fn due(state: &State, config: &Config, now: i64) -> Vec<AgentKey> {
    let busy = std::cell::OnceCell::new();
    state
        .agents
        .values()
        .filter(|a| ready_for_turn(a, now))
        .filter(|a| !state.jobs.values().any(|j| j.owner == a.key && j.settled.is_none()))
        .filter(|a| !awaiting_clone(state, a))
        .filter(|a| super::closing::may_run(state, a))
        .filter(|a| !super::closing::ending_turn(a) || super::cache::warm(config, a, false, now))
        // Nobody edits a clone the merge queue is incorporating or testing (fac-10, decision 5).
        .filter(|a| a.retiring.is_some() || !crate::queue::holds::held(state, a))
        // The build waits for a reopened mock's approve; running turns finish (fac-591, decision 1).
        .filter(|a| a.retiring.is_some() || !crate::lifecycle::mock::holds(state, a))
        // A visual card's mock folder has one writer: the baseline seat first, then the planner (fac-636).
        .filter(|a| !crate::lifecycle::baseline::waits(state, a))
        .filter(|a| {
            let auto = crate::memory::seat_card(state, &a.key)
                .and_then(|c| c.lookback.as_ref())
                .is_some_and(|run| run.trigger == mork_types::lookback::Trigger::Auto);
            !auto || !*busy.get_or_init(|| real_running(state) || state.agents.values().any(|a| real_owed(state, a)))
        })
        .map(|a| a.key.clone())
        .collect()
}

pub(crate) fn real_running(state: &State) -> bool {
    state
        .jobs
        .values()
        .filter(|j| j.purpose.is_none() && j.settled.is_none())
        .filter_map(|j| j.owner.as_str().split_once('/'))
        .any(|(card, _)| state.cards.get(card).is_some_and(|c| c.lookback.is_none()))
}

pub(crate) fn real_owed(state: &State, a: &Agent) -> bool {
    a.owed.is_some()
        && crate::memory::seat_card(state, &a.key).is_some_and(|c| c.lookback.is_none())
        && (super::closing::ending_turn(a) || (!a.paused && super::closing::may_run(state, a)))
}

/// Whether the seat itself is ready to launch an owed turn.
pub(super) fn ready_for_turn(a: &Agent, now: i64) -> bool {
    a.owed.is_some()
        && !a.paused
        && a.not_before.is_none_or(|t| t <= now)
        && !matches!(a.status, AgentStatus::Working { .. } | AgentStatus::Cut)
}

// Messages at Starting wait for the same clone preparation as the first brief request: a full card's planner's and
// room foreman's alike (fac-616, fac-633).
fn awaiting_clone(state: &State, a: &Agent) -> bool {
    let holds = a.role == Role::Planner || (a.role == Role::Foreman && a.parent.is_none());
    holds
        && crate::messages::places::card_of(state, &a.place)
            .and_then(|id| state.cards.get(&id))
            .is_some_and(|c| c.stage == Stage::Starting && (c.mode == Mode::Full || c.no_planner))
}

/// What a due agent's next turn is: its prompt, and whether it resumes its session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnPlan {
    pub agent: AgentKey,
    pub prompt: String,
    /// The prefix shared by the full and short prompts, kept apart from the request on replay.
    pub specification_bytes: usize,
    pub resume: bool,
    pub corrects: Option<JobId>,
    /// The images of the news it reads (1.4.8, Claude 4).
    pub images: Vec<String>,
    /// A retry: the job whose images it carries into its own `io/` under the same names (M2.8).
    pub carry: Option<JobId>,
    /// A retry of its session's opening request (fac-626): the job that first ran it, whose prompt it runs again and
    /// whose `io/` that prompt's text names.
    pub request: Option<JobId>,
    /// The session it runs in; none, a new one.
    pub session: Option<SessionId>,
    /// Such a retry in the session it began, with the sections it stages: sent instead when that session holds
    /// `request`'s prompt word for word, as read at launch (fac-626).
    pub short: Option<(String, BTreeMap<String, String>)>,
    /// The sections its prompt sends, by name: its session holds them once its model answered it (fac-620).
    pub staged: BTreeMap<String, String>,
}

// The prompt and the recorded launch must agree on the session. A closing turn uses its original one.
fn launch_session(state: &State, a: &mut Agent, ctx: Ctx<'_>) -> Option<super::models::Switch> {
    if a.retiring.is_some() {
        return None;
    }
    let switch = super::models::launch(state, ctx.config, a, ctx.now);
    super::fresh::at_boundary(state, a, ctx);
    switch
}

/// The plan of a due agent's turn. A `codex_exec` launch, an unwalled `codex exec`, cannot track compaction.
pub fn plan(state: &State, key: &AgentKey, ctx: Ctx<'_>, codex_exec: bool) -> Option<TurnPlan> {
    let mut a = state.agents.get(key)?.clone();
    launch_session(state, &mut a, ctx);
    let a = &a;
    // An opening request's retry is read against its own reply contract, whatever ran since (fac-626).
    let request = opening_retry(state, a);
    let ((mut prompt, staged), resume, corrects) = asked(state, a, ctx, codex_exec, request)?;
    let (images, carry) = match a.owed.as_ref()? {
        Owed::Wake => (super::prompt::news_images(state, a), None),
        Owed::Retry { prompt, .. } => (Vec::new(), carried(state, key, prompt, request)),
        Owed::Correction { .. } | Owed::Survey | Owed::Lesson => (Vec::new(), None),
    };
    // A session resumes only once it has started (1.4.6.4).
    let resume = resume && a.session_started;
    let mut short = request.filter(|_| resume).map(|r| short(state, a, ctx, (r, codex_exec)));
    let specification = super::prompt::after_compaction(state, a);
    prompt.insert_str(0, &specification);
    if let Some((text, _)) = &mut short {
        text.insert_str(0, &specification);
    }
    Some(TurnPlan {
        agent: key.clone(),
        specification_bytes: specification.len(),
        prompt,
        resume,
        corrects,
        images,
        carry,
        request: request.map(|r| r.id.clone()),
        session: a.session.clone(),
        short,
        staged,
    })
}

type Prompted = ((String, BTreeMap<String, String>), bool, Option<JobId>);

// The owed turn's prompt and the sections it stages, whether it resumes, and what it corrects.
fn asked(state: &State, a: &Agent, ctx: Ctx<'_>, codex_exec: bool, request: Option<&Job>) -> Option<Prompted> {
    use super::prompt::{Asked::Correction, Asked::Retry, rerun};
    Some(match a.owed.as_ref()? {
        Owed::Wake => {
            let (prompt, staged) = super::prompt::turn(state, a, ctx, codex_exec);
            ((crate::messages::interrupts::noted(state, a, prompt), staged), super::prompt::resumes(a), None)
        }
        Owed::Retry { prompt, resume, corrects, .. } => {
            let asked = Retry(request.map(|r| &r.id));
            (rerun(state, a, ctx.config, (asked, *resume, codex_exec), prompt), *resume, corrects.clone())
        }
        Owed::Correction { of, reason } if a.retiring.is_some() => {
            let with_mork = ctx.config.mork.project.is_some();
            ((super::lesson::refused(reason, with_mork), BTreeMap::new()), true, Some(of.clone()))
        }
        Owed::Survey | Owed::Lesson => {
            ((super::lesson::prompt(ctx.config.mork.project.is_some()), BTreeMap::new()), true, None)
        }
        Owed::Correction { of, reason } => {
            let refused = format!("Your reply was refused: {reason}. Reply again, fixed.");
            (rerun(state, a, ctx.config, (Correction(of), true, codex_exec), &refused), true, Some(of.clone()))
        }
    })
}

// A retry of its session's opening request (fac-626).
fn opening_retry<'s>(state: &'s State, a: &Agent) -> Option<&'s Job> {
    let Some(Owed::Retry { request, .. }) = &a.owed else { return None };
    request.as_ref().and_then(|r| state.jobs.get(r)).filter(|j| j.opening && !j.stdin.is_empty())
}

// Only an opening request, in the session it began, may run short (fac-626).
fn short(state: &State, a: &Agent, ctx: Ctx<'_>, request: (&Job, bool)) -> (String, BTreeMap<String, String>) {
    let cut = matches!(&a.owed, Some(Owed::Retry { prompt, .. }) if prompt.starts_with(CUT_NOTE));
    super::prompt::continuation(state, a, ctx.config, request, cut)
}

/// The job whose prompt carries the request `job` ran (fac-626): an opening request's first try, else its own.
pub(super) fn request_of<'s>(state: &'s State, job: &'s Job) -> &'s Job {
    job.continues.as_ref().and_then(|r| state.jobs.get(r)).unwrap_or(job)
}

// An opening request's retry takes its images from the latest of its tries holding them (fac-626); any other
// retry, from the try its text was taken from.
fn carried(state: &State, key: &AgentKey, prompt: &str, request: Option<&Job>) -> Option<JobId> {
    let Some(first) = request else { return retried(state, key, prompt) };
    let tries = state.jobs.values().filter(|j| j.id == first.id || j.continues.as_ref() == Some(&first.id));
    tries.filter(|j| &j.owner == key && !j.io_copies.is_empty()).map(|j| j.id.clone()).max()
}

fn retried(state: &State, key: &AgentKey, prompt: &str) -> Option<JobId> {
    state
        .jobs
        .values()
        .filter(|j| {
            &j.owner == key
                && !j.io_copies.is_empty()
                && !j.request_text().is_empty()
                && prompt.ends_with(j.request_text())
        })
        .map(|j| j.id.clone())
        .max()
}

/// The job of an owed turn is recorded, `launching` (1.4.3.1).
pub fn prepared(state: &State, job: &Job, session: Option<&str>, ctx: Ctx<'_>) -> Decision {
    let Some(stored) = state.agents.get(&job.owner) else { return Decision::default() };
    if stored.owed.is_none() {
        return Decision::default();
    }
    // As its plan had it: the pick launches, a switch's fresh session in place (fac-96).
    let mut switched = stored.clone();
    let switch = launch_session(state, &mut switched, ctx);
    let agent = &switched;
    let mut a = agent.clone();
    if matches!(a.owed, Some(Owed::Wake)) {
        a.seen_before = a.seen;
        a.seen = state.next_seq;
        // No news it reads from here on is older (fac-389).
        a.taken.retain(|s| *s >= a.seen_before);
        a.missed.retain(|s| *s >= a.seen_before);
        // Its news reaches past any reports held for it: they wake it no more (fac-528).
        a.held_reports = None;
    }
    super::closing::launched(&mut a);
    super::prompt::held::launched(&mut a, job);
    // A retry of an opening request keeps that request's prompt to run again, a cut's note with it (fac-626).
    if job.corrects.is_none() && !super::closing::ending_turn(agent) {
        let request = request_of(state, job);
        let noted = request.id != job.id && job.request_text().starts_with(CUT_NOTE);
        a.last_prompt = Some(if noted { cut_noted(request.request_text()) } else { request.request_text().to_owned() });
    }
    a.owed = None;
    a.report_round = a.report_round.filter(|_| a.wake_after);
    a.completion_wake &= a.wake_after;
    a.build_wake &= a.wake_after;
    a.not_before = None;
    a.status = AgentStatus::Working { job: job.id.clone() };
    if a.session.is_none() {
        a.session = session.map(SessionId::new);
    }
    // Its first turn names its `io/` folder, kept for the seat's lifetime (fac-654).
    a.io.get_or_insert_with(|| job.id.as_str().to_owned());
    let mut job = job.clone();
    job.head = asked_head(state, agent, &a);
    job.seen = news_reached(state, agent, &a);
    let mut d = Decision::default();
    d.journal("job.intent", None, json!({"job": job.id.as_str(), "agent": a.key.as_str()}));
    d.changes.push(Change::PutJob(Box::new(job)));
    let key = a.key.clone();
    d.changes.push(Change::PutAgent(a));
    if let Some(s) = switch {
        super::models::switch_line(state, &key, &s, ctx, &mut d);
    }
    let mut p = crate::post::Poster::for_ctx(state, ctx);
    crate::lifecycle::mock::starting(&mut p, agent);
    p.finish(&mut d);
    d
}

// A correction or a retry is asked about its turn's head, never a newer one (M4.2).
fn asked_head(state: &State, owed: &Agent, a: &Agent) -> Option<mork_types::Sha> {
    match owed.owed.as_ref()? {
        Owed::Wake => review_head(state, a),
        Owed::Retry { head, .. } => head.clone(),
        Owed::Correction { of, .. } => state.jobs.get(of).and_then(|j| j.head.clone()),
        Owed::Survey | Owed::Lesson => None,
    }
}

// A fresh turn is given the news up to the agent's moved cursor; a retry or a correction runs a turn
// again, its prompt and its news that turn's (fac-51).
fn news_reached(state: &State, owed: &Agent, a: &Agent) -> Option<u64> {
    match owed.owed.as_ref()? {
        Owed::Wake => Some(a.seen),
        Owed::Retry { seen, .. } => *seen,
        Owed::Correction { of, .. } => state.jobs.get(of).and_then(|j| j.seen),
        Owed::Survey | Owed::Lesson => None,
    }
}

fn review_head(state: &State, a: &Agent) -> Option<mork_types::Sha> {
    let card = crate::messages::places::card_of(state, &a.place).and_then(|c| state.cards.get(&c))?;
    match a.role {
        Role::Foreman | Role::Reviewer if card.mode == Mode::Review => card.gates.review_asked.clone(),
        Role::Planner | Role::Foreman | Role::Reviewer | Role::Researcher | Role::Baseline | Role::Echo => None,
    }
}

/// The supervisor claimed it.
pub fn running(state: &State, id: &JobId) -> Decision {
    let Some(job) = state.jobs.get(id) else { return Decision::default() };
    if job.status != JobStatus::Launching {
        return Decision::default();
    }
    let mut j = job.clone();
    j.status = JobStatus::Running;
    let mut d = Decision { changes: vec![Change::PutJob(Box::new(j))], ..Decision::default() };
    d.journal("job.running", None, json!({"job": id.as_str()}));
    d
}

/// A turn's session, as its stream showed it, kept on its agent whatever the turn's end.
pub fn session_seen(state: &State, id: &JobId, session: &SessionSeen, now: i64) -> Decision {
    let Some(job) = state.jobs.get(id) else { return Decision::default() };
    let Some(mut a) = state.agents.get(&job.owner).cloned() else { return Decision::default() };
    let before = a.clone();
    let cost = if job.detached {
        // A detached turn's session can still compact its live seat before exiting (fac-629).
        if session.compacted {
            super::prompt::held::compacted(&mut a);
        }
        None
    } else {
        super::ends::note_session(&mut a, session, job);
        super::costs::note(&mut a, session, job, now)
    };
    if a == before {
        return Decision::default();
    }
    let card = crate::messages::places::card_of(state, &a.place);
    let mut d = Decision { changes: vec![Change::PutAgent(a)], ..Decision::default() };
    d.journal("session.seen", None, json!({"job": id.as_str()}));
    if let Some(c) = cost {
        d.journal("cost", card.as_ref().map(|c| c.as_str()), c);
    }
    d
}

/// A job of no agent's turn is recorded, once.
pub fn recorded(state: &State, job: &Job) -> Decision {
    if state.jobs.contains_key(&job.id) {
        return Decision::default();
    }
    let mut d = Decision::default();
    d.journal("job.intent", None, json!({"job": job.id.as_str(), "kind": job.kind.as_str()}));
    d.changes.push(Change::PutJob(Box::new(job.clone())));
    // Recorded: no longer owed (M3.9).
    if let Some(key) =
        job.purpose.as_ref().map(mork_types::work::Purpose::key).filter(|k| state.owed_work.contains_key(k))
    {
        d.changes.push(Change::RemoveOwedWork { key });
    }
    crate::lifecycle::gates::bundle_recorded(state, job, &mut d);
    d
}
