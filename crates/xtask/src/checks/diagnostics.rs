use std::path::Path;

use serde_json::{Value, json};

fn finding_with(
    root: &Path,
    file: &str,
    line: u64,
    summary: &str,
    proof: &str,
    tool: &str,
    clamp_eof: bool,
) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let full = root.join(file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
    let rel = full.strip_prefix(&root).map_err(|_| format!("{file} is outside the clone"))?;
    let text = std::fs::read(&full).map_err(|e| format!("{file}: {e}"))?;
    let lines = text.split(|b| *b == b'\n').count() - usize::from(text.ends_with(b"\n"));
    let max = lines.max(1) as u64;
    let line = if clamp_eof && line == max.saturating_add(1) { max } else { line };
    if line == 0 || line > max {
        return Err(format!("{file}:{line} isn't a line in the clone"));
    }
    Ok(json!({"file": rel, "line": line, "summary": summary, "proof": proof, "tool": tool}))
}

pub(super) fn finding(
    root: &Path,
    file: &str,
    line: u64,
    summary: &str,
    proof: &str,
    tool: &str,
) -> Result<Value, String> {
    finding_with(root, file, line, summary, proof, tool, false)
}

// tsc reports EOF errors on line N+1 past a trailing newline; clamp that one line so the gate locates it.
pub(super) fn finding_eof(
    root: &Path,
    file: &str,
    line: u64,
    summary: &str,
    proof: &str,
    tool: &str,
) -> Result<Value, String> {
    finding_with(root, file, line, summary, proof, tool, true)
}

pub(super) fn rustfmt(root: &Path, text: &str) -> Result<Vec<Value>, String> {
    let lines: Vec<_> = text.lines().collect();
    let mut out = Vec::new();
    for (i, header) in lines.iter().enumerate() {
        let Some(place) = header.strip_prefix("Diff in ").and_then(|s| s.strip_suffix(':')) else { continue };
        let (file, line) = place.rsplit_once(':').ok_or_else(|| format!("bad rustfmt location: {header}"))?;
        let line = line.parse().map_err(|_| format!("bad rustfmt line: {header}"))?;
        let proof = lines[i..]
            .iter()
            .take(12)
            .take_while(|s| **s == *header || !s.starts_with("Diff in "))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        let found = finding(root, file, line, "rustfmt would change this file", &proof, "rustfmt")?;
        if !out.iter().any(|v: &Value| v["file"] == found["file"]) {
            out.push(found);
        }
    }
    Ok(out)
}

pub(super) fn clippy(root: &Path, text: &str) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).map_err(|e| format!("clippy JSON: {e}: {line}"))?;
        if v["reason"] != "compiler-message" {
            continue;
        }
        let m = &v["message"];
        if !matches!(m["level"].as_str(), Some("warning" | "error")) {
            continue;
        }
        let Some(span) = m["spans"].as_array().and_then(|ss| ss.iter().find(|s| s["is_primary"] == true)) else {
            continue;
        };
        let file = span["file_name"].as_str().ok_or("clippy span has no file")?;
        let number = span["line_start"].as_u64().ok_or("clippy span has no line")?;
        let summary = m["message"].as_str().ok_or("clippy message has no summary")?;
        let proof = m["rendered"].as_str().unwrap_or(summary).trim();
        out.push(finding(root, file, number, summary, proof, "clippy")?);
    }
    Ok(out)
}

pub(super) fn problem(root: &Path, text: &str, tool: &str) -> Result<Value, String> {
    let (first, rest) = text.split_once(':').unwrap_or((text, ""));
    if root.join(first).is_file() {
        let (line, summary) = match rest.trim_start().split_once(':') {
            Some((n, summary)) if n.parse::<u64>().is_ok() => (n.parse::<u64>().map_err(|e| e.to_string())?, summary),
            _ => (1, rest),
        };
        return finding(root, first, line, summary.trim(), text, tool);
    }
    if first == "crates/sys" {
        return finding(root, "crates/sys/Cargo.toml", 1, rest.trim(), text, tool);
    }
    if text.contains(" depends on ")
        || text.contains(" doesn't depend on ")
        || text.contains(" reaches ")
        || text.ends_with(": missing from the workspace")
    {
        let name = text.split([' ', ':']).next().unwrap_or_default();
        let dir = xtask::CRATES.iter().chain([&("xtask", "xtask")]).find(|(_, pkg)| *pkg == name);
        let manifest = dir
            .map(|(dir, _)| format!("crates/{dir}/Cargo.toml"))
            .filter(|p| root.join(p).is_file())
            .unwrap_or_else(|| "Cargo.toml".into());
        return finding(root, &manifest, 1, text, text, tool);
    }
    Err(format!("can't locate {tool} problem: {text}"))
}
