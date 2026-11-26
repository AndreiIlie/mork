//! `mork door-dispatch [--allow sql,mongo,http,file,sh] [--receipts <dir>] [--tunnels <dir>]` (fac-1, fac-86):
//! runs only allowed kinds, as argv never through a local shell, masking secrets; with `--receipts` a call runs once.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use mork_types::door_plan::{Bounds, DispatchAnswer, DispatchRequest, Plan, Session, WIRE};

use super::door_services::{Call, execute, kill};
use crate::door::redact::Redactor;

/// The dispatcher's own settings, from its argv.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub allow: Vec<String>,
    pub receipts: Option<PathBuf>,
    /// Where a call's own folder is made: its forward's socket, its client's option files.
    pub tunnels: Option<PathBuf>,
}

/// Reads the argv after `door-dispatch`.
pub fn settings(args: &[String]) -> Result<Settings, String> {
    let mut s = Settings::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--allow" => s.allow = value()?.split(',').map(str::to_owned).collect(),
            "--receipts" => s.receipts = Some(PathBuf::from(value()?)),
            "--tunnels" => s.tunnels = Some(PathBuf::from(value()?)),
            other => return Err(format!("door-dispatch: unknown argument `{other}`")),
        }
    }
    Ok(s)
}

/// Runs the dispatcher; returns its exit code. Its answer is one line on stdout.
pub fn main(args: &[String]) -> i32 {
    let answer = match settings(args) {
        Ok(s) => serve(&s),
        Err(e) => refused(&e),
    };
    let mut out = std::io::stdout();
    let line = serde_json::to_string(&answer).unwrap_or_default();
    i32::from(writeln!(out, "{line}").and_then(|()| out.flush()).is_err())
}

fn refused(why: &str) -> DispatchAnswer {
    DispatchAnswer { v: WIRE, refused: Some(why.to_owned()), ..DispatchAnswer::default() }
}

fn serve(s: &Settings) -> DispatchAnswer {
    let mut line = String::new();
    let read = std::io::stdin().lock().take(1 << 20).read_line(&mut line);
    let req: DispatchRequest = match read
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_str(line.trim()).map_err(|e| format!("the request isn't one: {e}")))
    {
        Ok(r) => r,
        Err(e) => return refused(&e),
    };
    if req.v != WIRE {
        return refused(&format!("wire version {} isn't this dispatcher's ({WIRE})", req.v));
    }
    let call_ok = !req.call.is_empty()
        && req.call.len() <= 128
        && req.call.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !call_ok {
        return refused("the call's id isn't one");
    }
    if req.receipt {
        let answer = receipt(s, &req.call, req.store.as_deref());
        // Received with no answer, or answered as a read its server didn't take the kill of: it
        // may still run there, and is killed (fac-183, fac-184).
        let running = answer.ran.as_ref().is_none_or(|r| r.stopped == Some(false));
        return if answer.received == Some(true) && running { stop(s, &req, answer) } else { answer };
    }
    let (Some(plan), Some(bounds)) = (&req.plan, req.bounds) else { return refused("no plan to run") };
    run(s, &req, plan, bounds)
}

// Received with no answer: its outcome is unknown. Never received: it never ran, but only the
// store the call was sent to can say so; a store gone, or made again, can't.
fn receipt(s: &Settings, call: &str, sent_to: Option<&str>) -> DispatchAnswer {
    let Some(dir) = &s.receipts else { return refused("this dispatcher keeps no receipts") };
    if !dir.is_dir() {
        return refused("its receipts folder is gone: whether the call ran can't be told");
    }
    let store = store_of(dir, false);
    if let Ok(text) = std::fs::read_to_string(dir.join(format!("{call}.json")))
        && let Ok(a) = serde_json::from_str::<DispatchAnswer>(&text)
    {
        return DispatchAnswer { received: Some(true), store, ..a };
    }
    let received = dir.join(format!("{call}.received")).exists();
    if !received && (store.is_none() || store.as_deref() != sent_to) {
        return refused("its receipts store isn't the one the call was sent to: whether it ran can't be told");
    }
    DispatchAnswer { v: WIRE, received: Some(received), store, ..DispatchAnswer::default() }
}

