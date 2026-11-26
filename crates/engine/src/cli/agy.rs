//! agy's stream (`agy --output-format stream-json`): its `init`, `step_update` and `result` events; its command, and
//! the one `user` event its stdin gives a turn's prompt.

use std::borrow::Cow;

use mork_types::config::{Cli, Seat};
use mork_types::cost::Tokens;
use mork_types::job::WallMode;
use serde_json::{Value, json};

use super::{StreamState, turn};

/// agy's sign-in under a CLI home, its own file where no session bus is (fac-657).
pub const TOKEN: &str = ".gemini/antigravity-cli/antigravity-oauth-token";

/// `agy …` for a turn: its model named with the seat's effort (`gemini-4-neon-high`, `claude-opus-5-5-high`), the
/// prompt read as stream-json from stdin and the reply in `<io>/schema.json`'s shape. `conversation` resumes it.
pub fn argv(seat: &Seat, conversation: Option<&str>, io: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![
        Cli::Agy.as_str().into(),
        "--model".into(),
        format!("{}-{}", seat.model, seat.effort.as_str()),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--json-schema".into(),
        format!("{io}/schema.json"),
        "--dangerously-skip-permissions".into(),
    ];
    if let Some(c) = conversation {
        a.extend(["--conversation".into(), c.to_owned()]);
    }
    a
}

/// Refuses an agy turn its wall wouldn't hold, whatever chose its seat: its tools are approved unasked (fac-657).
pub fn walled(seat: &Seat, wall: WallMode) -> Result<(), String> {
    if seat.cli == Cli::Agy && wall == WallMode::None {
        return Err(format!(
            "the `{}` seat would run agy unwalled: an agy seat runs walled alone, its tools approved unasked",
            seat.name
        ));
    }
    Ok(())
}

/// What a job's stdin holds: an agy turn's prompt as its one `user` event, any other's text as it is. The job keeps the
/// plain text, so a retry or a cut turn's recovery sends its prompt in one event again.
pub fn stdin<'a>(argv: &[String], text: &'a str) -> Cow<'a, str> {
    if argv.first().map(String::as_str) != Some(Cli::Agy.as_str()) {
        return Cow::Borrowed(text);
    }
    let mut line = json!({"event": "user", "message": {"content": text}}).to_string();
    line.push('\n');
    Cow::Owned(line)
}

/// The conversation a command resumes.
pub fn resumes(argv: &[String]) -> Option<&str> {
    argv.windows(2).find(|w| w[0] == "--conversation").map(|w| w[1].as_str())
}

/// Settled before the turn's decision: a resume answered in a new conversation fails the turn, its id unsaved (its ask
/// offers `fresh`), whatever quota that one met; else stderr's `AGY_ERROR` tells a quota stop, or the reset it lacked.
pub fn settle(argv: &[String], st: &mut StreamState, stderr: &str) {
    if let Some(asked) = resumes(argv)
        && let Some(other) = st.thread.take_if(|t| t != asked)
    {
        st.limit_rejected = None;
        st.answered = false;
        st.compacted = false;
        st.context = None;
        st.result = Some(Err(format!("agy found no conversation `{asked}`: it answered in a new one, `{other}`")));
        return;
    }
    if st.limit_rejected.flatten().is_none() {
        let errors = stderr.lines().filter_map(|l| l.trim_start().strip_prefix("AGY_ERROR"));
        let told: Vec<Option<i64>> = errors.filter_map(|e| quota(&Value::String(e.to_owned()))).collect();
        if !told.is_empty() {
            st.limit_rejected = Some(told.into_iter().flatten().next());
        }
    }
}

// An error's words, as the JSON they hold when they hold some.
fn error_of(words: &str) -> Value {
    let json = words.find('{').and_then(|i| serde_json::from_str::<Value>(&words[i..]).ok());
    json.unwrap_or_else(|| Value::String(words.to_owned()))
}

