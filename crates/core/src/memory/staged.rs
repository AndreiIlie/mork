use std::collections::BTreeMap;

use mork_types::card::Card;
use mork_types::memory::{Edit, Staged};
use mork_types::state::Sender;
use mork_types::{AgentKey, CardId};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

fn holding<'a>(state: &'a State, card: &Card) -> Option<&'a Card> {
    let id = crate::reviews::gate::subject_of(card).unwrap_or(&card.id);
    state.cards.get(id).filter(|c| c.stage.is_live())
}

fn descriptions<'a>(
    state: &'a State,
    memory: &str,
    pending: impl Iterator<Item = &'a Staged>,
) -> BTreeMap<&'a str, &'a str> {
    let mut files: BTreeMap<_, _> = state
        .memory_files
        .values()
        .filter(|f| f.memory == memory)
        .map(|f| (f.name.as_str(), f.description.as_str()))
        .collect();
    for entry in pending.filter(|s| !s.conflicted) {
        let name = entry.edit.name.as_str();
        if let Some(after) = &entry.edit.after {
            files.insert(name, &after.description);
        } else {
            files.remove(name);
        }
    }
    files
}

fn validate(memory: &str, edit: &Edit, new: bool, files: BTreeMap<&str, &str>) -> Result<bool, String> {
    let name = edit.name.as_str();
    let exists = files.contains_key(name);
    if !exists {
        let Some(after) = &edit.after else { return Err(format!("no file `{name}` in memory `{memory}`")) };
        if !new && let Some(other) = super::similar::close_file(files.into_iter(), name, &after.description) {
            return Err(format!(
                "close to existing file `{other}` in memory `{memory}`: read it and rewrite it under its name, or write again with `--new`"
            ));
        }
    }
    Ok(exists)
}

pub(super) fn change(
    state: &State,
    from: &Sender,
    memory: &str,
    edit: Edit,
    new: bool,
    ctx: Ctx<'_>,
) -> Result<Decision, String> {
    super::lessons::known(memory, ctx)?;
    let seat = match from {
        Sender::Agent(key) => {
            let card = state
                .agents
                .get(key)
                .and_then(|a| super::origin(state, a).1)
                .and_then(|id| state.cards.get(&id))
                .ok_or("memory edits require a seat on a card")?;
            Some((key, card))
        }
        _ => None,
    };
    let held = seat.and_then(|(key, card)| Some((key, &card.id, holding(state, card)?)));
    let pending = held.into_iter().flat_map(|(key, _, c)| c.staged_memory.iter().filter(move |s| &s.seat == key));
    let exists = validate(memory, &edit, new, descriptions(state, memory, pending))?;
    let kind = if edit.after.is_some() { "memory.write" } else { "memory.delete" };
    let mut answer = json!({"memory": memory, "name": edit.name});
    if edit.after.is_some() {
        answer["rewrote"] = json!(exists);
    }
    let mut p = Poster::for_ctx(state, ctx);
    if let Some((key, origin, holder)) = held {
        stage(&mut p, key, origin, holder.clone(), edit);
        answer["staged"] = json!(true);
    } else {
        super::lessons::apply(&mut p, memory, &edit, &from.label(), seat.map(|(_, c)| &c.id));
    }
    let mut d = Decision::ok(json!({(kind): answer}));
    p.finish(&mut d);
    Ok(d)
}

fn stage(p: &mut Poster<'_>, seat: &AgentKey, origin: &CardId, mut holder: Card, edit: Edit) {
    // Both history readers expose card journal data before the holding card ends.
    let op = if edit.after.is_some() { "write" } else { "delete" };
    p.journal("memory.staged", Some(origin.as_str()), json!({"by": seat, "op": op}));
    holder.staged_memory.push(Staged {
        edit,
        card: origin.clone(),
        seat: seat.clone(),
        at_ms: p.now,
        conflicted: false,
    });
    p.put_card(holder);
}

pub(super) fn mark(p: &mut Poster<'_>, memory: &str, name: &str) {
    let affected = |c: &&Card| {
        c.project.as_str() == memory
            && c.stage.is_live()
            && c.staged_memory.iter().any(|s| !s.conflicted && s.edit.name == name)
    };
    for original in p.state.cards.values().filter(affected) {
        let Some(mut card) = p.card(&original.id).filter(|c| affected(&c)) else { continue };
        for entry in card.staged_memory.iter_mut().filter(|s| s.edit.name == name) {
            entry.conflicted = true;
        }
        p.put_card(card);
    }
}

pub(crate) fn commit(p: &mut Poster<'_>, id: &CardId) {
    let Some(mut holder) = p.card(id).filter(|c| !c.staged_memory.is_empty()) else { return };
    let entries = std::mem::take(&mut holder.staged_memory);
    let project = holder.project.clone();
    p.put_card(holder);
    for s in entries {
        let why = if s.conflicted {
            "file changed after staging"
        } else if super::lessons::apply(p, project.as_str(), &s.edit, s.seat.as_str(), Some(&s.card)) {
            continue;
        } else {
            "file already deleted"
        };
        p.journal(
            "memory.skipped",
            Some(s.card.as_str()),
            json!({
                "memory": project, "name": s.edit.name, "after": s.edit.after, "by": s.seat, "why": why
            }),
        );
    }
}
