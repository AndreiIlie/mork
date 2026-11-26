//! The end of every job of no agent's turn (section 4): settled once, then routed to what follows from its purpose.

use mork_types::ask::{Ask, AskKind, Color};
use mork_types::card::{Card, Mode};
use mork_types::state::{Job, Settlement};
use mork_types::work::{CheckRun, CloneStatus, Cloned, FinalRun, Made, Purpose};
use serde_json::{Value, json};

use super::{Ctx, Decision, Intent, JobEnd};
use crate::agents::ends::settle;
use crate::messages::asks::{self, Raise};
use crate::post::Poster;
use crate::state::State;

/// What an install's words start with when the engine gives its exit code: `(exit 3): <last lines>`.
pub const EXIT_MARK: &str = "(exit ";

// A failed job's one line: `<what> failed: <words>`, or `<what> failed (exit 3): <lines>` for an install's.
fn failed_line(what: &str, words: &str) -> String {
    let first = words.lines().next().unwrap_or("");
    let sep = if first.starts_with(EXIT_MARK) { " " } else { ": " };
    format!("{what} failed{sep}{first}")
}

fn what(purpose: &Purpose) -> String {
    match purpose {
        Purpose::Clone { card, .. } => format!("the clone of {card}"),
        Purpose::Install { card, .. } => format!("the install in {card}'s clone"),
        Purpose::Bundle { card, .. } => format!("the bundle of {card}'s clone"),
        Purpose::Check { card, head, .. } => format!("the checks of {card} on {}", head.short()),
        Purpose::Suite { card, .. }
        | Purpose::JoinTest { card, .. }
        | Purpose::Incorporate { card, .. }
        | Purpose::Final { card, .. } => {
            format!("an older build's run of {card}")
        }
        Purpose::Candidate { card, on, .. } => format!("the merge train's candidate of {card} on {}", on.short()),
        Purpose::TrainRun { card, commit, .. } => {
            format!("the merge train's run of {card}'s candidate {}", commit.short())
        }
        Purpose::Build { commit } => format!("the build of {}", commit.short()),
        Purpose::WallTests { card, .. } => format!("the wall tests in {card}"),
    }
}

/// A job of no agent's turn ended: settled once, with what follows.
pub(crate) fn job_ended(state: &State, job: &Job, end: &JobEnd, ctx: Ctx<'_>) -> Decision {
    let Some(purpose) = job.purpose.clone() else { return Decision::default() };
    if let Purpose::WallTests { lease, .. } = &purpose {
        return crate::queue::wall_runs::ended(state, job, lease.as_str(), end, ctx);
    }
    let mut p = Poster::for_ctx(state, ctx);
    p.ending = Some(job.id.clone());
    let end = if job.stop_requested { &JobEnd::Stopped } else { end };
    let how = if let Purpose::Bundle { card, refresh: Some(tip), .. } = &purpose {
        crate::queue::refresh::ended(&mut p, card, tip, end)
    } else if let Some(how) = crate::lifecycle::retired::ended(&mut p, &job.id, &purpose, end) {
        how
    } else {
        routed(&mut p, &job.id, &purpose, end, ctx)
    };
    crate::lifecycle::retired::settled(&mut p, &purpose, &job.id);
    if let Purpose::Bundle { card, base_in: Some(_), .. } = &purpose {
        crate::queue::joins::import_ended(&mut p, card, how == Settlement::Stopped);
        crate::queue::refresh::next(&mut p, card);
    }
    if let Purpose::Clone { card, .. } | Purpose::Install { card, .. } = &purpose
        && p.card(card).is_some_and(|c| c.stage.is_live())
    {
        crate::queue::refresh::cloned(&mut p, card, &how);
    }
    crate::queue::leases::job_done(&mut p, Some(&job.id), &purpose, ctx);
    p.journal("job.settled", purpose.card().map(|c| c.as_str()), json!({"job": job.id.as_str(), "how": how}));
    let mut d = Decision { changes: vec![settle(job, how, ctx.now)], ..Decision::default() };
    p.finish(&mut d);
    d
}

