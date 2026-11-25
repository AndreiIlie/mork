//! Claude's stream (`--output-format stream-json --verbose`), F-C1 to F-C5.

use factory_types::config::{CLAUDE_BLOCKED, Seat};
use factory_types::seen::DetailKind::{Text, Thinking};
use factory_types::seen::{ListStep, StepStatus};
use serde_json::Value;

use super::{ContextReport, StreamState, limits, turn};

/// Reads one complete line of the stream into the state. Reading a line twice changes nothing.
pub fn read_line(st: &mut StreamState, line: &str) {
    let Ok(ev) = serde_json::from_str::<Value>(line) else { return };
    match ev.get("type").and_then(Value::as_str) {
        Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("init") => {
            st.session_started = true;
            turn::started(st);
        }
        Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => st.compacted = true,
        Some("assistant") => {
            answered(st, &ev);
            steps_of(st, &ev);
            context_of(st, &ev);
        }
        Some("command_lifecycle") => lifecycle(st, &ev),
        Some("user") => {
            results_of(st, &ev);
            planner_results(st, &ev);
        }
        Some("result") => {
            turn::finished(st);
            // A later background-task result must not replace a usable reply (fac-315).
            if !matches!(&st.result, Some(Ok(v)) if !v.is_string()) {
                st.result = Some(result_of(&ev));
            }
            if let Some(c) = ev.get("total_cost_usd").and_then(Value::as_f64) {
                st.cost_usd = Some(c);
            }
            super::usage::claude(st, &ev);
            correct_context(st, &ev);
        }
        Some("rate_limit_event") => rate_limits(st, &ev),
        _ => {}
    }
}

// A staged message reached the model when its command started, until it ended; nothing counts past the first result.
fn lifecycle(st: &mut StreamState, ev: &Value) {
    let Some(id) = ev.get("command_uuid").and_then(Value::as_str) else { return };
    if st.result.is_some() {
        return;
    }
    match ev.get("state").and_then(Value::as_str) {
        Some("started") if !st.staged.iter().chain(&st.taken).any(|s| s == id) => st.staged.push(id.to_owned()),
        Some("completed" | "cancelled" | "discarded" | "refused") => st.staged.retain(|s| s != id),
        _ => {}
    }
}

// The model's own answer on the main thread reads what reached it before (fac-389); a synthetic one is no answer.
fn answered(st: &mut StreamState, ev: &Value) {
    let main = ev.get("parent_tool_use_id").is_none_or(Value::is_null);
    let synthetic = ev.pointer("/message/model").and_then(Value::as_str) == Some("<synthetic>");
    st.answered |= main && !synthetic;
    if st.result.is_none() && main && !synthetic {
        st.taken.append(&mut st.staged);
        turn::read(st);
    }
}

/// The result event (1.4.8, Claude 6): `is_error` is the turn's error; otherwise `structured_output` is
/// the reply, and with none its words, a string that doesn't parse, which the checker refuses.
pub fn result_of(ev: &Value) -> Result<Value, String> {
    let text = ev.get("result").and_then(Value::as_str).unwrap_or("").to_owned();
    if ev.get("is_error").and_then(Value::as_bool) == Some(true) {
        let errors: Vec<&str> = ev
            .get("errors")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        return Err(match (text.is_empty(), errors.is_empty()) {
            (false, _) => text,
            (true, false) => errors.join("; "),
            (true, true) => "claude returned an error".into(),
        });
    }
    match ev.get("structured_output") {
        Some(v) if !v.is_null() => Ok(v.clone()),
        _ => Ok(Value::String(format!("claude returned no structured output: {text}"))),
    }
}

// Steps once per `tool_use` id (1.4.8, Claude 8); text, and thinking only where the block holds
// some, once per message and words, waiting for the next step of their thread (fac-96).
fn steps_of(st: &mut StreamState, ev: &Value) {
    let thread = ev.get("parent_tool_use_id").and_then(Value::as_str);
    let msg = ev.pointer("/message/id").and_then(Value::as_str).unwrap_or("");
    let Some(blocks) = ev.pointer("/message/content").and_then(Value::as_array) else { return };
    for b in blocks {
        let said = |k: &str| b.get(k).and_then(Value::as_str).unwrap_or("");
        match b.get("type").and_then(Value::as_str) {
            Some("text") => turn::detail(st, &format!("{msg}\0text\0{}", said("text")), thread, Text, said("text")),
            Some("thinking") => {
                let key = format!("{msg}\0thinking\0{}", said("thinking"));
                turn::detail(st, &key, thread, Thinking, said("thinking"));
            }
            Some("tool_use") => {
                planner_call(st, b);
                tool_step(st, thread, b);
            }
            _ => {}
        }
    }
}

