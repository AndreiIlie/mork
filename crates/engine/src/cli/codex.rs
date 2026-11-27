//! Codex's stream (`codex exec --json`), F-X1 to F-X4, and its `-o` last message.

use mork_types::config::Seat;
use mork_types::seen::DetailKind;
use serde_json::Value;

use super::{StreamState, turn};

/// Reads one complete line of the stream into the state. A `turn.completed` only marks the
/// end; the reply is read from `last-message.json` by [`finish`].
pub fn read_line(st: &mut StreamState, line: &str) {
    let Ok(ev) = serde_json::from_str::<Value>(line) else { return };
    event(st, &ev);
}

fn event(st: &mut StreamState, ev: &Value) {
    match ev.get("type").and_then(Value::as_str) {
        Some("thread.started") => {
            st.session_started = true;
            turn::started(st);
            if let Some(t) = ev.get("thread_id").and_then(Value::as_str) {
                st.thread = Some(t.to_owned());
            }
        }
        Some("turn.started") => turn::started(st),
        Some("turn.failed") | Some("error") => {
            turn::finished(st);
            let msg = error_words(ev);
            if !matches!(st.result, Some(Err(_))) {
                st.result = Some(Err(msg));
            }
        }
        Some("turn.completed") => {
            super::usage::codex_exec(st, ev);
            if st.result.is_none() {
                turn::finished(st);
                st.result = Some(Ok(Value::Null));
            }
        }
        Some("item.started") | Some("item.completed") => item(st, ev),
        _ => {}
    }
}

// Once per item id; commentary goes with the next step (1.4.8, Codex 4), and so does its whole
// text, and the reasoning's (fac-96). An id is the turn's own: the next turn counts again.
fn item(st: &mut StreamState, ev: &Value) {
    let Some(it) = ev.get("item") else { return };
    let completed = ev.get("type").and_then(Value::as_str) == Some("item.completed");
    let field = |k: &str| it.get(k).and_then(Value::as_str).unwrap_or("");
    match field("type") {
        "agent_message" if completed => {
            let text = field("text").trim();
            // The final message, the JSON reply, is not commentary.
            if serde_json::from_str::<Value>(text).is_ok_and(|v| v.is_object()) {
                return;
            }
            turn::detail(st, &format!("{}\0text\0{text}", field("id")), None, DetailKind::Text, text);
            commentary(st, text.lines().next().unwrap_or("").trim());
        }
        "reasoning" if completed => {
            let text = field("text");
            turn::detail(st, &format!("{}\0thinking\0{text}", field("id")), None, DetailKind::Thinking, text);
            if let Some(title) = text.split("**").nth(1).filter(|t| !t.trim().is_empty()) {
                commentary(st, title.trim());
            }
        }
        "command_execution" | "file_change" | "web_search" | "mcp_tool_call" | "collab_tool_call" => {
            tool_item(st, it, completed);
        }
        _ => {}
    }
}

/// A tool's item, the exec stream's or the app-server's: one step, ended once it completed.
pub(super) fn tool_item(st: &mut StreamState, it: &Value, completed: bool) {
    let id = it.get("id").and_then(Value::as_str).unwrap_or("");
    if id.is_empty() {
        return;
    }
    let is_command = matches!(it["type"].as_str(), Some("command_execution" | "commandExecution"));
    let raw = it.get("command").and_then(Value::as_str).filter(|c| !c.trim().is_empty());
    let (words, command) = match raw {
        Some(c) => super::command::display(st.seat.as_deref(), c, None),
        None => (step_words(it, &st.folder, &st.home), None),
    };
    let words: String = words.chars().take(100).collect();
    // An item seen only complete starts and ends at once (fac-206).
    match st.steps.iter_mut().find(|s| s.id == id) {
        // Its completion may say what its start didn't, as a search's query.
        Some(s) if completed && (raw.is_some() || !is_command) => {
            s.words = words;
            s.command = command;
        }
        Some(_) => {}
        None => {
            // Commentary says what the seat is doing until the next step.
            st.doing = None;
            let tool = it["tool"].as_str().or_else(|| it["type"].as_str()).unwrap_or("tool");
            turn::step(st, None, id, tool, words, command);
        }
    }
    if completed {
        turn::ended(st, id, tool_failed(it));
    }
}

fn tool_failed(item: &Value) -> bool {
    let exit = item.get("exit_code").or_else(|| item.get("exitCode")).and_then(Value::as_i64);
    matches!(item["status"].as_str(), Some("failed" | "declined"))
        || exit.is_some_and(|code| code != 0)
        || !item["error"].is_null()
        || item["success"].as_bool() == Some(false)
        || item.pointer("/result/isError").and_then(Value::as_bool) == Some(true)
}

/// What the seat says it is doing, until its next step.
pub(super) fn commentary(st: &mut StreamState, line: &str) {
    if !line.is_empty() {
        st.doing = Some(line.to_owned());
    }
}

// A tool item can arrive without a command. Its event fields then supply the step label.
fn step_words(it: &Value, folder: &str, home: &str) -> String {
    let field = |k: &str| it.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let kind = field("type").unwrap_or("");
    let words = match kind {
        "file_change" | "fileChange" => {
            let changes = it.get("changes").and_then(Value::as_array).into_iter().flatten();
            let paths: Vec<String> = changes
                .filter_map(|x| x.get("path").and_then(Value::as_str))
                .map(|p| super::claude::path_words(p, folder, home))
                .collect();
            (!paths.is_empty()).then(|| format!("Changed {}", paths.join(", ")))
        }
        "web_search" | "webSearch" => field("query").map(|q| format!("Searched the web for {q}")),
        "mcp_tool_call" | "mcpToolCall" => {
            field("tool").map(|t| field("server").map_or_else(|| t.to_owned(), |s| format!("{s}.{t}")))
        }
        _ => field("tool").map(str::to_owned),
    };
    words.unwrap_or_else(|| kind.to_owned())
}

