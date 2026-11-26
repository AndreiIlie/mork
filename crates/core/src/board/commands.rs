use mork_types::action::{BoardEdit, BoardNote};
use mork_types::state::Sender;
use mork_types::{CardId, Command};
use serde_json::json;

use super::writes;
use super::{Fields, check_fields, no_markers, one_line};
use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

/// A board command, past the buzz.
pub fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::CardCreate { project, title, description, id } = cmd {
        create(state, project, title, description, id.as_deref(), ctx)
    } else if let Command::CardClose { card, close_as, note } = cmd {
        close(state, from, card.as_str(), close_as, note, ctx)
    } else if let Command::CardChange {
        card,
        note,
        decision,
        withdraw,
        title,
        status,
        priority,
        labels,
        parent,
        acceptance,
        description,
    } = cmd
    {
        let f = Fields {
            title: title.clone(),
            status: status.clone(),
            priority: priority.clone(),
            labels: labels.clone(),
            parent: parent.clone(),
            acceptance: acceptance.clone(),
            description: description.clone(),
            note: note.clone(),
        };
        change(state, from, card.as_str(), withdraw.as_deref(), decision.as_deref(), f, ctx)
    } else {
        Decision::refused(format!("`{}` isn't a board command", cmd.name()))
    }
}

// `card.create <project> <title> <description> [id]`.
fn create(state: &State, project: &str, title: &str, description: &str, id: Option<&str>, ctx: Ctx<'_>) -> Decision {
    if !ctx.config.projects.contains_key(project) {
        return Decision::refused(format!("no project `{project}`"));
    }
    if let Err(e) = one_line("title", title).and_then(|()| no_markers("description", description)) {
        return Decision::refused(e.to_string());
    }
    if let Some(i) = id {
        let well_formed = i.split_once('-').is_some_and(|(pre, n)| {
            !pre.is_empty()
                && pre.chars().all(char::is_alphabetic)
                && n.split('.').all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()))
        });
        if !well_formed {
            return Decision::refused(format!("`{i}` isn't an id"));
        }
    }
    let asked = id.map(CardId::new);
    // The same request is the same action, pending, started or done, whatever the board holds
    // now: it gets the first one's result (2.5.4, M4.19).
    let same = state.actions.values().find(|a| {
        a.status != mork_types::action::ActionStatus::Dropped
            && matches!(&a.what, mork_types::action::ActionWhat::CreateBoardCard {
                project: p, title: t, description: x, id: i, card: None, ..
            } if p.as_str() == project && t == title && x == description && i.as_ref() == asked.as_ref())
    });
    if let Some(a) = same {
        return Decision::ok(json!({"card.create": {"project": project, "action": a.id}}));
    }
    // Then a new card's id must be free (M5.5: the same request first).
    if let Some(i) = id
        && state.board.contains_key(&CardId::new(i.to_lowercase()))
    {
        return Decision::refused(format!("{i} is taken"));
    }
    let mut p = Poster::for_ctx(state, ctx);
    let what = match writes::create(state, &project.into(), title, description, asked.as_ref(), None, ctx.now) {
        Ok(w) => w,
        Err(why) => return Decision::refused(why),
    };
    let action = p.act(what);
    let mut d = Decision::ok(json!({"card.create": {"project": project, "action": action}}));
    p.finish(&mut d);
    d
}

fn file_of(state: &State, card: &str) -> Result<(mork_types::ProjectId, String), Decision> {
    let id = CardId::new(card.to_lowercase());
    state
        .board
        .get(&id)
        .map(|b| (b.project.clone(), b.file.clone()))
        .ok_or_else(|| Decision::refused(format!("no configured board holds {card}")))
}

// `card.change <card> …`: every field given, as one change; a note alone stays a note.
fn change(
    state: &State,
    from: &Sender,
    card: &str,
    withdraw: Option<&str>,
    decision: Option<&str>,
    f: Fields,
    ctx: Ctx<'_>,
) -> Decision {
    let edit = || match withdraw {
        Some(w) => withdrawal(state, from, card, w, decision.is_some() || f != Fields::default()),
        None => edit_of(from, decision, f),
    };
    recorded(state, card, "card.change", edit, ctx)
}

