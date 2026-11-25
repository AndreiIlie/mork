//! Codex's app-server stream (fac-389): a walled turn's notifications, read as its exec stream is. The reply is the
//! turn's last agent message; a message steered in was read if it reached the turn before that message.

use factory_types::seen::DetailKind;
use serde_json::Value;

use super::{StreamState, codex, turn};

/// Reads one complete line of the stream into the state. Reading a line twice changes nothing.
pub fn read_line(st: &mut StreamState, line: &str) {
    let Ok(ev) = serde_json::from_str::<Value>(line) else { return };
    let params = ev.get("params").unwrap_or(&Value::Null);
    if ev.get("method").and_then(Value::as_str) == Some("thread/tokenUsage/updated") {
        super::usage::codex_app_server(st, params);
        return;
    }
    // A child's usage belongs to this seat; its reply and lifecycle remain its own (fac-526).
    if let (Some(root), Some(thread)) = (&st.thread, params.get("threadId").and_then(Value::as_str))
        && root != thread
    {
        return;
    }
    match ev.get("method").and_then(Value::as_str) {
        Some("turn/started") => turn::started(st),
        Some("item/started") => item(st, params, false),
        Some("item/completed") => item(st, params, true),
        Some("turn/completed") => completed(st, params.get("turn").unwrap_or(&Value::Null)),
        // A retried error isn't the turn's end.
        Some("error") if params.get("willRetry").and_then(Value::as_bool) != Some(true) => {
            failed(st, codex::error_words(params));
        }
        Some(_) => {}
        None => answered(st, &ev),
    }
}

// The thread's opening answers with its id; a refused thread or turn ends it.
fn answered(st: &mut StreamState, ev: &Value) {
    let id = ev.get("id").and_then(Value::as_u64);
    if !matches!(id, Some(2 | 3)) {
        return;
    }
    if ev.get("error").is_some() {
        failed(st, codex::error_words(ev));
        return;
    }
    if let Some(t) = ev.pointer("/result/thread/id").and_then(Value::as_str) {
        st.session_started = true;
        turn::started(st);
        st.thread = Some(t.to_owned());
    }
}

fn failed(st: &mut StreamState, words: String) {
    turn::finished(st);
    if !matches!(st.result, Some(Err(_))) {
        st.result = Some(Err(words));
    }
}

fn completed(st: &mut StreamState, t: &Value) {
    if st.result.is_some() {
        return;
    }
    turn::finished(st);
    st.result = Some(match t.get("status").and_then(Value::as_str) {
        Some("completed") => match &st.last_text {
            None => Err("codex wrote no last message".into()),
            Some(text) => Ok(serde_json::from_str(text)
                .unwrap_or_else(|e| Value::String(format!("codex's last message isn't JSON: {e}")))),
        },
        Some("interrupted") => Err("codex's turn was interrupted".into()),
        _ => Err(t.get("error").map_or_else(|| "codex failed".into(), codex::error_words)),
    });
    if !matches!(st.result, Some(Ok(_))) {
        st.taken.clear();
    }
}

// As the exec stream's items, named in camelCase; a delivered message's own item says it reached the turn.
const TOOL_ITEMS: [&str; 6] =
    ["commandExecution", "fileChange", "webSearch", "mcpToolCall", "dynamicToolCall", "collabAgentToolCall"];

fn item(st: &mut StreamState, params: &Value, completed: bool) {
    let Some(it) = params.get("item") else { return };
    let field = |k: &str| it.get(k).and_then(Value::as_str).unwrap_or("");
    let kind = field("type");
    // Only the turn's model writes these items in its own thread (fac-620); a compaction counts once it completed.
    st.answered |= matches!(kind, "agentMessage" | "reasoning") || TOOL_ITEMS.contains(&kind);
    st.compacted |= completed && kind == "contextCompaction";
    match kind {
        "userMessage" => {
            if let Some(id) = it.get("clientId").and_then(Value::as_str)
                && !st.staged.iter().any(|s| s == id)
            {
                st.staged.push(id.to_owned());
                let step = st.steps.iter().filter(|s| s.thread.is_none()).count();
                st.read_at.entry(id.to_owned()).or_insert(step);
            }
        }
        "agentMessage" if completed => {
            let text = field("text").trim();
            st.last_text = Some(text.to_owned());
            // Whatever reached the turn before its latest message was read by it.
            st.taken.clone_from(&st.staged);
            turn::read(st);
            if serde_json::from_str::<Value>(text).is_ok_and(|v| v.is_object()) {
                return;
            }
            turn::detail(st, &format!("{}\0text\0{text}", field("id")), None, DetailKind::Text, text);
            codex::commentary(st, text.lines().next().unwrap_or("").trim());
        }
        "reasoning" if completed => {
            let parts = ["summary", "content"].into_iter().filter_map(|k| it.get(k).and_then(Value::as_array));
            let text: Vec<&str> = parts.flatten().filter_map(Value::as_str).collect();
            let text = text.join("\n");
            turn::detail(st, &format!("{}\0thinking\0{text}", field("id")), None, DetailKind::Thinking, &text);
            if let Some(title) = text.split("**").nth(1).filter(|t| !t.trim().is_empty()) {
                codex::commentary(st, title.trim());
            }
        }
        t if TOOL_ITEMS.contains(&t) => codex::tool_item(st, it, completed),
        _ => {}
    }
}
