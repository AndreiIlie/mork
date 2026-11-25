use std::collections::{BTreeMap, BTreeSet};

use factory_types::command::TextList;
use factory_types::config::Config;
use factory_types::note::{NewNote, Note, NoteWhat, ProjectNotes};
use factory_types::state::{Agent, Sender};
use factory_types::{AgentKey, CardId, Command, ProjectId};
use serde_json::json;

use super::handoffs::{citing_cards, handoff_dir};
use crate::decide::{Ctx, Decision, Intent};
use crate::post::Poster;
use crate::state::{Change, State};

// Handles the three notes commands; deletion has already passed the shared sender check.
pub(super) fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    let result = if let Command::NotesAdd { kind, text, file, title, line, why, project } = cmd {
        what(*kind, text, file.as_ref(), title.as_deref(), *line, why.as_deref())
            .and_then(|w| add(state, from, w, project.as_ref(), ctx))
    } else if let Command::Notes { project } = cmd {
        known(project, ctx).map(|()| list(state, project, ctx))
    } else if let Command::NotesDelete { project, ids } = cmd {
        known(project, ctx).and_then(|()| delete(state, from, project, ids, ctx))
    } else {
        Err(format!("`{}` isn't a notes command", cmd.name()))
    };
    result.unwrap_or_else(Decision::refused)
}

fn known(project: &ProjectId, ctx: Ctx<'_>) -> Result<(), String> {
    ctx.config.projects.contains_key(project).then_some(()).ok_or_else(|| format!("no project `{project}`"))
}

// A note with no project is the factory's: nothing accounted for it.
pub(crate) fn under<'a>(note: &'a Note, factory: &'a ProjectId) -> &'a ProjectId {
    note.project.as_ref().unwrap_or(factory)
}

fn what(
    kind: NewNote,
    text: &str,
    file: Option<&TextList>,
    title: Option<&str>,
    line: Option<u64>,
    why: Option<&str>,
) -> Result<NoteWhat, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("notes.add requires a non-empty text".into());
    }
    if kind == NewNote::Proposal {
        if line.is_some() || why.is_some() {
            return Err("a proposal takes no --line or --why".into());
        }
        return proposal(text, file, title);
    }
    if title.is_some() {
        return Err("only a proposal takes --title".into());
    }
    if kind == NewNote::Smell {
        return smell(text, file, line, why);
    }
    if file.is_some() || line.is_some() || why.is_some() {
        return Err("feedback and a lesson are their text alone: --file, --line and --why are a smell's".into());
    }
    Ok(if kind == NewNote::Lesson {
        NoteWhat::Lesson { text: text.into() }
    } else {
        NoteWhat::Feedback { text: text.into() }
    })
}

fn proposal(text: &str, file: Option<&TextList>, title: Option<&str>) -> Result<NoteWhat, String> {
    let title = title
        .filter(|title| !title.contains(['\r', '\n', '\u{2028}', '\u{2029}']))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or("a proposal requires a non-empty, one-line --title")?;
    let files = match file {
        None => Vec::new(),
        Some(TextList::Items(files)) => files.clone(),
        Some(TextList::Text(_)) => return Err("a proposal's JSON file field takes an array of paths".into()),
    };
    Ok(NoteWhat::Proposal { title: title.into(), text: text.into(), files })
}

fn smell(text: &str, file: Option<&TextList>, line: Option<u64>, why: Option<&str>) -> Result<NoteWhat, String> {
    let Some(TextList::Text(file)) = file else { return Err("a smell requires one non-empty --file".into()) };
    let file = file.trim();
    if file.is_empty() {
        return Err("a smell requires a non-empty --file".into());
    }
    let why = why.map(str::trim).filter(|w| !w.is_empty()).ok_or("a smell requires a non-empty --why")?;
    let line = line.filter(|l| *l > 0).ok_or("a smell requires a --line from 1")?;
    Ok(NoteWhat::Smell { file: file.into(), line, what: text.into(), why: why.into() })
}

/// The project and card a seat's notes are filed under: its card's.
pub(crate) fn origin(state: &State, a: &Agent) -> (Option<ProjectId>, Option<CardId>) {
    let card = crate::messages::places::card_of(state, &a.place);
    let owner = a.key.as_str().split('/').next().unwrap_or_default();
    let project = card
        .as_ref()
        .and_then(|id| state.cards.get(id).map(|c| c.project.clone()))
        .or_else(|| crate::messages::streams::owner_project(state, owner));
    (project, card)
}

