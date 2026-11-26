//! A board write's content is decided when it is made (2.5, 4.18, M3.16): the engine writes exactly that, and recovers
//! by it after a crash.

use mork_types::action::{ActionWhat, BoardEdit};
use mork_types::card::BoardCard;
use mork_types::hash::short;
use mork_types::{CardId, ProjectId};

use super::notes::Written;
use crate::board;
use crate::state::State;

/// `YYYY-MM-DD HH:MM` (UTC), as Backlog.md dates.
pub fn date(ms: i64) -> String {
    let (y, mo, d, h, mi, _) = crate::state::names::civil_from_ms(ms);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}

// A dated note's line: `<who> 14.10: …`.
fn note_line(who: &str, text: &str, now: i64) -> String {
    let (_, mo, d, _, _, _) = crate::state::names::civil_from_ms(now);
    format!("{who} {d:02}.{mo:02}: {text}")
}

fn cards_of<'a>(state: &'a State, project: &'a ProjectId) -> impl Iterator<Item = &'a BoardCard> + 'a {
    state.board.values().filter(move |b| &b.project == project)
}

/// A new card on a project's board: its id (asked, or the next free one), its file and text.
pub(super) fn create(
    state: &State,
    project: &ProjectId,
    title: &str,
    description: &str,
    asked: Option<&CardId>,
    card: Option<CardId>,
    now: i64,
) -> Result<ActionWhat, String> {
    // The prefix its cards carry, or the board's own (`config.yml`), when it has no card (M5.27).
    let prefix = cards_of(state, project)
        .map(|b| b.prefix.clone())
        .find(|p| !p.is_empty())
        .or_else(|| state.world.board_prefixes.get(project).cloned())
        .unwrap_or_default();
    let mut what = ActionWhat::CreateBoardCard {
        project: project.clone(),
        title: title.to_owned(),
        description: description.to_owned(),
        id: asked.cloned(),
        file: String::new(),
        text: String::new(),
        card,
    };
    // A board whose prefix isn't known yet (no card read from it): the engine picks, as it reads.
    if prefix.is_empty() {
        return Ok(what);
    }
    let ids: Vec<String> = cards_of(state, project).map(|b| b.id.to_string()).collect();
    let id = match asked {
        Some(a) => {
            let n = board::number(&prefix, a).ok_or_else(|| format!("`{a}` isn't an id on the prefix {prefix}"))?;
            if ids.iter().any(|x| board::number(&prefix, x) == Some(n)) {
                return Err(format!("{a} is taken"));
            }
            a.to_uppercase()
        }
        None => board::next_id(&prefix, &ids).to_uppercase(),
    };
    let (name, body) = board::new_card(&id, title, description, &date(now)).map_err(|e| e.to_string())?;
    if let ActionWhat::CreateBoardCard { file, text, .. } = &mut what {
        *file = format!("tasks/{name}");
        *text = body;
    }
    Ok(what)
}

/// A change of a board file: its text after, and where it goes, from the file as last read.
/// A file not in the index, or one this edit can't be made to, is left to the engine to read.
pub fn change(
    state: &State,
    project: &ProjectId,
    file: &str,
    edit: BoardEdit,
    card: Option<CardId>,
    now: i64,
) -> ActionWhat {
    match checked_change(state, project, file, edit.clone(), card.clone(), now) {
        Ok(what) => what,
        Err(_) => made(project, file, edit, card, None),
    }
}

/// As [`change`], refused with why when the file as last read can't take the edit, so a
/// command records nothing it can't write.
pub(super) fn checked_change(
    state: &State,
    project: &ProjectId,
    file: &str,
    edit: BoardEdit,
    card: Option<CardId>,
    now: i64,
) -> Result<ActionWhat, String> {
    let Some(read) = cards_of(state, project).find(|b| b.file == file && !b.text.is_empty()) else {
        return Ok(made(project, file, edit, card, None));
    };
    let (after, dest) = edited(&read.text, &read.done_status, file, &edit, &written(state, project, file), now)?;
    Ok(made(project, file, edit, card, Some((short(&read.text), after, dest))))
}