// A read received with no answer may still run: its recorded session is killed, unless its server's
// own limit ended it (its id may then name another's). The record goes once the session is gone.
fn stop(s: &Settings, req: &DispatchRequest, answer: DispatchAnswer) -> DispatchAnswer {
    let (Some(dir), Some(route), Some(plan @ Plan::Sql { .. }), Some(pair)) =
        (&s.receipts, &req.route, &req.plan, &req.credential)
    else {
        return answer;
    };
    let path = dir.join(format!("{}.session", req.call));
    let Some(recorded) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Session>(&t).ok())
    else {
        return answer;
    };
    let answer = DispatchAnswer { session: Some(recorded), ..answer };
    if recorded.until_ms < crate::clock::now_ms() {
        let _ = std::fs::remove_file(&path);
        return answer;
    }
    if !s.allow.iter().any(|k| k == plan.kind()) {
        return DispatchAnswer { stopped: Some(false), ..answer };
    }
    let bounds = req.bounds.unwrap_or(Bounds { max_bytes: 0, max_items: 0, timeout_ms: 0 });
    let c = Call {
        id: &req.call,
        route,
        plan,
        credential: Some(pair),
        bounds,
        tunnels: s.tunnels.as_deref(),
        into: None,
        session: None,
    };
    let gone = kill(&c, recorded).unwrap_or(false);
    if gone {
        let _ = std::fs::remove_file(&path);
    }
    DispatchAnswer { stopped: Some(gone), ..answer }
}

// Made once with its folder, and published whole: a folder made again gets another.
fn store_of(dir: &Path, make: bool) -> Option<String> {
    let path = dir.join("store.id");
    let read = || std::fs::read_to_string(&path).ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    if let Some(id) = read() {
        return Some(id);
    }
    if make {
        let id = crate::random::uuid();
        let tmp = dir.join(format!("store.id.{id}"));
        if durable(&tmp, &id).is_ok() {
            let _ = std::fs::hard_link(&tmp, &path);
            let _ = std::fs::remove_file(&tmp);
        }
    }
    read()
}

pub(super) fn durable(path: &Path, text: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

fn run(s: &Settings, req: &DispatchRequest, plan: &Plan, bounds: Bounds) -> DispatchAnswer {
    let Some(route) = &req.route else { return refused("no route to run it on") };
    if !s.allow.iter().any(|k| k == plan.kind()) {
        return refused(&format!("this dispatcher doesn't allow `{}`", plan.kind()));
    }
    // The gate again, on what runs: a query it didn't give back as itself never ran through it.
    if let Plan::Sql { query, .. } = plan {
        let Some(read) = mork_core::door::sql::read(query).ok().filter(|r| &r.text() == query) else {
            return refused("its query isn't a canonical read");
        };
        if let Err(e) = read.timed(bounds.timeout_ms) {
            return refused(&e);
        }
    }
    if matches!(plan, Plan::Get { .. }) != req.into.is_some() {
        return refused("its file's place is missing, or isn't its plan's");
    }
    let service = matches!(plan, Plan::Sql { .. } | Plan::Mongo { .. } | Plan::Http { .. });
    if service != req.credential.is_some() {
        return refused("its credential is missing, or isn't its plan's");
    }
    let mut redactor = Redactor::default();
    for v in &req.redact {
        redactor.add(v);
    }
    if let Some(p) = &req.credential {
        redactor.add(&p.secret);
        if matches!(plan, Plan::Http { .. }) {
            redactor.add(&p.name);
        }
    }
    let call = &req.call;
    let mut store = None;
    if let Some(dir) = &s.receipts {
        // Durably received before it runs: a second delivery of the same call runs nothing.
        if !dir.is_dir() {
            return refused("its receipts folder is gone, so nothing runs");
        }
        store = store_of(dir, true);
        if store.is_none() {
            return refused("its receipts store can't be named, so nothing runs");
        }
        match durable(&dir.join(format!("{call}.received")), "") {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return refused("this call reached the dispatcher already: it never runs twice");
            }
            Err(e) => return refused(&format!("its receipt can't be written: {e}")),
        }
    }
    let (credential, tunnels, into) =
        (req.credential.as_ref(), s.tunnels.as_deref(), req.into.as_deref().map(Path::new));
    let session = s.receipts.as_ref().map(|d| d.join(format!("{call}.session")));
    let c = Call { id: call, route, plan, credential, bounds, tunnels, into, session: session.as_deref() };
    let answer = match execute(&c, &mut redactor) {
        Ok(ran) => DispatchAnswer { v: WIRE, ran: Some(ran), store, ..DispatchAnswer::default() },
        Err(why) => DispatchAnswer { store, ..refused(&redactor.mask_text(&why, false)) },
    };
    if let Some(dir) = &s.receipts {
        let _ = durable(&dir.join(format!("{call}.json")), &serde_json::to_string(&answer).unwrap_or_default());
    }
    answer
}