// A quota stop, by its status (`RESOURCE_EXHAUSTED`, HTTP 429), and the reset it reports, when it reports one.
fn quota(error: &Value) -> Option<Option<i64>> {
    (bare_429(error) || quota_status(error)).then(|| reset_of(error))
}

// The whole payload a status code, as a number or in words: within a diagnostic, a lone 429 is no status.
fn bare_429(error: &Value) -> bool {
    let words = error.as_str().map(|s| s.trim_matches(|c: char| c == ':' || c == '"' || c.is_whitespace()));
    error.as_u64() == Some(429) || words == Some("429")
}

const STATUS_KEYS: [&str; 6] = ["status", "code", "http_status", "httpStatus", "status_code", "statusCode"];

fn quota_status(v: &Value) -> bool {
    let quota_code = |v: &Value| v.as_u64() == Some(429) || matches!(v.as_str(), Some("RESOURCE_EXHAUSTED" | "429"));
    match v {
        Value::Object(o) => {
            o.iter().any(|(k, v)| (STATUS_KEYS.contains(&k.as_str()) && quota_code(v)) || quota_status(v))
        }
        Value::Array(a) => a.iter().any(quota_status),
        Value::String(s) => {
            quota_words(s)
                || match error_of(s) {
                    Value::String(_) => false,
                    json => quota_status(&json),
                }
        }
        _ => false,
    }
}

// In words, the status's name, or 429 named as a status: any other number in a diagnostic is no quota.
fn quota_words(s: &str) -> bool {
    let words: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let named = |p: &[String]| ["http", "status", "code"].contains(&p[0].as_str()) && p[1] == "429";
    let phrased = |p: &[String]| p[0] == "429" && p[1] == "too";
    words.iter().any(|w| w == "resource_exhausted") || words.windows(2).any(|p| named(p) || phrased(p))
}

fn reset_of(v: &Value) -> Option<i64> {
    match v {
        Value::Object(o) => o.iter().find_map(|(k, v)| match v.as_str() {
            Some(t) if ["reset_time", "resetTime"].contains(&k.as_str()) => super::limits::rfc3339_secs(t),
            _ => reset_of(v),
        }),
        Value::Array(a) => a.iter().find_map(reset_of),
        Value::String(s) => match error_of(s) {
            Value::String(_) => None,
            json => reset_of(&json),
        },
        _ => None,
    }
}

/// Reads one complete line of the stream into the state.
pub fn read_line(st: &mut StreamState, line: &str) {
    let Ok(ev) = serde_json::from_str::<Value>(line) else { return };
    let conversation = |payload: Option<&Value>| {
        ev.get("conversation_id").or_else(|| payload?.get("conversation_id")).and_then(Value::as_str).map(str::to_owned)
    };
    match ev.get("event").and_then(Value::as_str) {
        Some("init") => {
            st.session_started = true;
            turn::started(st);
            st.thread = conversation(ev.get("init")).or(st.thread.take());
            if let Some(m) = ev.pointer("/init/model").and_then(Value::as_str).filter(|m| !m.is_empty()) {
                st.model = Some(m.to_owned());
            }
        }
        Some("step_update") => {
            if let Some(s) = ev.get("step_update") {
                step(st, s, conversation(Some(s)).unwrap_or_default());
            }
        }
        Some("result") => {
            if let Some(r) = ev.get("result") {
                usage(st, r, conversation(Some(r)).or_else(|| st.thread.clone()));
                result(st, &ev, r);
            }
        }
        _ => {}
    }
}

// Main-conversation state: checkpoint marks compaction, agent_response updates context, and tools/responses mark
// answered; subagent steps carry another conversation_id and leave the parent's state untouched (fac-681).
fn main_step(st: &mut StreamState, u: &Value, kind: Option<&str>, state: Option<&str>) {
    match kind {
        Some("checkpoint") if state == Some("DONE") => st.compacted = true,
        Some("agent_response") => {
            st.answered = true;
            context_of(st, u);
        }
        Some("tool") => st.answered = true,
        _ => {}
    }
}