fn made(
    project: &ProjectId,
    file: &str,
    edit: BoardEdit,
    card: Option<CardId>,
    decided: Option<(String, String, String)>,
) -> ActionWhat {
    let (before, after, text, dest) = match decided {
        Some((before, text, dest)) => (before, short(&text), text, dest),
        None => (String::new(), String::new(), String::new(), String::new()),
    };
    ActionWhat::ChangeBoardCard {
        project: project.clone(),
        file: file.to_owned(),
        edit,
        before,
        after,
        text,
        dest,
        card,
        lands: None,
    }
}

/// Every note Mork wrote into this card's board file, in whichever folder it then was, dated when its action
/// was made, its lines as they went in: what tells a note's lines from the description.
pub fn written(state: &State, project: &ProjectId, file: &str) -> Vec<Written> {
    let here = |(p, w): &(&ProjectId, Written)| *p == project && w.of(file);
    state.actions.values().filter_map(written_note).filter(here).map(|(_, w)| w).collect()
}

/// A board file's name, the same in every folder a close moves it to.
pub fn name(file: &str) -> &str {
    file.rsplit('/').next().unwrap_or(file)
}

/// The note a recorded board write put in, as [`written`] reads it: its project, and the note.
pub fn written_note(a: &mork_types::action::Action) -> Option<(&ProjectId, Written)> {
    let ActionWhat::ChangeBoardCard { edit, project, file, dest, .. } = &a.what else { return None };
    if a.status == mork_types::action::ActionStatus::Dropped {
        return None;
    }
    let (who, text) = match edit {
        BoardEdit::Note { who, text } => (who.clone(), text.clone()),
        BoardEdit::Fields { note: Some(n), .. } => (n.who.clone(), n.text.clone()),
        BoardEdit::Close { done, who, note } => {
            (who.clone(), format!("closed as {}: {note}", if *done { "done" } else { "dropped" }))
        }
        BoardEdit::Fields { .. } => return None,
    };
    let lines = note_line(&who, &text, a.created_ms).lines().map(str::to_owned).collect();
    Some((project, Written { lines, file: file.clone(), dest: dest.clone(), made: a.created_ms }))
}

/// The file's text after the edit, and where it goes (a closed card moves).
pub fn edited(
    before: &str,
    done_status: &str,
    file: &str,
    edit: &BoardEdit,
    written: &[Written],
    now: i64,
) -> Result<(String, String), String> {
    let when = date(now);
    let after = match edit {
        BoardEdit::Note { who, text } => board::add_note(before, &note_line(who, text, now), &when),
        BoardEdit::Fields { title, status, priority, labels, parent, acceptance, description, note } => {
            let f = board::Fields {
                title: title.clone(),
                status: status.clone(),
                priority: priority.clone(),
                labels: labels.clone(),
                parent: parent.clone(),
                acceptance: acceptance.clone(),
                description: description.clone(),
                note: note.as_ref().map(|n| note_line(&n.who, &n.text, now)),
            };
            board::set_fields(before, &f, written, &when)
        }
        BoardEdit::Close { done, who, note } => {
            let how = if *done { "done" } else { "dropped" };
            let line = note_line(who, &format!("closed as {how}: {note}"), now);
            if *done { board::close(before, done_status, &line, &when) } else { board::add_note(before, &line, &when) }
        }
    }
    .map_err(|e| e.to_string())?;
    let name = name(file);
    let dest = match edit {
        BoardEdit::Close { done: true, .. } => format!("completed/{name}"),
        BoardEdit::Close { done: false, .. } => format!("archive/tasks/{name}"),
        BoardEdit::Note { .. } | BoardEdit::Fields { .. } => file.to_owned(),
    };
    Ok((after, dest))
}
