//! A door call carried out (fac-1). After a crash a `started` call is reconciled by a receipt or
//! the dispatcher's word; anything else is held with its outcome unknown, never re-run blind.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use mork_types::action::Action;
use mork_types::door::{CALLER_WENT, NEVER_BLIND, UNSTOPPED};
use mork_types::door_plan::{DispatchAnswer, DispatchRequest, Envelope, Plan, Session, WIRE};
use mork_types::state::Sender;
use mork_types::work::{ActionFailure, Found};
use serde_json::Value;

use super::redact::Redactor;
use crate::jobs::{bounded::Captured, door_transport};
use crate::paths::Home;

/// A call's folder: its `dispatched` mark, its receipt, and a read's server session.
pub fn dir(home: &Home, action: &str) -> PathBuf {
    home.door().join("calls").join(action)
}

/// The dispatcher's receipts.
pub fn receipts(home: &Home) -> PathBuf {
    home.door().join("receipts")
}

/// Where the dispatcher makes each call's own folder: its forward's socket, its client's options.
pub fn tunnels(home: &Home) -> PathBuf {
    home.door().join("tunnels")
}

fn guard(words: impl Into<String>) -> ActionFailure {
    ActionFailure::Guard { words: words.into(), pr: None }
}

// It may have run: held, never run again blind.
fn unknown(words: impl std::fmt::Display) -> ActionFailure {
    ActionFailure::Gone { words: format!("{NEVER_BLIND}: {words}") }
}

// It may still run on its server, on the session recorded for it: the read fails, and the same
// query waits for it to end (fac-184). With no session known, it is cut like any other.
fn still_running(words: impl std::fmt::Display, session: Option<Session>) -> ActionFailure {
    match session {
        Some(session) => ActionFailure::Unstopped { words: format!("{NEVER_BLIND}: {words}"), session },
        None => unknown(words),
    }
}

// The session its dispatcher answered with, or as kept in its call's folder.
fn session_of(d: &Path, answer: Option<&DispatchAnswer>) -> Option<Session> {
    answer
        .and_then(|a| a.session.or_else(|| a.ran.as_ref().and_then(|r| r.session)))
        .or_else(|| std::fs::read_to_string(d.join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()))
}

fn durable(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    if let Some(d) = path.parent() {
        std::fs::File::open(d)?.sync_all()?;
    }
    Ok(())
}

fn of(a: &Action) -> Result<(Envelope, String), ActionFailure> {
    let (call, _) = mork_core::door::parts(a).ok_or_else(|| guard("not a door call"))?;
    let e: Envelope = serde_json::from_str(&call.plan).map_err(|e| guard(format!("its plan can't be read: {e}")))?;
    Ok((e, call.call_id(a.id.as_str())))
}

// An orchestrator's copy stays outside every wall; a turn's stays in its own `$MORK_IO`.
// Call ids keep copies apart without letting a caller choose a destination.
fn into(home: &Home, a: &Action, e: &Envelope) -> Result<Option<String>, ActionFailure> {
    let Plan::Get { path, .. } = &e.plan else { return Ok(None) };
    let (call, _) = mork_core::door::parts(a).ok_or_else(|| guard("not a door call"))?;
    let dir = match &call.job {
        Some(job) => turn_io(home, job.as_str())?,
        None if matches!(call.from, Sender::Orchestrator(_)) => {
            let dir = home.door().join("files");
            match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
                Err(err) if err.kind() != std::io::ErrorKind::AlreadyExists => {
                    return Err(guard(format!("its copies folder can't be made: {err}")));
                }
                Ok(()) | Err(_) => {}
            }
            let meta = std::fs::symlink_metadata(&dir)
                .map_err(|err| guard(format!("its copies folder can't be read: {err}")))?;
            if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
                return Err(guard(format!("{} isn't a private folder", dir.display())));
            }
            dir
        }
        None => return Err(guard("file.get copies into a turn's io/ folder: ask it from a turn")),
    };
    let name = path.rsplit('/').next().unwrap_or(path);
    Ok(Some(dir.join(format!("{}-{name}", a.id)).display().to_string()))
}