/// Cancels unrecorded work, preserving a stopped job's retry and releasing its lease.
pub(crate) fn stop_owed(p: &mut Poster<'_>, purpose: &Purpose, ctx: Ctx<'_>) {
    if let Purpose::WallTests { lease, .. } = purpose {
        return crate::queue::wall_runs::stop(p, lease.as_str(), ctx);
    }
    let Some(mut c) = purpose.card().and_then(|c| p.card(c)) else { return };
    // The caller finishes stopping or ending the card before it keeps the train and its waiting imports.
    let refresh = matches!(purpose, Purpose::Bundle { refresh: Some(_), .. });
    let train = crate::queue::order::is_train(Some(purpose));
    if !refresh && !keeps_held_run(&c, purpose) && (!train || crate::queue::candidates::current(&c, purpose)) {
        c.failed_job = Some(purpose.clone());
        p.put_card(c);
    }
    crate::queue::leases::job_done(p, None, purpose, ctx);
}

fn settlement(end: &JobEnd) -> Settlement {
    match end {
        JobEnd::Finished { reply: Ok(_), .. } => Settlement::Applied,
        JobEnd::Stopped => Settlement::Stopped,
        JobEnd::Cut { .. } => Settlement::Cut,
        JobEnd::Failed { words } | JobEnd::Finished { reply: Err(words), .. } => {
            Settlement::Failed { words: words.clone() }
        }
    }
}

fn routed(p: &mut Poster<'_>, job: &mork_types::JobId, purpose: &Purpose, end: &JobEnd, ctx: Ctx<'_>) -> Settlement {
    let card = purpose.card().and_then(|c| p.card(c));
    // Non-bundle work settling after a card ended must never revive it, retry, or raise asks (fac-664).
    if let Some(c) = card.as_ref().filter(|c| !c.stage.is_live() && !matches!(purpose, Purpose::Bundle { .. })) {
        if crate::queue::order::is_train(Some(purpose)) {
            let _ = crate::queue::candidates::stale(p, c, purpose);
            crate::queue::train::maintain(p, c.project.as_str(), ctx);
        }
        return settlement(end);
    }
    match (end, card) {
        (JobEnd::Finished { reply: Ok(v), .. }, Some(c)) => {
            finished(p, c, purpose, job, v, ctx);
            Settlement::Applied
        }
        (JobEnd::Finished { reply: Ok(_), .. }, None) => Settlement::Applied,
        (JobEnd::Stopped, c) => {
            match c {
                // The train's job stopped is its candidate's given up, or its card's stopped: its
                // `resume` makes the candidate again (fac-68).
                Some(mut c) if crate::queue::order::is_train(Some(purpose)) => {
                    if !crate::queue::candidates::stale(p, &c, purpose) {
                        c.failed_job = Some(purpose.clone());
                        p.put_card(c.clone());
                    }
                    crate::queue::train::maintain(p, c.project.as_str(), ctx);
                }
                Some(mut c) if !keeps_held_run(&c, purpose) => {
                    c.failed_job = Some(purpose.clone());
                    p.put_card(c);
                }
                Some(_) | None => {}
            }
            Settlement::Stopped
        }
        (JobEnd::Cut { .. }, c) => {
            cut(p, c, purpose, ctx);
            Settlement::Cut
        }
        (JobEnd::Failed { words } | JobEnd::Finished { reply: Err(words), .. }, c) => {
            failed(p, c, purpose, words);
            Settlement::Failed { words: words.clone() }
        }
    }
}