/// The words of an error event, or of its `error`.
pub(super) fn error_words(ev: &Value) -> String {
    ev.get("error")
        .and_then(|e| e.get("message").and_then(Value::as_str).or_else(|| e.as_str()))
        .or_else(|| ev.get("message").and_then(Value::as_str))
        .unwrap_or("codex failed")
        .to_owned()
}

/// Turns a completed turn into its reply from `last-message.json` (1.4.8, Codex 3): a missing
/// file is an error; non-JSON is a reply that doesn't parse, which the checker refuses.
pub fn finish(st: &mut StreamState, last_message: Option<&str>) {
    if !matches!(st.result, Some(Ok(Value::Null))) {
        return;
    }
    st.result = Some(match last_message {
        None => Err("codex wrote no last message".into()),
        Some(text) => Ok(serde_json::from_str(text)
            .unwrap_or_else(|e| Value::String(format!("codex's last message isn't JSON: {e}")))),
    });
}

/// `codex exec …` exactly as 1.4.8 gives it. `thread` resumes it. `mcp_off` names the project
/// MCP servers to switch off; `images` are the turn's images.
pub fn argv(
    seat: &Seat,
    thread: Option<&str>,
    io: &str,
    mcp_off: &[String],
    images: &[String],
    style: Option<&str>,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["codex".into()];
    if seat.web {
        a.push("--search".into());
    }
    a.push("exec".into());
    if let Some(t) = thread {
        a.extend(["resume".into(), t.to_owned()]);
    }
    a.extend([
        "-m".into(),
        seat.model.clone(),
        "-c".into(),
        format!("model_reasoning_effort={}", seat.effort.as_str()),
    ]);
    a.extend(settings());
    if !seat.web {
        a.extend(["-c".into(), "web_search=\"disabled\"".into()]);
    }
    a.push("--ignore-user-config".into());
    a.extend(mcp_settings(mcp_off));
    a.extend(style_settings(style));
    a.extend(["--skip-git-repo-check".into(), "--json".into()]);
    a.extend(["--output-schema".into(), format!("{io}/schema.json"), "-o".into(), format!("{io}/last-message.json")]);
    a.extend(images.iter().map(|i| format!("--image={i}")));
    a.push("-".into());
    a
}

/// `codex app-server` for a walled turn (fac-389): `exec`'s settings, its user config blanked by the wall. Its
/// thread, prompt, images and schema are spoken on its stdin, and the messages delivered steer its turn.
pub fn app_server_argv(seat: &Seat, mcp_off: &[String], style: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = vec!["codex".into(), "-c".into(), format!("model={}", toml_string(&seat.model))];
    a.extend(["-c".into(), format!("model_reasoning_effort={}", seat.effort.as_str())]);
    a.extend(settings());
    let search = if seat.web { "live" } else { "disabled" };
    a.extend(["-c".into(), format!("web_search=\"{search}\"")]);
    a.extend(mcp_settings(mcp_off));
    a.extend(style_settings(style));
    a.push("app-server".into());
    a
}

// Full access inside the wall, nothing waiting on a person (fac-127), and unused built-in instructions and
// features turned off so turns keep a lean prompt without tool noise (fac-692).
fn settings() -> Vec<String> {
    [
        "sandbox_mode=danger-full-access",
        "approval_policy=never",
        "allow_login_shell=false",
        "features.apps=false",
        "features.default_mode_request_user_input=false",
        "include_permissions_instructions=false",
        "include_apps_instructions=false",
        "include_collaboration_mode_instructions=false",
        "features.plugins=false",
        "agents.enabled=false",
        "features.multi_agent=false",
        "features.multi_agent_v2=false",
    ]
    .into_iter()
    .flat_map(|c| ["-c".to_owned(), c.to_owned()])
    .collect()
}

fn mcp_settings(mcp_off: &[String]) -> Vec<String> {
    mcp_off.iter().flat_map(|name| ["-c".into(), format!("mcp_servers.{}.enabled=false", quote_key(name))]).collect()
}

// A planner's or foreman's output style as its developer instructions (fac-596).
fn style_settings(style: Option<&str>) -> Vec<String> {
    style
        .map(|text| ["-c".into(), format!("developer_instructions={}", toml_string(text))])
        .into_iter()
        .flatten()
        .collect()
}

// A TOML basic string: one line, its control characters escaped.
fn toml_string(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A TOML key, quoted when it holds more than letters, digits, `_` and `-` (1.4.8, Codex 2).
pub fn quote_key(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        name.to_owned()
    } else {
        toml_string(name)
    }
}

/// The MCP servers a project-level `.codex/config.toml` declares, in the folders from `cwd`
/// up, skipping the users' own (`user_codex_homes`: the operator's and the seat's), sorted (1.4.8, Codex 2).
pub fn project_mcp_servers(cwd: &std::path::Path, user_codex_homes: &[&std::path::Path]) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for dir in cwd.ancestors() {
        let codex = dir.join(".codex");
        if user_codex_homes.contains(&codex.as_path()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(codex.join("config.toml")) else { continue };
        let Ok(v) = text.parse::<toml::Table>() else { continue };
        if let Some(servers) = v.get("mcp_servers").and_then(toml::Value::as_table) {
            names.extend(servers.keys().cloned());
        }
    }
    names.into_iter().collect()
}