// The asking turn's `MORK_IO`, as its spec names it (fac-654): its seat's folder or its job's `io/`, nothing else.
fn turn_io(home: &Home, job: &str) -> Result<PathBuf, ActionFailure> {
    let spec = crate::jobs::folder::JobDir::new(home.job(job)).spec();
    let spec = spec.map_err(|e| guard(format!("its turn's spec can't be read: {e}")))?;
    let io = spec.and_then(|s| s.env.get("MORK_IO").map(PathBuf::from));
    let io = io.ok_or_else(|| guard("its turn names no io/ folder"))?;
    let seat = io
        .strip_prefix(home.seat_ios())
        .is_ok_and(|r| matches!(r.components().next(), Some(Component::Normal(_))) && r.components().count() == 1);
    if seat || io == home.job(job).join("io") {
        return Ok(io);
    }
    Err(guard(format!("its turn's io/ folder {} isn't one of the home's", io.display())))
}

// A receipt's question may reach the read's server to kill its session: never less than a minute.
fn send(
    home: &Home,
    bin: &Path,
    e: &Envelope,
    req: &DispatchRequest,
    stop: Option<&AtomicBool>,
) -> std::io::Result<Captured> {
    let line = serde_json::to_string(req).map_err(std::io::Error::other)?;
    let argv = door_transport::local_argv(bin, e.route.allow(), &receipts(home), &tunnels(home));
    let timeout_ms = if req.receipt { e.bounds.timeout_ms.max(60_000) } else { e.bounds.timeout_ms };
    door_transport::send(home.root(), &argv, &line, (e.bounds.max_bytes, timeout_ms), stop)
}

// A read with no answer may still run on its server: its dispatcher kills the session it recorded
// (fac-183), and answers whether it was killed or already gone; none, when none was left to kill.
fn kill(home: &Home, bin: &Path, e: &Envelope, req: &DispatchRequest) -> Option<DispatchAnswer> {
    let ask = DispatchRequest { receipt: true, redact: Vec::new(), into: None, ..req.clone() };
    send(home, bin, e, &ask, None).ok().as_ref().and_then(answer_of)
}

// A read's server session as its dispatcher recorded it, kept in its call's folder: its
// connection's id and time limit, read by the engine once the dispatcher's own record is gone.
fn keep_session(d: &Path, answer: Option<&DispatchAnswer>) {
    let Some(a) = answer else { return };
    if let Some(s) = a.session.or_else(|| a.ran.as_ref().and_then(|r| r.session)) {
        let _ = durable(&d.join("session.json"), &serde_json::to_string(&s).unwrap_or_default());
    }
}

fn killed(stopped: Option<bool>) -> &'static str {
    match stopped {
        Some(true) => "its query was killed on its server",
        Some(false) => "its server didn't take the kill of its query: it may still run there",
        None => "no query of it was left to kill on its server",
    }
}

fn answer_of(c: &Captured) -> Option<DispatchAnswer> {
    let text = String::from_utf8_lossy(&c.stdout);
    serde_json::from_str(text.lines().next().unwrap_or("")).ok()
}

/// Runs a door action: its result, or why not. A read is cut once `stop` is set: its caller went.
pub fn run(home: &Home, bin: &Path, a: &Action, r: &Redactor, stop: &AtomicBool) -> Result<Value, ActionFailure> {
    let (e, call) = of(a)?;
    let stop = matches!(e.plan, Plan::Sql { .. }).then_some(stop);
    if stop.is_some_and(|s| s.load(Ordering::SeqCst)) {
        return Err(guard(format!("{CALLER_WENT} before it ran")));
    }
    // Again under the target's lock, right before the dispatch: the start's check is a moment old.
    super::unchanged(home, a).map_err(guard)?;
    // Read now, and only by the engine: missing or ambiguous, nothing runs.
    let (credential, secrets) = super::credentials(home, &e.plan).map_err(guard)?;
    let mut r = r.clone();
    for v in &secrets {
        r.add(v);
    }
    let r = &r;
    let known = known_store(home);
    if !receipts(home).is_dir() {
        // A store lost is the operator's to look into: made again unseen, its loss would go unnoticed.
        if known.is_some() {
            return Err(guard("its receipts folder is gone and isn't made again: make it by hand to go on"));
        }
        std::fs::create_dir_all(receipts(home)).map_err(|err| guard(format!("its receipts can't be kept: {err}")))?;
    }
    let into = into(home, a, &e)?;
    let d = dir(home, a.id.as_str());
    std::fs::create_dir_all(&d).map_err(|err| guard(format!("its folder can't be made: {err}")))?;
    match durable(&d.join("dispatched"), known.as_deref().unwrap_or("")) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => return Err(unknown("it was dispatched before")),
        Err(err) => return Err(guard(format!("its mark can't be written: {err}"))),
    }
    let req = DispatchRequest {
        v: WIRE,
        call,
        route: Some(e.route.clone()),
        plan: Some(e.plan.clone()),
        credential,
        bounds: Some(e.bounds),
        receipt: false,
        redact: r.values().to_vec(),
        store: None,
        into,
    };
    let captured = send(home, bin, &e, &req, stop);
    let answered = captured.as_ref().ok().and_then(answer_of);
    if let Some(DispatchAnswer { store: Some(s), .. }) = &answered {
        keep_store(home, s);
    }
    keep_session(&d, answered.as_ref());
    let read = matches!(e.plan, Plan::Sql { .. });
    let result = match answered {
        None if read && captured.is_ok() => unanswered(home, bin, (&e, &req, &d), captured, stop, r),
        _ => interpret(&e, captured, &d, r),
    }?;
    let _ = durable(&d.join("receipt.json"), &result.to_string());
    Ok(result)
}

