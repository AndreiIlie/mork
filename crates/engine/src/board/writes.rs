use mork_core::board::{self, writes::date};
use mork_types::action::{Action, ActionWhat, BoardEdit};
use mork_types::work::ActionFailure;
use serde_json::{Value, json};
use std::path::Path;

use super::{BoardWrite, Markdown};

fn failed(words: impl Into<String>) -> ActionFailure {
    ActionFailure::Failed { words: words.into() }
}

impl BoardWrite for Markdown {
    fn write(&self, action: &Action, now: i64) -> Result<Value, ActionFailure> {
        match &action.what {
            ActionWhat::CreateBoardCard { file, text, .. } if !file.is_empty() => write_new(&self.backlog, file, text),
            ActionWhat::CreateBoardCard { title, description, id, .. } => {
                create(&self.backlog, now, title, description, id.as_deref())
            }
            ActionWhat::ChangeBoardCard { file, before, after, text, dest, .. } if !after.is_empty() => {
                write_change(&self.backlog, file, &Change { before, after, text, dest })
            }
            ActionWhat::ChangeBoardCard { file, edit, .. } => {
                change(&self.backlog, file, edit, action.result.as_ref(), now)
            }
            _ => Err(failed("not a board action")),
        }
    }
}

/// Every id on a board: in `tasks/`, `completed/`, `archive/` and every folder under them, by
/// file name and by `id:` line (4.18.1).
pub fn ids(backlog: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for top in ["tasks", "completed", "archive"] {
        walk(&backlog.join(top), &mut out);
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            walk(&p, out);
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.split(" - ").next().filter(|_| name.ends_with(".md")) {
            out.push(id.to_owned());
        }
        if let Ok(f) =
            std::fs::read_to_string(&p).map(|t| board::parse(&t)).and_then(|r| r.map_err(std::io::Error::other))
        {
            out.push(f.id);
        }
    }
}

fn create(b: &Path, now: i64, title: &str, description: &str, asked: Option<&str>) -> Result<Value, ActionFailure> {
    let tasks = b.join("tasks");
    if std::fs::symlink_metadata(&tasks).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(failed("the board's tasks folder is a symlink: nothing written"));
    }
    let (prefix, _) =
        board::config(&std::fs::read_to_string(b.join("config.yml")).map_err(|e| failed(format!("config.yml: {e}")))?)
            .map_err(failed)?;
    let all = ids(b);
    let id = match asked {
        Some(a) => {
            let n =
                board::number(&prefix, a).ok_or_else(|| failed(format!("`{a}` isn't an id on the prefix {prefix}")))?;
            if all.iter().any(|x| board::number(&prefix, x) == Some(n) && x.to_lowercase() == a.to_lowercase()) {
                return Err(failed(format!("{a} is taken")));
            }
            a.to_uppercase()
        }
        None => board::next_id(&prefix, &all).to_uppercase(),
    };
    let (name, text) = board::new_card(&id, title, description, &date(now)).map_err(failed)?;
    let dest = tasks.join(&name);
    let tmp = tasks.join(format!(".{name}.mork-tmp"));
    std::fs::write(&tmp, &text).map_err(|e| failed(e.to_string()))?;
    let linked = std::fs::hard_link(&tmp, &dest);
    let _ = std::fs::remove_file(&tmp);
    linked.map_err(|e| failed(format!("{name}: {e}: nothing overwritten")))?;
    Ok(json!({"id": id, "file": format!("tasks/{name}")}))
}

/// The file's content after the edit, and where it goes, for a change the engine decides (a file the
/// boards' read doesn't hold): `core`'s own edit, with the done status as `config.yml` says.
pub(super) fn edited(
    now: i64,
    b: &Path,
    file: &str,
    before: &str,
    edit: &BoardEdit,
) -> Result<(String, String), ActionFailure> {
    let done_status = board::config(&std::fs::read_to_string(b.join("config.yml")).unwrap_or_default())
        .map(|(_, d)| d)
        .unwrap_or_default();
    // No record of the notes here: a description over a note isn't replaced on this path.
    mork_core::board::writes::edited(before, &done_status, file, edit, &[], now).map_err(failed)
}

// Refused by the content check before anything was replaced: what lets a note be retried (fac-318).
fn unwritten(words: String) -> ActionFailure {
    ActionFailure::BoardUnwritten { words }
}

