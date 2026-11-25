// Board writes (4.18): a new card is hard-linked so it never overwrites; a change is written only over the
// content it was made from. Checked for leaks first (3.8); the engine never commits a board file.

use std::path::{Path, PathBuf};

use factory_types::action::{ActionWhat, BoardEdit};
use factory_types::work::ActionFailure;
use serde_json::{Value, json};

use super::act_run::{Call, failed};
use crate::board;

pub fn backlog(c: &Call, project: &str) -> Result<PathBuf, ActionFailure> {
    Ok(Path::new(&c.project(project)?.board).join("backlog"))
}

use factory_core::board::writes::date;

fn leak(c: &Call, text: &str, what: &str) -> Result<(), ActionFailure> {
    match c.secrets().hit(text) {
        Some(kind) => {
            Err(ActionFailure::Leak { words: format!("leak: this board write would publish {kind}, in {what}") })
        }
        None => Ok(()),
    }
}

/// The text a board write is checked for leaks in, before the write and when it is made.
pub fn leak_text(what: &ActionWhat) -> Option<String> {
    match what {
        ActionWhat::CreateBoardCard { file, text, .. } if !file.is_empty() => Some(text.clone()),
        ActionWhat::CreateBoardCard { title, description, .. } => Some(format!("{title}\n{description}")),
        ActionWhat::ChangeBoardCard { edit, .. } => Some(words_of(edit)),
        ActionWhat::Fetch { .. }
        | ActionWhat::Take { .. }
        | ActionWhat::TakeJoin { .. }
        | ActionWhat::Push { .. }
        | ActionWhat::BackupPush { .. }
        | ActionWhat::OpenPr { .. }
        | ActionWhat::Comment { .. }
        | ActionWhat::ReplyComment { .. }
        | ActionWhat::ResolveComment { .. }
        | ActionWhat::Merge { .. }
        | ActionWhat::MoveBase { .. }
        | ActionWhat::RemoveClone { .. }
        | ActionWhat::RemoveTrain { .. }
        | ActionWhat::Door(_) => None,
    }
}

pub fn run(c: &Call) -> Result<Value, ActionFailure> {
    let place = match &c.action.what {
        ActionWhat::ChangeBoardCard { file, .. } => file.clone(),
        _ => "the new card".to_owned(),
    };
    leak(c, &leak_text(&c.action.what).unwrap_or_default(), &place)?;
    match &c.action.what {
        ActionWhat::CreateBoardCard { project, file, text, .. } if !file.is_empty() => {
            write_new(c, project, file, text)
        }
        ActionWhat::CreateBoardCard { project, title, description, id, .. } => {
            create(c, project, title, description, id.as_deref())
        }
        ActionWhat::ChangeBoardCard { project, file, before, after, text, dest, .. } if !after.is_empty() => {
            write_change(c, project, file, &Change { before, after, text, dest })
        }
        ActionWhat::ChangeBoardCard { project, file, edit, .. } => change(c, project, file, edit),
        ActionWhat::Fetch { .. }
        | ActionWhat::Take { .. }
        | ActionWhat::TakeJoin { .. }
        | ActionWhat::Push { .. }
        | ActionWhat::BackupPush { .. }
        | ActionWhat::OpenPr { .. }
        | ActionWhat::Comment { .. }
        | ActionWhat::ReplyComment { .. }
        | ActionWhat::ResolveComment { .. }
        | ActionWhat::Merge { .. }
        | ActionWhat::MoveBase { .. }
        | ActionWhat::RemoveClone { .. }
        | ActionWhat::RemoveTrain { .. }
        | ActionWhat::Door(_) => Err(failed("not a board action")),
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

fn create(
    c: &Call,
    project: &str,
    title: &str,
    description: &str,
    asked: Option<&str>,
) -> Result<Value, ActionFailure> {
    let b = backlog(c, project)?;
    let tasks = b.join("tasks");
    if std::fs::symlink_metadata(&tasks).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(failed("the board's tasks folder is a symlink: nothing written"));
    }
    let (prefix, _) =
        board::config(&std::fs::read_to_string(b.join("config.yml")).map_err(|e| failed(format!("config.yml: {e}")))?)
            .map_err(failed)?;
    let all = ids(&b);
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
    let (name, text) = board::new_card(&id, title, description, &date(c.now)).map_err(failed)?;
    let dest = tasks.join(&name);
    let tmp = tasks.join(format!(".{name}.factory-tmp"));
    std::fs::write(&tmp, &text).map_err(|e| failed(e.to_string()))?;
    let linked = std::fs::hard_link(&tmp, &dest);
    let _ = std::fs::remove_file(&tmp);
    linked.map_err(|e| failed(format!("{name}: {e}: nothing overwritten")))?;
    Ok(json!({"id": id, "file": format!("tasks/{name}")}))
}

/// A board command's leak, before anything is recorded: the kind of secret, and the field
/// that holds it. [`write_leak`] checks what the write would hold, as the write does.
pub fn command_leak(secrets: &crate::secrets::Secrets, cmd: &factory_types::Command) -> Option<String> {
    let v = serde_json::to_value(cmd).ok()?;
    let fields = v.as_object()?.values().next()?.as_object()?;
    fields.iter().find_map(|(field, value)| {
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
            _ => return None,
        };
        secrets.hit(&text).map(|kind| format!("leak: this board write would publish {kind}, in `{field}`"))
    })
}

