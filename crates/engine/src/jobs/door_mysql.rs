//! SQL calls keep credentials out of argv and close cancelled server connections permanently.
//! Same-session profiles are separated from results by a per-call boundary (fac-1, fac-92).

use std::path::Path;
use std::time::Duration;

use factory_types::door_plan::Ran;

use super::bounded;

const SESSION: &str = "CONNECTION_ID()";

/// The script's first statement, which names the session: the rest is held back until that name
/// is recorded, so no query runs on a connection nobody could kill (fac-183).
pub const FIRST: &str = "SELECT CONNECTION_ID();\n";

/// The session every query runs in.
pub const READ_ONLY: &str = "SET SESSION TRANSACTION READ ONLY";

/// Room for the session's id beyond a call's cap: however small the cap, the id is read whole,
/// and what the query returns is held to the cap alone.
pub const SESSION_ROOM: usize = 64;

/// Room for what follows the query's output beyond a call's cap, so an output that fits isn't
/// cut by it: the boundary, and the profile's header and two rows, one the query's first 300 bytes.
pub const PROFILE_ROOM: usize = 1024;

// The profile's header, as `SHOW PROFILES` prints it in batch.
const PROFILE: &str = "Query_ID\tDuration\tQuery\n";

// Profiling, where the server has it: one without it runs the query all the same, unprofiled.
const PROFILING: &str = "SET @factory_on = IF(@@have_profiling = 'YES', 'SET profiling = 1', 'DO 0'), \
                         @factory_show = IF(@@have_profiling = 'YES', 'SHOW PROFILES', 'DO 0');\n\
                         PREPARE factory_on FROM @factory_on;\nPREPARE factory_show FROM @factory_show;\n\
                         EXECUTE factory_on;\n";

// The column of the one-row result that ends the query's output.
const END: &str = "factory_end";

/// The client's option file: the login, and the server at a forward's socket or at its address.
pub fn options(user: &str, password: &str, at: &super::door_services::At<'_>) -> String {
    // Quoted, its quotes and backslashes escaped, as MySQL's option files read a value back.
    let q = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
    let server = match at {
        super::door_services::At::Socket(p) => format!("socket={}\nprotocol=SOCKET\n", q(&p.display().to_string())),
        super::door_services::At::Tcp(host, port) => format!("host={}\nport={port}\nprotocol=TCP\n", q(host)),
    };
    format!("[client]\nuser={}\npassword={}\n{server}", q(user), q(password))
}

/// The client's argv and stdin for one canonical read, at most `rows` rows and one more, ended at `timeout_ms`.
/// `end` must uniquely mark the end of this call's query output; its stdin begins with [`FIRST`].
pub fn command(
    options: &Path,
    database: &str,
    query: &str,
    end: &str,
    (rows, timeout_ms): (Option<u64>, u64),
) -> (Vec<String>, Vec<u8>) {
    let argv = vec![
        "mysql".into(),
        format!("--defaults-file={}", options.display()),
        // Initialization errors abort the connection even with --force, before any input runs.
        format!("--init-command={}", initialize(rows, timeout_ms)),
        "--default-character-set=utf8mb4".into(),
        "--binary-mode".into(),
        "--batch".into(),
        "--unbuffered".into(),
        "--force".into(),
        "--line-numbers".into(),
        "--skip-reconnect".into(),
        format!("--database={database}"),
        "--connect-timeout=10".into(),
    ];
    let stdin = format!("{FIRST}{PROFILING}{query};\n{};\nEXECUTE factory_show;\n", boundary(end));
    (argv, stdin.into_bytes())
}

// Composite quote modes expand again on assignment; their other expanded flags stay intact. The
// server ends a SELECT past the call's own deadline, whatever becomes of its client (fac-183).
fn initialize(rows: Option<u64>, timeout_ms: u64) -> String {
    let mut mode = "CONCAT(',', @@SESSION.sql_mode, ',')".to_owned();
    for name in ["NO_BACKSLASH_ESCAPES", "ANSI_QUOTES", "ANSI", "DB2", "MAXDB", "MSSQL", "ORACLE", "POSTGRESQL"] {
        mode = format!("REPLACE({mode}, ',{name},', ',')");
    }
    let verify = "SET SESSION sql_mode = IF(FIND_IN_SET('NO_BACKSLASH_ESCAPES', @@SESSION.sql_mode) \
                  OR FIND_IN_SET('ANSI_QUOTES', @@SESSION.sql_mode), 'factory_unsupported_lexical_mode', @@SESSION.sql_mode)";
    let cap = rows.map_or(String::new(), |n| format!("; SET SESSION sql_select_limit = {}", n.saturating_add(1)));
    format!(
        "{READ_ONLY}; SET SESSION max_execution_time = {timeout_ms}; \
         SET SESSION sql_mode = TRIM(BOTH ',' FROM {mode}); {verify}{cap}"
    )
}

/// Separates optional profiling failures from read failures using the generated script's lines.
/// Only a complete, error-free read and profile can supply server timing.
pub fn answer(mut ran: Ran, script: &[u8], end: &str, max: usize) -> Ran {
    ran.cut_at_cap = (ran.truncated && !ran.timed_out).then_some(true);
    let script = String::from_utf8_lossy(script);
    let mut optional = false;
    let errors: Vec<&str> = ran
        .stderr
        .lines()
        .filter(|line| {
            if optional_error(line, &script) {
                optional = true;
                false
            } else {
                !line.trim().is_empty()
            }
        })
        .collect();
    let complete = !ran.truncated && !ran.timed_out;
    if complete && !errors.is_empty() {
        ran.stderr = errors.join("\n");
        ran.exit = Some(ran.exit.filter(|e| *e != 0).unwrap_or(1));
    } else if complete && ran.exit == Some(0) {
        ran.stderr.clear();
        if session(ran.stdout.as_bytes()).is_none() || boundary_at(output(&ran.stdout), end).is_none() {
            ran.exit = Some(1);
            ran.stderr = "mysql: query completion framing is missing".into();
        }
    }
    let whole = complete && ran.exit == Some(0) && !optional;
    // A completed failure returns stderr, not rows; trimming unused rows must not turn its exit
    // into the successful truncated-output path.
    let limit = if complete && ran.exit != Some(0) { usize::MAX } else { max };
    let (out, over, server_us) = within(&ran.stdout, end, limit, whole);
    ran.stdout = out;
    ran.truncated |= over;
    ran.server_us = server_us;
    ran
}