fn finished(p: &mut Poster<'_>, mut card: Card, purpose: &Purpose, job: &mork_types::JobId, v: &Value, ctx: Ctx<'_>) {
    card.job_cuts = 0;
    if !keeps_held_run(&card, purpose) {
        card.failed_job = None;
    }
    p.put_card(card.clone());
    if let Some(hit) = v["secret_hit"].as_str()
        && matches!(purpose, Purpose::Check { .. } | Purpose::TrainRun { checks: true, .. })
    {
        return crate::lifecycle::gates::secret_found(p, card, purpose, hit, ctx);
    }
    match purpose {
        Purpose::Clone { at, .. } if card.mode == Mode::Review => {
            let resolved = serde_json::from_value(v["resolved"].clone()).unwrap_or_default();
            let threads = serde_json::from_value(v["threads"].clone()).unwrap_or_default();
            let rebased = serde_json::from_value(v["rebased"].clone()).ok();
            let cloned = Cloned { at: at.clone(), resolved, threads, rebased };
            crate::reviews::gate::ready(p, card, &cloned, false, ctx);
        }
        Purpose::Clone { .. } => crate::lifecycle::start::cloned(p, card, v, ctx),
        Purpose::Install { cloned: Some(c), .. } => {
            let resolved = serde_json::from_value(v["resolved"].clone()).unwrap_or_else(|_| c.resolved.clone());
            crate::reviews::gate::ready(p, card, &Cloned { resolved, ..(**c).clone() }, true, ctx);
        }
        Purpose::Install { cloned: None, .. } => crate::lifecycle::start::made(p, card, ctx),
        Purpose::Bundle { base_in, .. } => match serde_json::from_value::<CloneStatus>(v.clone()) {
            Ok(s) => {
                let (id, card) = (card.id.clone(), bundled(p, card, base_in.as_ref()));
                crate::lifecycle::gates::bundle_ended(p, card, job, s, ctx);
                crate::queue::joins::bundle_ran(p, &id, base_in.as_ref());
            }
            Err(e) => failed(p, Some(card), purpose, &format!("its status.json isn't valid: {e}")),
        },
        Purpose::Check { fork, .. } => match serde_json::from_value::<CheckRun>(v.clone()) {
            Ok(run) => crate::lifecycle::gates::check_ended(p, card, run, fork, ctx),
            Err(e) => failed(p, Some(card), purpose, &format!("its result isn't valid: {e}")),
        },
        // An older build's suite, join or run of the queue in the card's clone: the train runs them now.
        Purpose::Suite { .. } | Purpose::JoinTest { .. } | Purpose::Incorporate { .. } | Purpose::Final { .. } => {}
        Purpose::Candidate { .. } => match serde_json::from_value::<Made>(v.clone()) {
            Ok(m) => crate::queue::candidates::made(p, card, purpose, m, ctx),
            Err(e) => failed(p, Some(card), purpose, &format!("its result isn't valid: {e}")),
        },
        Purpose::TrainRun { .. } => match serde_json::from_value::<FinalRun>(v.clone()) {
            Ok(run) => crate::queue::runs::ran(p, card, purpose, run, ctx),
            Err(e) => failed(p, Some(card), purpose, &format!("its result isn't valid: {e}")),
        },
        // A build settles as `BuildEnded` (1.8), never here; wall tests settle above.
        Purpose::Build { .. } | Purpose::WallTests { .. } => {}
    }
}

// A cut job runs again once, on its own, unless its card is paused; a second cut in a row
// raises a `cut` ask (1.4.6.5). A cut check runs again at once (4.7.6).
fn cut(p: &mut Poster<'_>, card: Option<Card>, purpose: &Purpose, ctx: Ctx<'_>) {
    let Some(mut c) = card else {
        crate::queue::leases::start_job(p, purpose.clone(), ctx);
        return;
    };
    // The merge train's interrupted run holds it for `/resume` (fac-10, decision 14).
    if crate::queue::order::is_train(Some(purpose)) {
        return crate::queue::members::job_failed(p, c, purpose, "", true);
    }
    if c.paused {
        if !keeps_held_run(&c, purpose) {
            c.failed_job = Some(purpose.clone());
        }
        p.put_card(c);
        return;
    }
    let check = matches!(purpose, Purpose::Check { .. });
    if c.job_cuts >= 1 && !check {
        if !keeps_held_run(&c, purpose) {
            c.failed_job = Some(purpose.clone());
        }
        let id = c.id.clone();
        p.put_card(c);
        asks::raise(
            p,
            Raise::new(
                id.as_str(),
                AskKind::Cut,
                Color::Red,
                format!("{} was cut twice in a row; /resume runs it again", what(purpose)),
            ),
        );
        return;
    }
    c.job_cuts += 1;
    p.put_card(c);
    crate::queue::leases::start_job(p, purpose.clone(), ctx);
}

