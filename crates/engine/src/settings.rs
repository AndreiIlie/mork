//! Claude's settings file, `~/.claude/settings.json` (F-A3): its `env` block's telemetry
//! variables, passed into every wall explicitly (1.4.8, Claude 5); and the operator's output style (fac-596).

use std::collections::BTreeMap;
use std::path::Path;

/// The telemetry variables of a settings file: `OTEL_*` and `CLAUDE_CODE_ENABLE_TELEMETRY`.
pub fn telemetry(text: &str) -> BTreeMap<String, String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return BTreeMap::new() };
    let Some(env) = v.get("env").and_then(|e| e.as_object()) else { return BTreeMap::new() };
    env.iter()
        .filter(|(k, _)| k.starts_with("OTEL_") || *k == "CLAUDE_CODE_ENABLE_TELEMETRY")
        .filter_map(|(k, v)| {
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => return None,
            };
            Some((k.clone(), val))
        })
        .collect()
}

/// Reads them from the operator's home; none when the file is missing.
pub fn read_telemetry(home: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(home.join(".claude/settings.json")).map(|t| telemetry(&t)).unwrap_or_default()
}

/// The operator's output style, `~/.claude/output-styles/operator.md`, its front matter dropped: a planner's or foreman's
/// system prompt carries it (fac-596). Why it can't be read, when it can't.
pub fn read_style(home: &Path) -> Result<String, String> {
    let path = home.join(".claude/output-styles/operator.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let body = text.trim_start();
    let mut lines = body.split_inclusive('\n');
    let mut start = 0;
    if let Some(first) = lines.next().filter(|line| line.trim_end() == "---") {
        start = first.len();
        for line in lines {
            start += line.len();
            if line.trim_end() == "---" {
                break;
            }
        }
    }
    match body[start..].trim() {
        "" => Err(format!("{}: no text under its front matter", path.display())),
        b => Ok(b.to_owned()),
    }
}
