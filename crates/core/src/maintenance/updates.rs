//! Mork's own updates (fac-19): a new commit on its master owes a build, which the engine pins when in place; a
//! failed build or refused pin waits for the operator's retry.

use mork_types::job::JobKind;
use mork_types::state::{Job, MessageKind, Sender, Settlement};
use mork_types::view::Update;
use mork_types::work::Purpose;
use mork_types::{Command, JobId, Sha};
use serde_json::json;

use crate::decide::{Ctx, Decision, Intent};
use crate::messages::places::MAIN;
use crate::post::Poster;
use crate::state::{Change, State};

/// Master's tip against `current`, the pin or else the running build: a tip that isn't it, with no build owed or made,
/// owes one. A build already in place, tried for the pin since (`pinned`), is recorded as a build's end records it.
pub(crate) fn seen(
    state: &State,
    tip: &Sha,
    current: &str,
    pinned: Option<&Result<(), String>>,
    ctx: Ctx<'_>,
) -> Decision {
    let mut d = Decision::default();
    if let Some(pinned) = pinned {
        tried(state, &mut d, tip.as_str(), pinned, ctx.now);
        return d;
    }
    if tip.as_str() == current || built(state, tip.as_str()) {
        return d;
    }
    let mut p = Poster::for_ctx(state, ctx);
    p.journal("update.seen", None, json!({"tip": tip, "current": current}));
    p.intent(Intent::StartJob { purpose: Purpose::Build { commit: tip.clone() } });
    p.finish(&mut d);
    d
}

/// Whether a build of `commit` is owed or has a job, whatever its end: one that failed is tried
/// again only by the operator's retry.
pub fn built(state: &State, commit: &str) -> bool {
    let owed = Purpose::Build { commit: Sha::new(commit) }.key();
    state.owed_work.contains_key(&owed) || state.jobs.values().any(|j| of(j, commit))
}

/// Whether the last build of `commit` put it in place, with none owed or under way since.
pub fn in_place(state: &State, commit: &str) -> bool {
    let owed = state.owed_work.contains_key(&Purpose::Build { commit: Sha::new(commit) }.key());
    !owed && last_job(state, commit).is_some_and(|j| j.settled == Some(Settlement::Applied))
}

// A build's job, or the job that says its build couldn't start (`work_refused`).
fn of(j: &Job, commit: &str) -> bool {
    let owed = matches!(&j.purpose, Some(Purpose::Build { commit: c }) if c.as_str() == commit);
    owed || (j.kind == JobKind::Build && j.spec.env.get("MORK_BUILD_COMMIT").is_some_and(|c| c == commit))
}

fn last_job<'a>(state: &'a State, commit: &str) -> Option<&'a Job> {
    state.jobs.values().filter(|j| of(j, commit)).max_by_key(|j| (j.spec.created_ms, j.id.clone()))
}

/// What the top line says of master's tip when it isn't `current`: its build under way, or
/// failed, or in place (`in_place`) and not pinned, with the command that tries again.
pub fn line(state: &State, tip: &str, current: &str, in_place: bool) -> Option<Update> {
    if tip == current {
        return None;
    }
    let short: String = tip.chars().take(7).collect();
    let owed = state.owed_work.contains_key(&Purpose::Build { commit: Sha::new(tip) }.key());
    let last = last_job(state, tip);
    if owed || last.is_some_and(|j| j.settled.is_none()) {
        return Some(Update::Preparing { short });
    }
    let last = last?;
    if in_place && last.settled == Some(Settlement::Applied) {
        return Some(Update::NotPinned { short, retry: Command::Pin { commit: tip.to_owned() } });
    }
    Some(Update::BuildFailed { short, retry: Command::Build { commit: tip.to_owned() } })
}

/// A build owed for an update couldn't start: said in main, as a failed one is.
pub(crate) fn start_failed(p: &mut Poster<'_>, purpose: &Purpose, words: &str) {
    let Purpose::Build { commit } = purpose else { return };
    let text = format!("The build of {} couldn't start: {words}{}", commit.short(), retry_build(commit.short()));
    p.post(MAIN, Sender::Engine, Some("operator"), MessageKind::Engine, "", &text);
}

// How a failed build of an update is tried again: the words after its failure in main.
fn retry_build(short: &str) -> String {
    format!(" The pin stays where it was; retry from the top line, or with `mork ctl build {short}`.")
}

// A build in place whose pin the checks refused: said in main.
fn pin_refused(short: &str, why: &str) -> String {
    format!(
        "The build of {short} is in place, but it wasn't pinned: {why}. The pin stays where it was; retry from the top line, or with `mork ctl pin {short}`."
    )
}

/// A build ended: settled; a failure is said in main (1.4.10.3).
pub(crate) fn build_ended(
    state: &State,
    id: &JobId,
    commit: &str,
    result: &Result<(), String>,
    pinned: &Option<Result<(), String>>,
    ctx: Ctx<'_>,
) -> Decision {
    let Some(job) = state.jobs.get(id) else { return Decision::default() };
    if job.settled.is_some() {
        return Decision::default();
    }
    let mut d = Decision::default();
    match result {
        Ok(()) => {
            d.changes.push(crate::agents::ends::settle(job, Settlement::Applied, ctx.now));
            d.journal("build", None, json!({"commit": commit, "ok": true}));
            if let Some(pinned) = pinned {
                tried(state, &mut d, commit, pinned, ctx.now);
            }
        }
        Err(words) => {
            d.changes.push(crate::agents::ends::settle(job, Settlement::Failed { words: words.clone() }, ctx.now));
            d.journal("build", None, json!({"commit": commit, "ok": false, "words": words}));
            let short: String = commit.chars().take(7).collect();
            // An update's build, owed by a landing or by master's new tip (fac-19), says how it is tried again.
            let retry = if job.purpose.is_some() { retry_build(&short) } else { String::new() };
            d.changes.push(said(state, &format!("The build of {short} failed: {words}{retry}"), ctx.now));
        }
    }
    d
}

// The pin tried for a build in place, journaled: `pin_tried` reads it, so a refusal waits for its
// retry. A refusal is said in main.
fn tried(state: &State, d: &mut Decision, commit: &str, pinned: &Result<(), String>, now: i64) {
    match pinned {
        Ok(()) => d.journal("update.pinned", None, json!({"commit": commit})),
        Err(why) => {
            d.journal("update.refused", None, json!({"commit": commit, "why": why}));
            let short: String = commit.chars().take(7).collect();
            d.changes.push(said(state, &pin_refused(&short, why), now));
        }
    }
}

fn said(state: &State, text: &str, now: i64) -> Change {
    Change::AddMessage(crate::messages::message(
        state,
        MAIN,
        Sender::Engine,
        Some("operator"),
        MessageKind::Engine,
        text,
        now,
    ))
}