// Checks that ended with no result can't be trusted (4.7.5); `resume` runs a failed job again (1.4.10.3).
fn failed(p: &mut Poster<'_>, card: Option<Card>, purpose: &Purpose, words: &str) {
    let Some(mut c) = card else {
        crate::maintenance::updates::start_failed(p, purpose, words);
        return;
    };
    let id = c.id.clone();
    if let Some(again) = recovery(&c, purpose, words) {
        p.journal("bundle.recover", Some(id.as_str()), json!({"words": words.lines().next().unwrap_or("")}));
        // A stopped card's runs again on `resume`, as a cut job's does; a closed or landed one's can't wait.
        if c.paused && !crate::lifecycle::gates::ended(&c) {
            if !keeps_held_run(&c, &again) {
                c.failed_job = Some(again);
            }
            p.put_card(c);
        } else if let Purpose::Bundle { base_in: Some(_), recover, .. } = &again {
            crate::queue::joins::start(p, &id, true, recover.clone());
        } else {
            p.intent(Intent::StartJob { purpose: again });
        }
        return;
    }
    // A closed or landed card's last bundle that couldn't run (no clone yet): the clone goes anyway.
    if crate::lifecycle::gates::ended(&c) && matches!(purpose, Purpose::Bundle { .. }) {
        p.act(mork_types::action::ActionWhat::RemoveClone { card: id, run: c.run, keep_proofs: false });
        return;
    }
    if crate::queue::order::is_train(Some(purpose)) {
        return crate::queue::members::job_failed(p, c, purpose, words, false);
    }
    if let Purpose::Check { head, .. } = purpose {
        c.gates.checking = None;
        c.gates.checks_waiting = Some(head.clone());
        p.put_card(c);
        let text = format!(
            "the checks on {} can't be trusted: {}; /resume runs them again",
            head.short(),
            words.lines().next().unwrap_or("")
        );
        asks::raise(p, Raise::new(id.as_str(), AskKind::Checks, Color::Red, text));
        return;
    }
    if !keeps_held_run(&c, purpose) {
        c.failed_job = Some(purpose.clone());
    }
    p.put_card(c);
    let text = failed_line(&what(purpose), words);
    asks::raise(p, Raise::new(id.as_str(), AskKind::Error, Color::Red, text));
    if let Purpose::Bundle { base_in: Some(_), .. } = purpose {
        crate::queue::joins::import_failed(p, &id, words);
    }
}

// The merge queue's held run stays the card's failed job until `/resume` runs it: another job of
// the card (a diagnostic turn's bundle) ending, failing, cut or stopped doesn't answer it (fac-10).
fn keeps_held_run(c: &Card, purpose: &Purpose) -> bool {
    crate::queue::order::is_train(c.failed_job.as_ref()) && !crate::queue::order::is_train(Some(purpose))
}

// Git's words for an object the repository doesn't hold (`fatal: bad object <sha>`).
const BAD_OBJECT: &str = "bad object";

// A bundle that failed on an object its clone lacks runs again once, the card's fork point
// brought in first; the run again carries it, so its own failure is the red ask (fac-12).
fn recovery(c: &Card, purpose: &Purpose, words: &str) -> Option<Purpose> {
    let Purpose::Bundle { card, base_in, recover: None, refresh } = purpose else { return None };
    let fork = c.fork.clone().or_else(|| c.base_tip.clone()).filter(|_| words.contains(BAD_OBJECT))?;
    Some(Purpose::Bundle {
        card: card.clone(),
        base_in: base_in.clone(),
        recover: Some(fork),
        refresh: refresh.clone(),
    })
}

// A bundle ran: the tip it brought in is the clone's base now (4.12.1), and the red ask of the
// card's bundle that failed before is answered (fac-12).
fn bundled(p: &mut Poster<'_>, mut card: Card, base_in: Option<&mork_types::Sha>) -> Card {
    if let Some(tip) = base_in {
        card.base_tip = Some(tip.clone());
        p.put_card(card.clone());
    }
    let id = card.id.clone();
    let prefix =
        format!("{} failed:", what(&Purpose::Bundle { card: id.clone(), base_in: None, recover: None, refresh: None }));
    let open: Vec<Ask> = p
        .all_asks()
        .filter(|a| a.open && a.kind == AskKind::Error && a.agent.is_none() && a.place == id.as_str())
        .filter(|a| a.text.starts_with(&prefix))
        .cloned()
        .collect();
    for a in open {
        asks::close_one(p, a.id.as_str());
    }
    card
}