/// What the write of a decided action would publish, checked as the write checks it: the kind
/// of secret, and the command's fields that hold its pieces.
pub fn write_leak(
    secrets: &crate::secrets::Secrets,
    what: &ActionWhat,
    cmd: &factory_types::Command,
) -> Option<String> {
    let (kind, value) = secrets.hit_value(&leak_text(what)?)?;
    let v = serde_json::to_value(cmd).ok()?;
    let fields = v.as_object()?.values().next()?.as_object()?;
    let pieces: Vec<&str> = value.lines().map(str::trim).filter(|p| !p.is_empty()).collect();
    let holding: Vec<String> = fields
        .iter()
        .filter(|(_, f)| {
            let text = match f {
                Value::String(s) => s.clone(),
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
                _ => return false,
            };
            let t = text.trim();
            !t.is_empty() && (pieces.iter().any(|p| t.contains(p)) || value.contains(t))
        })
        .map(|(name, _)| format!("`{name}`"))
        .collect();
    let place = match holding.as_slice() {
        [] => "the card's text".to_owned(),
        [one] => one.clone(),
        many => format!("{} together", many.join(" and ")),
    };
    Some(format!("leak: this board write would publish {kind}, in {place}"))
}

fn words_of(edit: &BoardEdit) -> String {
    match edit {
        BoardEdit::Note { text, .. } => text.clone(),
        BoardEdit::Fields { title, status, priority, labels, parent, acceptance, description, note } => {
            [title, status, priority, parent, acceptance, description]
                .into_iter()
                .filter_map(Clone::clone)
                .chain(labels.clone().unwrap_or_default())
                .chain(note.as_ref().map(|n| n.text.clone()))
                .collect::<Vec<_>>()
                .join("\n")
        }
        BoardEdit::Close { note, .. } => note.clone(),
    }
}