fn optional_error(diagnostic: &str, script: &str) -> bool {
    let Some(error) = diagnostic.strip_prefix("ERROR ") else { return false };
    let Some((header, _)) = error.split_once(": ") else { return false };
    let Some((code, at)) = header.split_once(' ') else { return false };
    let Some(code) = code.parse::<u32>().ok() else { return false };
    // MySQL client codes include transport failures, even when the current statement is optional.
    if code < 1000 || (2000..3000).contains(&code) {
        return false;
    }
    let Some((state, line)) = at.split_once(" at line ") else { return false };
    if state.len() != 7 || !state.starts_with('(') || !state.ends_with(')') {
        return false;
    }
    let Some(index) = line.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) else { return false };
    let Some(statement) = script.lines().nth(index) else { return false };
    PROFILING.lines().chain(std::iter::once("EXECUTE factory_show;")).any(|s| s == statement)
}

/// The session's id, once the client has printed its whole line: a part of it would name another.
pub fn session(out: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(out);
    let (id, _) = text.strip_prefix(SESSION)?.strip_prefix('\n')?.split_once('\n')?;
    id.trim().parse().ok()
}

fn boundary(end: &str) -> String {
    format!("SELECT '{end}' AS {END}")
}

fn boundary_at(out: &str, end: &str) -> Option<(usize, usize)> {
    let block = format!("{END}\n{end}\n");
    let at = out.rfind(&block).filter(|&i| out[..i].is_empty() || out[..i].ends_with('\n'))?;
    Some((at, block.len()))
}

/// Query output capped at `max` bytes, its truncation flag, and optional server microseconds.
/// `end` separates metadata; timing requires a whole run with the query's profile.
pub fn within(out: &str, end: &str, max: usize, whole: bool) -> (String, bool, Option<u64>) {
    let out = output(out);
    let (own, server_us) = match boundary_at(out, end) {
        Some((i, len)) => (&out[..i], whole.then(|| profiled(&out[i + len..], end)).flatten()),
        None => (out, None),
    };
    let mut cut = max.min(own.len());
    while !own.is_char_boundary(cut) {
        cut -= 1;
    }
    (own[..cut].to_owned(), cut < own.len(), server_us)
}

// The query's time in the session's profile: the row before the boundary's, the boundary profiled
// too. None when either isn't listed (no profiling, or a short history), or a row isn't read whole.
fn profiled(trailer: &str, end: &str) -> Option<u64> {
    let rows = trailer.strip_prefix(PROFILE)?.strip_suffix('\n')?;
    let rows: Vec<(u64, u64, &str)> = rows.split('\n').map(profile_row).collect::<Option<_>>()?;
    let end = boundary(end);
    let query = rows.iter().find(|r| r.2.trim() == end)?.0.checked_sub(1)?;
    rows.iter().find(|r| r.0 == query).map(|r| r.1)
}

// A profile's row, `<id>\t<seconds>\t<statement>`: its id, its seconds in microseconds, its statement.
fn profile_row(line: &str) -> Option<(u64, u64, &str)> {
    let mut f = line.splitn(3, '\t');
    let (id, seconds, statement) = (f.next()?, f.next()?, f.next()?);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (whole, part) = seconds.split_once('.')?;
    if !(digits(id) && digits(whole) && digits(part)) {
        return None;
    }
    let micros: String = part.chars().chain(std::iter::repeat('0')).take(6).collect();
    let us = whole.parse::<u64>().ok()?.checked_mul(1_000_000)?.checked_add(micros.parse().ok()?)?;
    Some((id.parse().ok()?, us, statement))
}

/// The query's own output: what follows the session's id.
pub fn output(out: &str) -> &str {
    out.strip_prefix(SESSION)
        .and_then(|r| r.strip_prefix('\n'))
        .and_then(|r| r.split_once('\n'))
        .map_or(out, |(_, rest)| rest)
}

/// Closes the server connection `out` names, preventing further statements even with --force.
/// Returns whether the server accepted the kill or the connection was already gone.
pub fn kill(options: &Path, out: &[u8], env: &std::collections::BTreeMap<String, String>) -> bool {
    session(out).is_some_and(|id| kill_session(options, id, env))
}

/// Closes server connection `id`: whether the server accepted the kill or it was already gone.
pub fn kill_session(options: &Path, id: u64, env: &std::collections::BTreeMap<String, String>) -> bool {
    let argv = vec![
        "mysql".into(),
        format!("--defaults-file={}", options.display()),
        "--batch".into(),
        "--connect-timeout=10".into(),
        "-e".into(),
        format!("KILL CONNECTION {id}"),
    ];
    let b = bounded::Bounds { max_out: 4096, max_err: 4096, timeout: Duration::from_secs(15) };
    match bounded::run(None, "door", &bounded::Run { argv: &argv, env: Some(env), cwd: None, stdin: &[] }, b) {
        Ok(c) if c.exit == Some(0) => true,
        Ok(c) => String::from_utf8_lossy(&c.stderr).contains("ERROR 1094"),
        Err(_) => false,
    }
}