// Claude's planner (fac-128): its calls change the agent's list once their results say they held,
// a subagent's too, which shares the list.
fn planner_call(st: &mut StreamState, b: &Value) {
    let (Some(id), Some(name)) = (b.get("id").and_then(Value::as_str), b.get("name").and_then(Value::as_str)) else {
        return;
    };
    if ["TaskCreate", "TaskUpdate", "TaskList"].contains(&name) {
        let input = b.get("input").cloned().unwrap_or(Value::Null);
        st.planner_calls.insert(id.to_owned(), (name.to_owned(), input));
    }
}

// A tool's result ends its step (fac-206).
fn results_of(st: &mut StreamState, ev: &Value) {
    let Some(blocks) = ev.pointer("/message/content").and_then(Value::as_array) else { return };
    for b in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
        if let Some(id) = b.get("tool_use_id").and_then(Value::as_str) {
            turn::ended(st, id);
        }
    }
}

fn planner_results(st: &mut StreamState, ev: &Value) {
    let Some(blocks) = ev.pointer("/message/content").and_then(Value::as_array) else { return };
    for b in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
        let call = b.get("tool_use_id").and_then(Value::as_str).and_then(|id| st.planner_calls.get(id)).cloned();
        let Some((name, input)) = call else { continue };
        let failed = ev.pointer("/tool_use_result/success").and_then(Value::as_bool) == Some(false);
        if failed || b.get("is_error").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        match name.as_str() {
            "TaskCreate" => created(st, ev, b, &input),
            "TaskUpdate" => updated(st, &input),
            _ => listed(st, ev),
        }
    }
}

