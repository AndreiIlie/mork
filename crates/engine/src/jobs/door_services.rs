//! What a door call runs (fac-86), where its route says: here, or over Andrei's ssh alias. A credential is
//! written only to the call's own option files or a client's stdin; a remote run with no conclusive result is `lost`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use factory_types::door::FILE_GET_MAX;
use factory_types::door_plan::{Bounds, Pair, Plan, Ran, Route, Session};

use super::bounded::{self, Hold, Watch};
use super::door_files::{deliver, get_beneath, last_lines, read_beneath, tail};
use super::door_ssh::{self, Scratch, Tunnel};
use super::{door_libra, door_mysql};
use crate::clock;
use crate::door::redact::Redactor;

/// Where a client reaches its service: a forward's socket, or an address of this machine's.
pub enum At<'a> {
    Socket(&'a Path),
    Tcp(&'a str, u16),
}

pub struct Call<'a> {
    pub id: &'a str,
    pub route: &'a Route,
    pub plan: &'a Plan,
    pub credential: Option<&'a Pair>,
    pub bounds: Bounds,
    /// The folder the calls' own folders are made in.
    pub tunnels: Option<&'a Path>,
    /// Where a `Get` writes its file.
    pub into: Option<&'a Path>,
    /// Where a read's server session is recorded while its query runs.
    pub session: Option<&'a Path>,
}

/// Runs it, its output cut and masked; or why nothing ran, in words for the caller to mask.
pub fn execute(c: &Call<'_>, r: &mut Redactor) -> Result<Ran, String> {
    let max = usize::try_from(c.bounds.max_bytes).unwrap_or(usize::MAX);
    let start = clock::instant();
    let deadline = start + Duration::from_millis(c.bounds.timeout_ms);
    let mut ran = match (c.route, c.plan) {
        (Route::Local { .. }, Plan::Read { root, path }) => read_beneath(root, path, max),
        (Route::Local { .. }, Plan::Get { root, path }) => get_beneath(root, path, into(c)?),
        (Route::Local { .. }, Plan::Tail { path, lines }) => tail(path, *lines, max),
        (Route::Local { .. }, Plan::Run { argv }) => helper(argv, &[], &env(), max, deadline, &Watch::default()),
        (Route::Ssh { alias, .. }, Plan::Read { root, path }) => {
            let ran = remote(alias, &door_ssh::read_argv("head", max as u64 + 1, root, path), max + 1, deadline);
            within(ran, max)
        }
        (Route::Ssh { alias, .. }, Plan::Get { root, path }) => get_remote(alias, root, path, into(c)?, deadline),
        (Route::Ssh { alias, .. }, Plan::Tail { path, lines }) => {
            let ran = remote(alias, &door_ssh::read_argv("tail", max as u64 + 1, path, ""), max + 1, deadline);
            tail_of(ran, *lines, max)
        }
        (Route::Ssh { alias, misc, .. }, Plan::Run { argv }) => {
            let mut ran = remote(alias, argv, max, deadline);
            // Cut at its cap, the misc door's command may go on there.
            ran.lost |= *misc && ran.truncated;
            ran
        }
        (_, Plan::Sql { .. } | Plan::Mongo { .. } | Plan::Libra { .. }) => service(c, max, deadline, r)?,
    };
    ran.elapsed = u64::try_from(clock::elapsed(start).as_millis()).unwrap_or(u64::MAX);
    ran.stdout = match c.plan {
        Plan::Libra { .. } | Plan::Mongo { .. } => r.mask_json(&ran.stdout),
        _ => r.mask_text(&ran.stdout, ran.truncated),
    };
    ran.stderr = r.mask_text(&ran.stderr, true);
    Ok(ran)
}

fn into<'a>(c: &Call<'a>) -> Result<&'a Path, String> {
    c.into.ok_or_else(|| "file.get has no io/ folder to write to".to_owned())
}

// One byte past the cap is read, so a bigger file is refused, never cut; its bytes as they came.
fn get_remote(alias: &str, root: &str, path: &str, into: &Path, deadline: Instant) -> Ran {
    let n = FILE_GET_MAX + 1;
    let argv = door_ssh::command_argv(alias, &door_ssh::read_argv("head", n, root, path));
    let max = usize::try_from(n).unwrap_or(usize::MAX);
    let (mut ran, bytes) = captured(&argv, &door_ssh::env(env()), max, deadline);
    ran.lost = ran.exit == Some(door_ssh::SSH_FAILED) || ran.timed_out;
    if ran.exit != Some(0) || ran.lost {
        return ran;
    }
    deliver(path, &bytes, into)
}