// A read left with no answer may still run on its server, and is killed there. One whose caller
// went, killed, failed as stopped; else its outcome is unknown, with what the kill did.
fn unanswered(
    home: &Home,
    bin: &Path,
    (e, req, d): (&Envelope, &DispatchRequest, &Path),
    captured: std::io::Result<Captured>,
    stop: Option<&AtomicBool>,
    r: &Redactor,
) -> Result<Value, ActionFailure> {
    let answer = kill(home, bin, e, req);
    keep_session(d, answer.as_ref());
    let session = session_of(d, answer.as_ref());
    let stopped = answer.and_then(|a| a.stopped);
    if stop.is_some_and(|s| s.load(Ordering::SeqCst)) && stopped != Some(false) {
        let words = format!("{CALLER_WENT}, and {}", killed(stopped));
        let ran = mork_types::door_plan::Ran { exit: Some(137), stderr: words, ..Default::default() };
        let mut v = super::output::shape(&e.plan, &ran, &e.bounds, r);
        v["caller_went"] = serde_json::json!(true);
        return Ok(v);
    }
    let unkilled = session.filter(|_| stopped == Some(false));
    interpret(e, captured, d, r).map_err(|f| match (f, unkilled) {
        (ActionFailure::Gone { words }, Some(session)) => {
            ActionFailure::Unstopped { words: format!("{words}; {}", killed(stopped)), session }
        }
        (ActionFailure::Gone { words }, None) => ActionFailure::Gone { words: format!("{words}; {}", killed(stopped)) },
        (other, _) => other,
    })
}

// The receipts store the calls went to, as last seen; a call's mark names it.
fn store_file(home: &Home) -> PathBuf {
    home.door().join("stores").join("local")
}

fn known_store(home: &Home) -> Option<String> {
    let id = std::fs::read_to_string(store_file(home)).ok()?;
    Some(id.trim().to_owned()).filter(|t| !t.is_empty())
}