fn words_of(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn status_of(v: &Value) -> Option<StepStatus> {
    match v.get("status").and_then(Value::as_str)? {
        "pending" => Some(StepStatus::Pending),
        "in_progress" => Some(StepStatus::InProgress),
        "completed" => Some(StepStatus::Completed),
        _ => None,
    }
}

// A main-thread result names the new step's id; a subagent's only says it (`Task #6 created …`).
fn created(st: &mut StreamState, ev: &Value, b: &Value, input: &Value) {
    let said = b.get("content").and_then(Value::as_str).and_then(|t| t.strip_prefix("Task #"));
    let said = said.map(|t| t.chars().take_while(char::is_ascii_digit).collect::<String>());
    let id = ev.pointer("/tool_use_result/task/id").and_then(Value::as_str).map(str::to_owned).or(said);
    let (Some(id), Some(words)) = (id.filter(|i| !i.is_empty()), words_of(input, "subject")) else { return };
    if !st.list.iter().any(|s| s.id == id) {
        st.list.push(ListStep { id, words, status: StepStatus::Pending });
    }
}

fn updated(st: &mut StreamState, input: &Value) {
    let Some(id) = words_of(input, "taskId") else { return };
    if input.get("status").and_then(Value::as_str) == Some("deleted") {
        st.list.retain(|s| s.id != id);
        return;
    }
    let Some(step) = st.list.iter_mut().find(|s| s.id == id) else { return };
    if let Some(w) = words_of(input, "subject") {
        step.words = w;
    }
    if let Some(s) = status_of(input) {
        step.status = s;
    }
}

// TaskList says the whole list, a resumed session's earlier turns' steps among them: of those this
// turn never saw, only the open ones show.
fn listed(st: &mut StreamState, ev: &Value) {
    let Some(tasks) = ev.pointer("/tool_use_result/tasks").and_then(Value::as_array) else { return };
    let mut list = Vec::new();
    for t in tasks {
        let (Some(id), Some(words), Some(status)) = (words_of(t, "id"), words_of(t, "subject"), status_of(t)) else {
            continue;
        };
        if status != StepStatus::Completed || st.list.iter().any(|s| s.id == id) {
            list.push(ListStep { id, words, status });
        }
    }
    st.list = list;
}

fn tool_step(st: &mut StreamState, thread: Option<&str>, b: &Value) {
    let (Some(id), Some(name)) = (b.get("id").and_then(Value::as_str), b.get("name").and_then(Value::as_str)) else {
        return;
    };
    if name == "StructuredOutput" || st.steps.iter().any(|s| s.id == id) {
        return;
    }
    let input = b.get("input").cloned().unwrap_or(Value::Null);
    let said = input.get("command").and_then(Value::as_str).filter(|_| name == "Bash");
    let said = said.and_then(|c| super::said::words(st.seat.as_deref(), c));
    let words = said.unwrap_or_else(|| tool_words(name, &input, &st.folder, &st.home));
    let words = words.lines().next().unwrap_or("").trim().to_owned();
    let words = if thread.is_some() { format!("↳ {words}") } else { words };
    turn::step(st, thread, id, words.chars().take(100).collect());
}

// The words Andrei approved for each tool (fac-206); a tool they don't name, or one missing what
// they say of it, is its name.
fn tool_words(name: &str, input: &Value, folder: &str, home: &str) -> String {
    let field = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let path = || field("file_path").or_else(|| field("notebook_path")).map(|p| path_words(p, folder, home));
    let words = match name {
        "Bash" => field("description").or_else(|| field("command")).map(str::to_owned),
        "Read" => path().map(|p| format!("Read {p}")),
        "Write" => path().map(|p| format!("Wrote {p}")),
        "Edit" | "MultiEdit" | "NotebookEdit" => path().map(|p| format!("Changed {p}")),
        "Grep" => field("pattern").map(|p| format!("Searched for `{p}`")),
        "Glob" => field("pattern").map(|p| format!("Listed files matching `{p}`")),
        "WebFetch" => field("url").map(|u| format!("Read a page on {}", host(u))),
        "WebSearch" => field("query").map(|q| format!("Searched the web for {q}")),
        "Agent" | "Task" => field("description").map(|d| format!("Started a subagent: {d}")),
        _ => None,
    };
    words.unwrap_or_else(|| name.to_owned())
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// A file tool's path in words: relative inside the folder, `~` for the home, and the last 59
/// characters after `…` when longer than 60 (1.4.8, Claude 8).
pub fn path_words(path: &str, folder: &str, home: &str) -> String {
    let inside = |root: &str| {
        let root = root.trim_end_matches('/');
        (!root.is_empty()).then(|| path.strip_prefix(root).and_then(|r| r.strip_prefix('/'))).flatten()
    };
    let short = if let Some(r) = inside(folder) {
        r.to_owned()
    } else if let Some(r) = inside(home) {
        format!("~/{r}")
    } else {
        path.to_owned()
    };
    let n = short.chars().count();
    if n > 60 {
        let tail: String = short.chars().skip(n - 59).collect();
        format!("…{tail}")
    } else {
        short
    }
}

fn count(u: &Value, k: &str) -> Option<u64> {
    u.get(k).and_then(Value::as_u64)
}

fn prompt_of(u: &Value) -> Option<u64> {
    Some(count(u, "input_tokens")? + count(u, "cache_creation_input_tokens")? + count(u, "cache_read_input_tokens")?)
}

// A main-thread call's usage is what the window holds now (1.4.8, Claude 9).
fn context_of(st: &mut StreamState, ev: &Value) {
    if ev.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
        return;
    }
    let Some(msg) = ev.get("message") else { return };
    let Some(u) = msg.get("usage") else { return };
    let (Some(prompt), Some(out)) = (prompt_of(u), count(u, "output_tokens")) else { return };
    let model = msg.get("model").and_then(Value::as_str).map(str::to_owned);
    let id = msg.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
    let used = prompt + out;
    if st.last_call.as_ref().is_some_and(|(i, u, _)| *i == id && *u == used) {
        return; // the same call's count, repeated per content block
    }
    st.last_call = Some((id, used, prompt));
    let window = model.as_ref().and_then(|m| st.windows.get(m)).copied();
    st.context = Some(ContextReport { used, window, model });
}

// The result: its sums are never the context.
fn correct_context(st: &mut StreamState, ev: &Value) {
    if let Some(models) = ev.get("modelUsage").and_then(Value::as_object) {
        for (m, u) in models {
            if let Some(w) = u.get("contextWindow").and_then(Value::as_u64) {
                st.windows.insert(m.clone(), w);
            }
        }
    }
    let last = ev.pointer("/usage/iterations").and_then(Value::as_array).and_then(|a| a.last());
    if let (Some(it), Some((_, _, prompt))) = (last, st.last_call.clone())
        && prompt_of(it) == Some(prompt)
        && let (Some(out), Some(ctx)) = (count(it, "output_tokens"), st.context.as_mut())
    {
        ctx.used = prompt + out;
    }
    if let Some(ctx) = st.context.as_mut() {
        ctx.window = ctx.model.as_ref().and_then(|m| st.windows.get(m)).copied();
    }
}

fn rate_limits(st: &mut StreamState, ev: &Value) {
    let Some(info) = ev.get("rate_limit_info") else { return };
    let rejected = info.get("status").and_then(Value::as_str) == Some("rejected");
    let named = info.get("rateLimitType").and_then(Value::as_str);
    let mut found = limits::claude_windows(info);
    if rejected {
        let reset = found.iter().find(|w| Some(w.window.as_str()) == named).and_then(|w| w.resets_at);
        st.limit_rejected = Some(info.get("resetsAt").and_then(Value::as_i64).or(reset));
    }
    for mut w in found.drain(..) {
        w.account = st.account.clone();
        w.seen_ms = st.at;
        match st.limits.iter_mut().find(|x| x.bucket == w.bucket && x.window == w.window) {
            Some(x) => *x = w,
            None => st.limits.push(w),
        }
    }
}

/// Claude Code's attribution off: no commit trailer, no PR line, no session link (1.4.8.3); its
/// auto memory off: a seat keeps no notes of its own (fac-25).
pub const SETTINGS: &str = r#"{"attribution":{"commit":"","pr":"","sessionUrl":false},"autoMemoryEnabled":false}"#;

/// The folders a turn's Claude is given beyond its own.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dirs<'a> {
    /// The turn's images, when it has some.
    pub images: Option<&'a str>,
    /// The running build's skills, loaded as the `factory` plugin (fac-88), versioned with the build.
    pub skills: Option<&'a str>,
}