// Nothing of the caller's but `PATH` and `HOME`, and never interactive.
fn env() -> BTreeMap<String, String> {
    let mut e = bounded::door_env();
    for (k, v) in [("LC_ALL", "C.UTF-8"), ("TERM", "dumb"), ("PAGER", "cat"), ("SYSTEMD_PAGER", ""), ("NO_COLOR", "1")]
    {
        e.insert(k.to_owned(), v.to_owned());
    }
    e
}

fn left(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(clock::instant())
}

// One program run here, bounded and watched.
fn helper(
    argv: &[String],
    stdin: &[u8],
    env: &BTreeMap<String, String>,
    max: usize,
    deadline: Instant,
    watch: &Watch<'_>,
) -> Ran {
    let (ran, out) = run_bytes(argv, stdin, env, max, deadline, watch);
    Ran { stdout: String::from_utf8_lossy(&out).into_owned(), ..ran }
}

// As `helper`, its stdout's bytes apart and untouched.
fn captured(argv: &[String], env: &BTreeMap<String, String>, max: usize, deadline: Instant) -> (Ran, Vec<u8>) {
    run_bytes(argv, &[], env, max, deadline, &Watch::default())
}

fn run_bytes(
    argv: &[String],
    stdin: &[u8],
    env: &BTreeMap<String, String>,
    max: usize,
    deadline: Instant,
    watch: &Watch<'_>,
) -> (Ran, Vec<u8>) {
    let b = bounded::Bounds { max_out: max, max_err: 16 * 1024, timeout: left(deadline) };
    match bounded::run_watched(None, "door", &bounded::Run { argv, env: Some(env), cwd: None, stdin }, b, watch) {
        Ok(c) => {
            let ran = Ran {
                exit: c.exit,
                stderr: String::from_utf8_lossy(&c.stderr).into_owned(),
                truncated: c.truncated,
                timed_out: c.timed_out,
                ..Ran::default()
            };
            (ran, c.stdout)
        }
        Err(e) => {
            let words = format!("{}: {e}", argv.first().map_or("", String::as_str));
            (Ran { exit: Some(127), stderr: words, ..Ran::default() }, Vec::new())
        }
    }
}

// A remote argv over the alias. ssh ended it itself (255), or it passed its deadline with ssh
// killed here: whether, or how far, it ran there is unknown.
fn remote(alias: &str, argv: &[String], max: usize, deadline: Instant) -> Ran {
    let mut ran =
        helper(&door_ssh::command_argv(alias, argv), &[], &door_ssh::env(env()), max, deadline, &Watch::default());
    ran.lost = ran.exit == Some(door_ssh::SSH_FAILED) || ran.timed_out;
    ran
}

// A file read remotely one byte past the bound: there was more.
fn within(mut ran: Ran, max: usize) -> Ran {
    if ran.exit == Some(0) && ran.stdout.len() > max {
        let mut end = max;
        while !ran.stdout.is_char_boundary(end) {
            end -= 1;
        }
        ran.stdout.truncate(end);
        ran.truncated = true;
    }
    ran
}

// A log's end read remotely one byte past the bound: the log is longer, and was read from inside it.
fn tail_of(ran: Ran, lines: u64, max: usize) -> Ran {
    if ran.exit != Some(0) {
        return ran;
    }
    let bytes = ran.stdout.as_bytes();
    let inside = bytes.len() > max;
    Ran { elapsed: ran.elapsed, ..last_lines(&bytes[bytes.len().saturating_sub(max)..], inside, lines) }
}

// The service's address as its target sees it.
fn address(plan: &Plan) -> Result<(&str, u16), String> {
    match plan {
        Plan::Sql { host, port, .. } | Plan::Mongo { host, port, .. } => Ok((host, *port)),
        Plan::Libra { origin, .. } => door_libra::address(origin).ok_or_else(|| "its origin isn't one".to_owned()),
        _ => Err("not a service".into()),
    }
}

