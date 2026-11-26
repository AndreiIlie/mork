// Board writes (4.18): a new card is hard-linked so it never overwrites; a change is written only over the
// content it was made from. Checked for leaks first (3.8); the engine never commits a board file.

use mork_types::action::{ActionWhat, BoardEdit};
use mork_types::work::ActionFailure;
use serde_json::Value;

use super::act_run::Call;
use crate::board;
use crate::board::BoardWrite;

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
    board::write(&c.project(&c.project)?.board).write(&c.action, c.now)
}

/// A board command's leak, before anything is recorded: the kind of secret, and the field
/// that holds it. [`write_leak`] checks what the write would hold, as the write does.
pub fn command_leak(secrets: &crate::secrets::Secrets, cmd: &mork_types::Command) -> Option<String> {
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
pub fn write_leak(secrets: &crate::secrets::Secrets, what: &ActionWhat, cmd: &mork_types::Command) -> Option<String> {
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
