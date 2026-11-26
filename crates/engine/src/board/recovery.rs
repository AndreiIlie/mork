use mork_types::action::{Action, ActionWhat, BoardEdit};
use mork_types::work::Found;
use serde_json::{Value, json};
use std::path::Path;

fn unclear(words: impl Into<String>) -> Found {
    Found::Unclear { words: words.into() }
}

pub(super) fn found(b: &Path, action: &Action) -> Found {
    match &action.what {
        ActionWhat::CreateBoardCard { file, text, .. } if !file.is_empty() => created_as_decided(b, file, text),
        ActionWhat::CreateBoardCard { title, description, .. } => created(b, title, description),
        ActionWhat::ChangeBoardCard { file, before, after, dest, .. } if !after.is_empty() => {
            changed_as_decided(b, file, before, after, dest)
        }
        ActionWhat::ChangeBoardCard { file, edit, .. } => changed(b, file, edit, action.result.as_ref()),
        _ => unclear("not a board action"),
    }
}

// Other content is held, nothing written (2.5).
fn created_as_decided(b: &Path, file: &str, text: &str) -> Found {
    match std::fs::read_to_string(b.join(file)) {
        Ok(now) if now == text => Found::Done { result: json!({"id": super::writes::id_of(file), "file": file}) },
        Ok(_) => unclear(format!("{file} holds another card's text")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Found::NotDone,
        Err(e) => unclear(format!("{file}: {e}")),
    }
}

// Content neither before nor after the change is held (2.5).
fn changed_as_decided(b: &Path, file: &str, before: &str, after: &str, dest: &str) -> Found {
    let hash = |f: &str| std::fs::read_to_string(b.join(f)).ok().map(|t| mork_types::hash::short(&t));
    if hash(dest).as_deref() == Some(after) {
        return Found::Done { result: json!({"file": dest, "before": before, "after": after}) };
    }
    if hash(file).as_deref() == Some(before) {
        return Found::NotDone;
    }
    unclear(format!("{file} holds neither what it held before the change nor the change"))
}

fn created(b: &Path, title: &str, description: &str) -> Found {
    let found = std::fs::read_dir(b.join("tasks")).into_iter().flatten().flatten().find_map(|e| {
        let text = std::fs::read_to_string(e.path()).ok()?;
        let f = mork_core::board::parse(&text).ok()?;
        (f.title == title && f.description == description.trim())
            .then(|| (f.id, e.file_name().to_string_lossy().into_owned()))
    });
    match found {
        Some((id, name)) => Found::Done { result: json!({"id": id, "file": format!("tasks/{name}")}) },
        None => Found::NotDone,
    }
}

// A change already made shows its words in the file (its note, its fields); a closed card is
// where it was moved.
fn changed(b: &Path, file: &str, edit: &BoardEdit, result: Option<&Value>) -> Found {
    let name = std::path::Path::new(file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let text = std::fs::read_to_string(b.join(file)).unwrap_or_default();
    // What the file held when the change started, and what it writes (2.5.2's table).
    if let Some(p) = result.filter(|_| !matches!(edit, BoardEdit::Close { .. })) {
        let now = crate::files::sha256_hex(text.as_bytes());
        return if p.get("after").and_then(|v| v.as_str()) == Some(now.as_str()) {
            Found::Done { result: json!({"file": file}) }
        } else if p.get("before").and_then(|v| v.as_str()) == Some(now.as_str()) {
            Found::NotDone
        } else {
            unclear(format!("{file} holds neither what it held before the change nor the change"))
        };
    }
    let has = |s: &str| text.contains(s);
    let done = match edit {
        BoardEdit::Note { text: t, .. } => has(t),
        BoardEdit::Fields { title, status, .. } => {
            title.as_deref().is_none_or(has) && status.as_deref().is_none_or(|s| has(&format!("status: {s}")))
        }
        BoardEdit::Close { done, .. } => {
            let moved = if *done { b.join("completed").join(&name) } else { b.join("archive/tasks").join(&name) };
            moved.exists() && !b.join(file).exists()
        }
    };
    if done {
        return Found::Done { result: json!({"file": file}) };
    }
    if b.join(file).exists() { Found::NotDone } else { unclear(format!("{file} is gone")) }
}