// Its own folder, its forward when the route is ssh, then its client. A forward lost while a
// client failed leaves it unknown how far the client got.
fn service(c: &Call<'_>, max: usize, deadline: Instant, r: &mut Redactor) -> Result<Ran, String> {
    let (mut ran, lost) = reached(c, deadline, |scratch, pair, at| {
        Ok(match c.plan {
            Plan::Sql { database, query, .. } => {
                let rows = Some(c.bounds.max_items).filter(|n| *n != u64::MAX);
                sql(scratch, pair, at, (database, query, (rows, c.bounds.timeout_ms)), (max, c.session), deadline)?
            }
            Plan::Mongo { .. } => mongo(scratch, pair, at, c.plan, max, deadline)?,
            _ => libra(pair, at, c.plan, max, deadline, r),
        })
    })?;
    ran.lost |= ran.exit != Some(0) && lost;
    Ok(ran)
}

/// Kills a read's recorded session on its server, reached as its read was: whether it is gone.
pub fn kill(c: &Call<'_>, recorded: Session) -> Result<bool, String> {
    let deadline = clock::instant() + Duration::from_secs(30);
    let (gone, _) = reached(c, deadline, |scratch, pair, at| {
        let options = scratch.secret("my.cnf", &door_mysql::options(&pair.name, &pair.secret, at))?;
        Ok(door_mysql::kill_session(&options, recorded.session, &env()))
    })?;
    Ok(gone)
}