/// `claude -p …` exactly as 1.4.8 gives it. `resume` resumes the session; else it starts
/// under `--session-id`. Its prompt, and the messages delivered while it works, come as stream-json (fac-389).
pub fn argv(
    seat: &Seat,
    session: &str,
    resume: bool,
    schema: &Value,
    dirs: Dirs<'_>,
    style: Option<&str>,
    context_cap: Option<u64>,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["claude".into(), "-p".into()];
    a.push(if resume { "--resume".into() } else { "--session-id".into() });
    a.push(session.to_owned());
    a.extend(["--model".into(), seat.model.clone(), "--effort".into(), seat.effort.as_str().to_owned()]);
    if let Some(cap) = context_cap {
        a.extend(["--autocompact".into(), cap.to_string()]);
    }
    a.extend(["--output-format".into(), "stream-json".into(), "--verbose".into()]);
    a.extend(["--input-format".into(), "stream-json".into()]);
    a.extend(["--json-schema".into(), schema.to_string()]);
    a.extend(["--permission-mode".into(), "bypassPermissions".into()]);
    // Every setting source loads: `project` is what reads the clone's AGENTS.md.
    a.push("--strict-mcp-config".into());
    // A block list, not an allow-list: Claude drops an allowed name it no longer knows without a
    // word, as TodoWrite was, and the seat loses the tool (fac-127).
    match &seat.tools {
        Some(tools) => a.extend(["--tools".into(), tools.join(",")]),
        None => a.extend(["--disallowedTools".into(), CLAUDE_BLOCKED.join(",")]),
    }
    if let Some(d) = dirs.images {
        a.extend(["--add-dir".into(), d.to_owned()]);
    }
    if let Some(d) = dirs.skills {
        a.extend(["--plugin-dir".into(), d.to_owned()]);
    }
    a.extend(["--settings".into(), settings(seat)]);
    // A planner's or foreman's output style (fac-596); a resumed session keeps its start's prompt until it compacts,
    // Codex's too.
    if let Some(text) = style {
        a.extend(["--append-system-prompt".into(), text.to_owned()]);
    }
    a
}

/// What an unwalled seat never reads (fac-25): the `Read` rules cover Read, Grep, Glob and Bash's own
/// reads, the `Bash` ones refuse any command naming them; neither binds a command that hides the name.
pub const DENY: [&str; 4] = ["Read(//**/ACCESS.md)", "Read(//**/.ssh/**)", "Bash(*ACCESS.md*)", "Bash(*.ssh*)"];

/// The turn's settings: the Agent hook (fac-403), `advisorModel` when named (fac-5), and an
/// unwalled seat's `permissions.deny` (fac-25), alongside `SETTINGS`.
pub fn settings(seat: &Seat) -> String {
    let unwalled = seat.wall == factory_types::config::Wall::None;
    let mut v: Value = serde_json::from_str(SETTINGS).unwrap_or_else(|_| serde_json::json!({}));
    let bin = std::env::current_exe().unwrap_or_default();
    let bin = bin.to_string_lossy().replace('\'', "'\\''");
    let flag = if seat.model.to_ascii_lowercase().contains("fable") { " --seat-fable" } else { "" };
    let command = format!("'{bin}' claude-agent-hook{flag}");
    v["hooks"] = serde_json::json!({"PreToolUse": [{
        "matcher": "Agent", "hooks": [{"type": "command", "command": command}]
    }]});
    if let Some(model) = &seat.advisor {
        v["advisorModel"] = Value::String(model.clone());
    }
    if unwalled {
        v["permissions"] = serde_json::json!({"deny": DENY});
    }
    v.to_string()
}