// The edit checked against the card's file as last read, and recorded as one action. Refused
// before anything is recorded: a write it can't make leaves no action behind.
fn recorded(
    state: &State,
    card: &str,
    command: &str,
    edit: impl FnOnce() -> Result<BoardEdit, String>,
    ctx: Ctx<'_>,
) -> Decision {
    let (project, file) = match file_of(state, card) {
        Ok(x) => x,
        Err(d) => return d,
    };
    let what = match edit().and_then(|e| writes::checked_change(state, &project, &file, e, None, ctx.now)) {
        Ok(w) => w,
        Err(why) => return Decision::refused(why),
    };
    let mut p = Poster::for_ctx(state, ctx);
    let action = p.act(what);
    let mut d = Decision::ok(json!({command: {"card": card, "action": action}}));
    p.finish(&mut d);
    d
}

// `withdraw`: a note quoting the standing decision numbered so (fac-347, fac-256). The decision
// stays on the card; the note takes it out of the specification.
fn withdrawal(state: &State, from: &Sender, card: &str, number: &str, others: bool) -> Result<BoardEdit, String> {
    if others {
        return Err("give `withdraw` alone: it takes no other field".into());
    }
    let number = number.trim();
    let n: usize = number
        .strip_prefix('#')
        .unwrap_or(number)
        .parse()
        .map_err(|_| format!("`withdraw` takes the number of a note (`3` or `#3`), not `{number}`"))?;
    let board = state.board.get(&CardId::new(card.to_lowercase()));
    let notes = crate::board::notes::apart(state, board, board.map_or("", |b| b.description.as_str())).0;
    if n == 0 || n > notes.len() {
        return Err(format!("{card} has no note #{n}: it has {}", notes.len()));
    }
    let i = notes.len() - n;
    let Some(words) = mork_types::decisions::decision_words(&notes[i]) else {
        return Err(format!("note #{n} of {card} is not a marked decision"));
    };
    if mork_types::decisions::withdrawn(&notes)[i] {
        return Err(format!("decision #{n} of {card} is already withdrawn"));
    }
    Ok(BoardEdit::Note { who: from.label(), text: mork_types::decisions::withdrawal(n, words) })
}

// Each value is checked, and a refusal names its field.
fn edit_of(from: &Sender, decision: Option<&str>, mut f: Fields) -> Result<BoardEdit, String> {
    if decision.is_some() && f.note.is_some() {
        return Err("give either `note` or `decision`, not both".into());
    }
    if decision.is_some_and(|d| d.trim().is_empty()) {
        return Err("`decision` is empty".into());
    }
    if let Some(d) = decision {
        f.note = Some(format!("Decision: {d}"));
    }
    if f == Fields::default() {
        return Err("nothing to change: give a note or a field".into());
    }
    if f.note.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err("`note` is empty".into());
    }
    check_fields(&f).map_err(|e| e.to_string())?;
    let note = f.note.clone().map(|text| BoardNote { who: from.label(), text });
    let alone = Fields { note: None, ..f.clone() } == Fields::default();
    if alone && let Some(n) = note {
        return Ok(BoardEdit::Note { who: n.who, text: n.text });
    }
    Ok(BoardEdit::Fields {
        title: f.title,
        status: f.status,
        priority: f.priority,
        labels: f.labels,
        parent: f.parent,
        acceptance: f.acceptance,
        description: f.description,
        note: note.map(Box::new),
    })
}

// `card.close <card> done|dropped <note>`.
fn close(state: &State, from: &Sender, card: &str, how: &str, note: &str, ctx: Ctx<'_>) -> Decision {
    let done = match how {
        "done" => true,
        "dropped" => false,
        other => return Decision::refused(format!("`{other}` isn't how a card closes: done or dropped")),
    };
    let edit = || {
        no_markers("note", note).map_err(|e| e.to_string())?;
        Ok(BoardEdit::Close { done, who: from.label(), note: note.to_owned() })
    };
    recorded(state, card, "card.close", edit, ctx)
}