// Runs `client` in the call's own folder, with its forward when the route is ssh; and whether that
// forward was lost by the end.
fn reached<T>(
    c: &Call<'_>,
    deadline: Instant,
    client: impl FnOnce(&Scratch, &Pair, &At<'_>) -> Result<T, String>,
) -> Result<(T, bool), String> {
    let pair = c.credential.ok_or("its credential wasn't sent, so nothing runs")?;
    let (host, port) = address(c.plan)?;
    let scratch = Scratch::make(c.tunnels.ok_or("this dispatcher has no tunnels folder")?, c.id)?;
    let socket = scratch.socket();
    let mut tunnel = match c.route {
        Route::Ssh { alias, .. } => {
            let argv = door_ssh::tunnel_argv(alias, &socket, host, port);
            Some(Tunnel::open(&argv, &socket, &door_ssh::env(env()), deadline)?)
        }
        Route::Local { .. } => None,
    };
    let at = if tunnel.is_some() { At::Socket(&socket) } else { At::Tcp(host, port) };
    let done = client(&scratch, pair, &at)?;
    Ok((done, tunnel.as_mut().is_some_and(Tunnel::lost)))
}

// Only its session's id is asked for until that id is recorded: a query never runs on a session
// nobody could kill. The record goes when the client has ended, unless the kill of a cut one failed.
fn sql(
    s: &Scratch,
    pair: &Pair,
    at: &At<'_>,
    (database, query, bounds): (&str, &str, (Option<u64>, u64)),
    (max, record): (usize, Option<&Path>),
    deadline: Instant,
) -> Result<Ran, String> {
    let options = s.secret("my.cnf", &door_mysql::options(&pair.name, &pair.secret, at))?;
    // A boundary of this call's own: no query's output can hold it.
    let end = crate::random::uuid();
    let (argv, stdin) = door_mysql::command(&options, database, query, &end, bounds);
    let env = env();
    let stopped = std::cell::Cell::new(None);
    let unrecorded = std::cell::RefCell::new(None);
    let recorded = std::cell::Cell::new(None);
    let cut = |out: &[u8]| stopped.set(Some(door_mysql::kill(&options, out, &env)));
    let first = |out: &[u8]| {
        let Some(session) = door_mysql::session(out) else { return Hold::Wait };
        let Some(path) = record else { return Hold::Release };
        let until_ms = clock::real_now_ms().saturating_add(i64::try_from(bounds.1).unwrap_or(i64::MAX));
        let kept = Session { session, limit_ms: bounds.1, until_ms };
        match super::door_dispatch::durable(path, &serde_json::to_string(&kept).unwrap_or_default()) {
            Ok(()) => {
                recorded.set(Some(kept));
                Hold::Release
            }
            Err(e) => {
                *unrecorded.borrow_mut() =
                    Some(format!("its session couldn't be recorded, so its query never ran: {e}"));
                Hold::Cut
            }
        }
    };
    let watch = Watch { cut: Some(&cut), stop: None, hold: Some((door_mysql::FIRST.len(), &first)) };
    let room = door_mysql::SESSION_ROOM + door_mysql::PROFILE_ROOM;
    let mut ran = helper(&argv, &stdin, &env, max.saturating_add(room), deadline, &watch);
    ran.stopped = stopped.get();
    if let Some(path) = record
        && ran.stopped != Some(false)
    {
        let _ = std::fs::remove_file(path);
    }
    if let Some(why) = unrecorded.into_inner() {
        return Err(why);
    }
    Ok(Ran { session: recorded.get(), ..door_mysql::answer(ran, &stdin, &end, max) })
}

fn yaml(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

// Every byte but RFC 3986's unreserved ones percent-encoded, as a URI's user or host.
fn encoded(v: &str) -> String {
    v.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `mongoexport`'s config: its URI (the login's user, the server, the database it
/// authenticates against) and the password, apart.
pub fn mongo_config(pair: &Pair, at: &At<'_>, auth: &str) -> String {
    let server = match at {
        At::Socket(p) => encoded(&p.display().to_string()),
        At::Tcp(host, port) => format!("{host}:{port}"),
    };
    let uri = format!("mongodb://{}@{server}/?authSource={}", encoded(&pair.name), encoded(auth));
    format!("uri: {}\npassword: {}\n", yaml(&uri), yaml(&pair.secret))
}

fn mongo(s: &Scratch, pair: &Pair, at: &At<'_>, plan: &Plan, max: usize, deadline: Instant) -> Result<Ran, String> {
    let Plan::Mongo { database, auth_database, collection, query, fields, sort, limit, .. } = plan else {
        return Err("not a find".into());
    };
    let config = s.secret("mongo.yaml", &mongo_config(pair, at, auth_database.as_deref().unwrap_or(database)))?;
    let mut argv = vec![
        "mongoexport".into(),
        format!("--config={}", config.display()),
        format!("--db={database}"),
        format!("--collection={collection}"),
        format!("--query={query}"),
        format!("--limit={limit}"),
        "--jsonFormat=relaxed".into(),
        "--quiet".into(),
    ];
    if sort.as_object().is_some_and(|o| !o.is_empty()) {
        argv.push(format!("--sort={sort}"));
    }
    if !fields.is_empty() {
        argv.push(format!("--fields={}", fields.join(",")));
    }
    Ok(helper(&argv, &[], &env(), max, deadline, &Watch::default()))
}

// The token first, every token in its answer masked before it is judged; refused, the GET is never
// made and only Libra's error is kept. A GET answered other than 2xx fails too, Libra's words kept.
fn libra(pair: &Pair, at: &At<'_>, plan: &Plan, max: usize, deadline: Instant, r: &mut Redactor) -> Ran {
    let Plan::Libra { origin, token_path, scope, path, .. } = plan else { return Ran::default() };
    let env = env();
    let seconds = || left(deadline).as_secs().max(1);
    let config = door_libra::token(origin, token_path, scope, &pair.name, &pair.secret, at, seconds());
    let got = helper(&door_libra::argv(), config.as_bytes(), &env, 64 * 1024, deadline, &Watch::default());
    let (body, status) = door_libra::answer(&got.stdout);
    for t in door_libra::tokens_of(body) {
        r.add(&t);
    }
    let token = door_libra::token_of(body).filter(|_| got.exit == Some(0) && status == Some(200));
    let Some(token) = token else {
        let said = door_libra::refusal(body);
        let why = format!("Libra's token request failed (HTTP {status:?}): {} {said}", got.stderr.trim());
        let exit = Some(got.exit.filter(|e| *e != 0).unwrap_or(22));
        return Ran { exit, stdout: String::new(), stderr: why, timed_out: got.timed_out, ..Ran::default() };
    };
    r.add(&token);
    let config = door_libra::get(origin, path, &token, at, seconds());
    let mut ran =
        helper(&door_libra::argv(), config.as_bytes(), &env, max.saturating_add(8), deadline, &Watch::default());
    let (body, status) = door_libra::answer(&ran.stdout);
    let body = body.to_owned();
    if ran.exit == Some(0) && !status.is_some_and(|s| (200..300).contains(&s)) {
        ran.stderr = format!("Libra answered HTTP {status:?}: {body}");
        ran.exit = Some(22);
    }
    ran.stdout = body;
    ran
}
