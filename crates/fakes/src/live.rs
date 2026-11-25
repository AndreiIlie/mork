//! The fakes' side of a live turn's stdin (fac-389): a first stream-json line, Codex's app-server opening.
//! Nothing after them is read: the messages delivered stay in the pipe, as no fake takes them.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// The turn every fake app-server runs.
pub const TURN: &str = "turn-1";

/// The prompt of a Claude or agy turn spoken as stream-json: its first line's content.
pub fn stream_prompt() -> String {
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    let v: Value = serde_json::from_str(&line).unwrap_or_default();
    text_of(v.pointer("/message/content"))
}

/// An app-server turn's opening, answered up to its `turn/start`: its thread, then its prompt.
pub fn codex_open() -> (String, String) {
    let mut thread = format!("thread-{}", std::process::id());
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let id = v.get("id").cloned().unwrap_or(Value::Null);
        match v.get("method").and_then(Value::as_str) {
            Some("initialize") => answer(&id, &json!({"userAgent": "fake-codex"})),
            Some("thread/start" | "thread/resume") => {
                if let Some(t) = v.pointer("/params/threadId").and_then(Value::as_str) {
                    t.clone_into(&mut thread);
                }
                answer(&id, &json!({"thread": {"id": thread}}));
            }
            Some("turn/start") => {
                answer(&id, &json!({"turn": {"id": TURN, "status": "inProgress"}}));
                return (thread, text_of(v.pointer("/params/input")));
            }
            _ => {}
        }
    }
    (thread, String::new())
}

/// A line of the app-server's stream.
pub fn notify(method: &str, params: &Value) {
    emit(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
}

fn answer(id: &Value, result: &Value) {
    emit(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

fn emit(v: &Value) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

// A content as a string, or its text parts joined.
fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => {
            parts.iter().filter_map(|p| p.get("text")?.as_str()).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    }
}