fn keep_store(home: &Home, id: &str) {
    if known_store(home).as_deref() == Some(id) {
        return;
    }
    let path = store_file(home);
    let tmp = path.with_file_name(format!(".new-{}", std::process::id()));
    if path.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::write(&tmp, id).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

const LOST: &str = "its ssh connection ended with no conclusive result: it may have run there";
const CUT_AT_BOUND: &str = "its query was cut at its deadline or cap";

// A query cut while it ran, still running on its server, is no plain result: the operator is asked.
fn unstopped(ran: &mork_types::door_plan::Ran) -> bool {
    (ran.timed_out || ran.truncated) && ran.stopped == Some(false)
}

// `Guard` when nothing ran (the dispatcher's word, or no transport); else its outcome is unknown.
fn interpret(
    e: &Envelope,
    captured: std::io::Result<Captured>,
    d: &Path,
    r: &Redactor,
) -> Result<Value, ActionFailure> {
    let c = captured.map_err(|err| guard(format!("its transport didn't start: {err}")))?;
    if c.timed_out {
        return Err(unknown("it passed its deadline"));
    }
    match answer_of(&c) {
        Some(DispatchAnswer { refused: Some(why), .. }) => {
            Err(guard(format!("the target ran nothing: {}", r.mask_text(&why, false))))
        }
        Some(DispatchAnswer { ran: Some(ran), .. }) if ran.lost => Err(unknown(LOST)),
        Some(a @ DispatchAnswer { ran: Some(_), .. }) if a.ran.as_ref().is_some_and(unstopped) => {
            Err(still_running(UNSTOPPED, session_of(d, Some(&a))))
        }
        Some(DispatchAnswer { ran: Some(ran), .. }) => Ok(super::output::shape(&e.plan, &ran, &e.bounds, r)),
        _ => {
            let err = r.mask_text(&String::from_utf8_lossy(&c.stderr), true);
            Err(unknown(format!("no answer from its dispatcher (exit {:?}): {}", c.exit, err.trim())))
        }
    }
}

/// What a crash left of a `started` call: its receipt; the dispatcher's receipt; the
/// dispatcher's word that it never got it, or no `dispatched` mark at all; else unclear.
pub fn found(home: &Home, bin: &Path, a: &Action, r: &Redactor) -> Found {
    let d = dir(home, a.id.as_str());
    if let Some(v) = std::fs::read_to_string(d.join("receipt.json")).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        return Found::Done { result: v };
    }
    if !d.join("dispatched").exists() {
        return Found::NotDone;
    }
    let unclear = |why: &str| Found::Unclear { words: format!("it was dispatched, and {why}: {NEVER_BLIND}") };
    let Ok((e, call)) = of(a) else { return unclear("its plan can't be read") };
    let req = receipt_request(home, &d, &e, call);
    let sql = req.credential.is_some();
    let answer = send(home, bin, &e, &req, None).ok().as_ref().and_then(answer_of);
    if sql {
        keep_session(&d, answer.as_ref());
    }
    let stopped = answer.as_ref().map(|a| a.stopped).filter(|_| sql);
    // Its server didn't take the kill: it may still run there, on the session recorded for it.
    let session = session_of(&d, answer.as_ref()).filter(|_| stopped == Some(Some(false)));
    let held = |why: &str| match (stopped, session) {
        (Some(stopped), Some(session)) => Found::Unstopped {
            words: format!("it was dispatched, and {why}, and {}: {NEVER_BLIND}", killed(stopped)),
            session,
        },
        (Some(stopped), None) => unclear(&format!("{why}, and {}", killed(stopped))),
        (None, _) => unclear(why),
    };
    match answer {
        Some(DispatchAnswer { ran: Some(ran), .. }) if ran.lost => held(LOST),
        Some(DispatchAnswer { ran: Some(ran), .. }) if unstopped(&ran) => held(CUT_AT_BOUND),
        Some(DispatchAnswer { ran: Some(ran), .. }) => {
            let v = super::output::shape(&e.plan, &ran, &e.bounds, r);
            let _ = durable(&d.join("receipt.json"), &v.to_string());
            Found::Done { result: v }
        }
        // The dispatcher's durable word: it never got this call. The mark goes with the attempt.
        Some(DispatchAnswer { received: Some(false), refused: None, .. }) => {
            let _ = std::fs::remove_file(d.join("dispatched"));
            Found::NotDone
        }
        Some(DispatchAnswer { refused: Some(why), .. }) => {
            held(&format!("its dispatcher says: {}", r.mask_text(&why, false)))
        }
        _ => held("no receipt says what it did"),
    }
}

// The dispatcher's receipt asked for: a read's route, plan and credential too, so that its session,
// if it still runs, is killed.
fn receipt_request(home: &Home, d: &Path, e: &Envelope, call: String) -> DispatchRequest {
    let sent_to = std::fs::read_to_string(d.join("dispatched")).ok().map(|t| t.trim().to_owned());
    let credential = match &e.plan {
        Plan::Sql { .. } => super::credentials(home, &e.plan).ok().and_then(|(c, _)| c),
        _ => None,
    };
    let sql = credential.is_some();
    DispatchRequest {
        v: WIRE,
        call,
        route: sql.then(|| e.route.clone()),
        plan: sql.then(|| e.plan.clone()),
        credential,
        bounds: sql.then_some(e.bounds),
        receipt: true,
        redact: Vec::new(),
        store: sent_to.filter(|t| !t.is_empty()),
        into: None,
    }
}

/// Retries the kill of a read's recorded session (fac-184); true once the query has ended there,
/// killed now or no session left. No word from the dispatcher means false.
pub fn kill_again(home: &Home, bin: &Path, a: &Action) -> bool {
    let Ok((e, call)) = of(a) else { return false };
    let req = receipt_request(home, &dir(home, a.id.as_str()), &e, call);
    if req.credential.is_none() {
        return false;
    }
    let answer = send(home, bin, &e, &req, None).ok().as_ref().and_then(answer_of);
    matches!(answer, Some(DispatchAnswer { received: Some(true), refused: None, stopped, .. }) if stopped != Some(false))
}