struct Change<'a> {
    before: &'a str,
    after: &'a str,
    text: &'a str,
    dest: &'a str,
}

// Written whole, never over anything (4.18.1).
fn write_new(b: &Path, file: &str, text: &str) -> Result<Value, ActionFailure> {
    let tasks = b.join("tasks");
    if std::fs::symlink_metadata(&tasks).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(failed("the board's tasks folder is a symlink: nothing written"));
    }
    if file.split('/').any(|p| p == "..") || !file.starts_with("tasks/") {
        return Err(failed(format!("{file} isn't a new card's file")));
    }
    let dest = b.join(file);
    let tmp = dest.with_file_name(format!(".{}.mork-tmp", file.trim_start_matches("tasks/")));
    std::fs::write(&tmp, text).map_err(|e| failed(e.to_string()))?;
    let linked = std::fs::hard_link(&tmp, &dest);
    let _ = std::fs::remove_file(&tmp);
    linked.map_err(|e| failed(format!("{file}: {e}: nothing overwritten")))?;
    Ok(json!({"id": id_of(file), "file": file}))
}

/// A new card's id, from its file's name (`tasks/EX-12 - Title.md`).
pub(super) fn id_of(file: &str) -> String {
    file.rsplit('/').next().and_then(|n| n.split(" - ").next()).unwrap_or("").to_owned()
}

// Written only over the content it was made from (2.5.5).
fn write_change(b: &Path, file: &str, ch: &Change<'_>) -> Result<Value, ActionFailure> {
    if file.split('/').chain(ch.dest.split('/')).any(|p| p == "..") {
        return Err(failed(format!("{file} isn't a board file")));
    }
    let path = b.join(file);
    let now = std::fs::read_to_string(&path).map_err(|e| failed(format!("{file}: {e}")))?;
    if mork_types::hash::short(&now) != ch.before {
        return Err(unwritten(format!("{file} was changed since this change was made: nothing written")));
    }
    let tmp = path.with_file_name(format!(
        ".{}.mork-tmp",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    std::fs::write(&tmp, ch.text).map_err(|e| failed(e.to_string()))?;
    if std::fs::read_to_string(&path).ok().as_deref() != Some(now.as_str()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(unwritten(format!("{file} changed while it was being written: nothing written")));
    }
    put(b, &path, &tmp, file, ch.dest)?;
    Ok(json!({"file": ch.dest, "before": ch.before, "after": ch.after}))
}

fn put(b: &Path, path: &Path, tmp: &Path, file: &str, dest: &str) -> Result<(), ActionFailure> {
    if dest == file {
        return std::fs::rename(tmp, path).map_err(|e| failed(e.to_string()));
    }
    let to = b.join(dest);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| failed(e.to_string()))?;
    }
    let linked = std::fs::hard_link(tmp, &to);
    let _ = std::fs::remove_file(tmp);
    linked.map_err(|e| failed(format!("{dest}: {e}: nothing overwritten")))?;
    std::fs::remove_file(path).map_err(|e| failed(e.to_string()))
}

fn change(b: &Path, file: &str, edit: &BoardEdit, result: Option<&Value>, now: i64) -> Result<Value, ActionFailure> {
    if file.split('/').any(|p| p == "..") {
        return Err(failed(format!("{file} isn't a board file")));
    }
    let path = b.join(file);
    let before = std::fs::read_to_string(&path).map_err(|e| failed(format!("{file}: {e}")))?;
    // The file must hold what it held when the change was first made (2.5.5).
    if let Some(want) = result.and_then(|r| r.get("before")).and_then(Value::as_str)
        && crate::files::sha256_hex(before.as_bytes()) != want
    {
        return Err(unwritten(format!("{file} was changed since this change was made: nothing written")));
    }
    let (after, dest) = edited(now, b, file, &before, edit)?;
    let tmp = path.with_file_name(format!(
        ".{}.mork-tmp",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    std::fs::write(&tmp, &after).map_err(|e| failed(e.to_string()))?;
    // Written only over the content it was made from (2.5.5).
    if std::fs::read_to_string(&path).ok().as_deref() != Some(before.as_str()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(unwritten(format!("{file} changed while it was being written: nothing written")));
    }
    put(b, &path, &tmp, file, &dest)?;
    Ok(
        json!({"file": dest, "before": crate::files::sha256_hex(before.as_bytes()), "after": crate::files::sha256_hex(after.as_bytes())}),
    )
}