/// The file's content after the edit, and where it goes, for a change the engine decides (a file the
/// boards' read doesn't hold): `core`'s own edit, with the done status as `config.yml` says.
pub fn edited(
    c: &Call,
    b: &Path,
    file: &str,
    before: &str,
    edit: &BoardEdit,
) -> Result<(String, String), ActionFailure> {
    let done_status = board::config(&std::fs::read_to_string(b.join("config.yml")).unwrap_or_default())
        .map(|(_, d)| d)
        .unwrap_or_default();
    // No record of the notes here: a description over a note isn't replaced on this path.
    factory_core::board::writes::edited(before, &done_status, file, edit, &[], c.now).map_err(failed)
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
fn write_new(c: &Call, project: &str, file: &str, text: &str) -> Result<Value, ActionFailure> {
    let b = backlog(c, project)?;
    let tasks = b.join("tasks");
    if std::fs::symlink_metadata(&tasks).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(failed("the board's tasks folder is a symlink: nothing written"));
    }
    if file.split('/').any(|p| p == "..") || !file.starts_with("tasks/") {
        return Err(failed(format!("{file} isn't a new card's file")));
    }
    let dest = b.join(file);
    let tmp = dest.with_file_name(format!(".{}.factory-tmp", file.trim_start_matches("tasks/")));
    std::fs::write(&tmp, text).map_err(|e| failed(e.to_string()))?;
    let linked = std::fs::hard_link(&tmp, &dest);
    let _ = std::fs::remove_file(&tmp);
    linked.map_err(|e| failed(format!("{file}: {e}: nothing overwritten")))?;
    Ok(json!({"id": id_of(file), "file": file}))
}

/// A new card's id, from its file's name (`tasks/EX-12 - Title.md`).
pub fn id_of(file: &str) -> String {
    file.rsplit('/').next().and_then(|n| n.split(" - ").next()).unwrap_or("").to_owned()
}

// Written only over the content it was made from (2.5.5).
fn write_change(c: &Call, project: &str, file: &str, ch: &Change<'_>) -> Result<Value, ActionFailure> {
    let b = backlog(c, project)?;
    if file.split('/').chain(ch.dest.split('/')).any(|p| p == "..") {
        return Err(failed(format!("{file} isn't a board file")));
    }
    let path = b.join(file);
    let now = std::fs::read_to_string(&path).map_err(|e| failed(format!("{file}: {e}")))?;
    if factory_types::hash::short(&now) != ch.before {
        return Err(unwritten(format!("{file} was changed since this change was made: nothing written")));
    }
    let tmp = path.with_file_name(format!(
        ".{}.factory-tmp",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    std::fs::write(&tmp, ch.text).map_err(|e| failed(e.to_string()))?;
    if std::fs::read_to_string(&path).ok().as_deref() != Some(now.as_str()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(unwritten(format!("{file} changed while it was being written: nothing written")));
    }
    put(&b, &path, &tmp, file, ch.dest)?;
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

fn change(c: &Call, project: &str, file: &str, edit: &BoardEdit) -> Result<Value, ActionFailure> {
    let b = backlog(c, project)?;
    if file.split('/').any(|p| p == "..") {
        return Err(failed(format!("{file} isn't a board file")));
    }
    let path = b.join(file);
    let before = std::fs::read_to_string(&path).map_err(|e| failed(format!("{file}: {e}")))?;
    // The file must hold what it held when the change was first made (2.5.5).
    if let Some(want) = c.action.result.as_ref().and_then(|r| r.get("before")).and_then(Value::as_str)
        && crate::files::sha256_hex(before.as_bytes()) != want
    {
        return Err(unwritten(format!("{file} was changed since this change was made: nothing written")));
    }
    let (after, dest) = edited(c, &b, file, &before, edit)?;
    let tmp = path.with_file_name(format!(
        ".{}.factory-tmp",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    std::fs::write(&tmp, &after).map_err(|e| failed(e.to_string()))?;
    // Written only over the content it was made from (2.5.5).
    if std::fs::read_to_string(&path).ok().as_deref() != Some(before.as_str()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(unwritten(format!("{file} changed while it was being written: nothing written")));
    }
    put(&b, &path, &tmp, file, &dest)?;
    Ok(
        json!({"file": dest, "before": crate::files::sha256_hex(before.as_bytes()), "after": crate::files::sha256_hex(after.as_bytes())}),
    )
}