fn add(
    state: &State,
    from: &Sender,
    what: NoteWhat,
    project: Option<&ProjectId>,
    ctx: Ctx<'_>,
) -> Result<Decision, String> {
    let (project, card, seat) = if let Sender::Agent(key) = from {
        let a = state.agents.get(key).ok_or_else(|| format!("no seat `{key}`"))?;
        let (own, card) = origin(state, a);
        let own = own.ok_or("this seat has no card's project to file a note under")?;
        let factory = &ctx.config.factory.project;
        // Feedback and a lesson may be the factory's, whichever project's card the seat is on (fac-592, fac-593).
        let to_factory =
            project == Some(factory) && matches!(what, NoteWhat::Feedback { .. } | NoteWhat::Lesson { .. });
        if project.is_some_and(|p| *p != own) && !to_factory {
            return Err(format!(
                "this seat files notes under its own project, `{own}`; only feedback and a lesson may go to the factory's, `--project {factory}`"
            ));
        }
        (project.cloned().unwrap_or(own), card, Some(key.clone()))
    } else {
        let p = project.cloned().ok_or("outside a wall, notes.add requires --project")?;
        (p, None, None)
    };
    known(&project, ctx)?;
    let id = state.notes_last.checked_add(1).ok_or("note ids exhausted")?;
    let note = Note { id, project: Some(project), card, seat, time: ctx.now, what };
    let mut d = Decision::ok(json!({"notes.add": note}));
    let data = json!({"id": id, "kind": note.what.kind(), "project": note.project, "by": from.label()});
    d.journal("note.added", note.card.as_ref().map(CardId::as_str), data);
    d.changes.push(Change::PutNote(Box::new(note)));
    Ok(d)
}

/// Files a note the engine took for a seat or a card, after any filed in the same decision.
pub(crate) fn put(
    p: &mut Poster<'_>,
    project: Option<ProjectId>,
    card: Option<CardId>,
    seat: Option<AgentKey>,
    what: NoteWhat,
) {
    let pending = p.other.iter().filter_map(|c| if let Change::PutNote(n) = c { Some(n.id) } else { None }).max();
    let id = pending.unwrap_or(0).max(p.state.notes_last).saturating_add(1);
    let data = json!({"id": id, "kind": what.kind(), "project": project, "seat": seat});
    p.journal("note.added", card.as_ref().map(CardId::as_str), data);
    let time = p.now;
    p.other.push(Change::PutNote(Box::new(Note { id, project, card, seat, time, what })));
}

fn list(state: &State, project: &ProjectId, ctx: Ctx<'_>) -> Decision {
    let factory = &ctx.config.factory.project;
    let notes: Vec<&Note> = state.notes.values().rev().filter(|n| under(n, factory) == project).collect();
    Decision::ok(json!({"notes": {"project": project, "notes": notes}}))
}

/// Main's Notes tab: configured projects in file order, then removed projects by name; notes newest first.
pub fn view(state: &State, config: &Config) -> Vec<ProjectNotes> {
    let mut projects: BTreeMap<ProjectId, Vec<Note>> = BTreeMap::new();
    for n in state.notes.values().rev() {
        projects.entry(under(n, &config.factory.project).clone()).or_default().push(n.clone());
    }
    let order: Vec<_> = config.ordered_projects().collect();
    let mut out: Vec<_> = projects.into_iter().map(|(project, notes)| ProjectNotes { project, notes }).collect();
    out.sort_by_key(|p| order.iter().position(|name| **name == p.project).unwrap_or(usize::MAX));
    out
}

fn delete(state: &State, from: &Sender, project: &ProjectId, ids: &[String], ctx: Ctx<'_>) -> Result<Decision, String> {
    if ids.is_empty() {
        return Err("notes.delete requires at least one id".into());
    }
    let factory = &ctx.config.factory.project;
    let mut selected = BTreeSet::new();
    for id in ids {
        let id = id.parse::<u64>().map_err(|_| format!("`{id}` isn't a note id"))?;
        if !state.notes.get(&id).is_some_and(|n| under(n, factory) == project) {
            return Err(format!("project `{project}` has no note {id}"));
        }
        selected.insert(id);
    }
    for id in &selected {
        if let Some(Note { card: Some(card), what: NoteWhat::Handoff { .. }, .. }) = state.notes.get(id) {
            let citing = citing_cards(state, card);
            if !citing.is_empty() {
                let citing: Vec<_> = citing.iter().map(CardId::as_str).collect();
                return Err(format!("note {id} keeps `{card}`'s handoff, cited by live cards: {}", citing.join(", ")));
            }
        }
    }
    let mut d = Decision::ok(json!({"notes.delete": {"project": project, "deleted": selected.len()}}));
    d.journal("notes.deleted", None, json!({"project": project, "ids": selected, "by": from.label()}));
    for id in &selected {
        // A handoff's note goes with its files: nothing else lists them.
        if let Some(Note { card: Some(card), project: kept, what: NoteWhat::Handoff { run, .. }, .. }) =
            state.notes.get(id)
        {
            let (project, dir) = (kept.clone().unwrap_or_else(|| project.clone()), handoff_dir(card, *run));
            d.intents.push(Intent::RemoveHandoff { project, dir });
        }
    }
    d.changes.extend(selected.into_iter().map(|id| Change::RemoveNote { id }));
    Ok(d)
}