fn tokens_of(v: &Value) -> Option<Tokens> {
    let count = |k: &str| v.get("usage")?.get(k)?.as_u64();
    let (input, output) = (count("input_tokens")?, count("output_tokens")?);
    Some(Tokens { input, cache_read: count("cache_read_tokens").unwrap_or(0), cache_write: 0, output })
}

fn context_of(st: &mut StreamState, u: &Value) {
    let Some(t) = tokens_of(u) else { return };
    let used = t.input.saturating_add(t.cache_read).saturating_add(t.output);
    st.context = Some(super::ContextReport { used, window: None, model: st.model.clone() });
}

// A tool's step, once per conversation and index, ended once it is done; a command its command, any other its tool.
fn step(st: &mut StreamState, u: &Value, conversation: String) {
    let field = |k: &str| u.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let (kind, state) = (field("step_type"), field("state"));
    if conversation.is_empty() || st.thread.as_deref().is_none_or(|t| t == conversation) {
        main_step(st, u, kind, state);
    }
    let Some(index) = u.get("step_index").and_then(Value::as_u64) else { return };
    if kind != Some("tool") {
        return;
    }
    let id = format!("{conversation}\0{index}");
    let tool = field("tool_name").or_else(|| u.pointer("/tool_info/name").and_then(Value::as_str)).unwrap_or("tool");
    let raw = u.pointer("/tool_info/parameters/CommandLine").and_then(Value::as_str).filter(|c| !c.trim().is_empty());
    let file = file_words(st, tool, u);
    let detailed = raw.is_some() || file.is_some();
    let (words, command) = match raw {
        Some(c) => super::command::display(st.seat.as_deref(), c, None),
        None => (file.unwrap_or_else(|| tool.to_owned()), None),
    };
    let words = words.chars().take(100).collect();
    match st.steps.iter_mut().find(|s| s.id == id) {
        // Its end may say what its start didn't, as its command or its file.
        Some(s) if detailed => {
            s.words = words;
            s.command = command;
        }
        Some(_) => {}
        None => turn::step(st, None, &id, words, command),
    }
    if field("state") == Some("DONE") {
        turn::ended(st, &id);
    }
}

// A file tool names its file as Claude's do (fac-691); agy's transcripts call it `Path`.
fn file_words(st: &StreamState, tool: &str, u: &Value) -> Option<String> {
    let verb = match tool {
        "read_file" => "Read",
        "edit_file" => "Changed",
        "write_file" => "Wrote",
        _ => return None,
    };
    let path = u.pointer("/tool_info/parameters/Path").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
    path.map(|p| format!("{verb} {}", super::claude::path_words(p, &st.folder, &st.home)))
}

// Its counts are the conversation's so far, across resumes: input apart from cache reads, output with thinking.
fn usage(st: &mut StreamState, r: &Value, conversation: Option<String>) {
    if let (Some(conversation), Some(tokens)) = (conversation, tokens_of(r)) {
        st.token_totals.insert(conversation, tokens);
    }
}

// The first result holds: a reply in its schema's shape, else its words, which the checker refuses; a failed or timed
// out turn is an error.
fn result(st: &mut StreamState, ev: &Value, r: &Value) {
    turn::finished(st);
    if st.result.is_some() {
        return;
    }
    let field = |k: &str| r.get(k).or_else(|| ev.get(k)).and_then(Value::as_str).filter(|v| !v.is_empty());
    let status = field("status").unwrap_or("no status");
    if status == "SUCCESS" {
        st.answered = true;
    } else {
        st.limit_rejected = r.get("error").or_else(|| ev.get("error")).and_then(quota);
    }
    st.result = Some(match r.get("structured_output") {
        _ if field("finished_reason") == Some("print_timeout") => Err("agy's turn timed out (print_timeout)".into()),
        _ if status != "SUCCESS" => {
            Err(field("error").map_or_else(|| format!("agy's turn ended {status}"), str::to_owned))
        }
        Some(v) if !v.is_null() => Ok(v.clone()),
        _ => Ok(Value::String(format!("agy returned no structured output: {}", field("response").unwrap_or("")))),
    });
}
