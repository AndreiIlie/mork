use factory_types::config::Config;
use factory_types::memory::{FileRow, GENERAL, MemoryFile, MemoryView, Tally, key};
use factory_types::state::Sender;
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::state::{Change, State};

const NAME_MAX: usize = 64;
const DESCRIPTION_MAX: usize = 160;
const TEXT_MAX: usize = 4096;

pub(super) struct Written<'a> {
    pub(super) memory: &'a str,
    pub(super) name: &'a str,
    pub(super) description: &'a str,
    pub(super) text: &'a str,
    pub(super) new: bool,
}

pub(super) fn known(memory: &str, ctx: Ctx<'_>) -> Result<(), String> {
    let project = ctx.config.projects.keys().any(|p| p.as_str() == memory);
    match (memory == GENERAL, project) {
        (true, true) => Err(format!("a project is called `{GENERAL}`, as the general memory is: rename it")),
        (true, false) | (false, true) => Ok(()),
        (false, false) => Err(format!("no memory `{memory}`: it is `{GENERAL}`, or a project's name")),
    }
}

pub(super) fn found<'a>(state: &'a State, memory: &str, name: &str, ctx: Ctx<'_>) -> Result<&'a MemoryFile, String> {
    known(memory, ctx)?;
    state.memory_files.get(&key(memory, name)).ok_or_else(|| format!("no file `{name}` in memory `{memory}`"))
}

fn tally_of(state: &State, memory: &str) -> Tally {
    state.memory_tallies.get(memory).cloned().unwrap_or_else(|| Tally { memory: memory.into(), ..Tally::default() })
}

fn tallied(state: &State, memory: &str, count: impl FnOnce(&mut Tally)) -> Change {
    let mut t = tally_of(state, memory);
    count(&mut t);
    Change::PutMemoryTally(t)
}

/// Andrei's send of a memory to review (fac-610): its tally takes the time.
pub(crate) fn reviewed(state: &State, memory: &str, now: i64) -> Change {
    tallied(state, memory, |t| t.reviewed_ms = Some(now))
}

fn held(state: &State) -> Vec<&str> {
    let mut memories: Vec<&str> = state.memory_tallies.keys().map(String::as_str).collect();
    memories.extend(state.memory_files.values().map(|f| f.memory.as_str()));
    memories.sort_unstable();
    memories.dedup();
    memories
}

/// Only a seat's read is counted: an orchestrator's or Andrei's gets the text and changes nothing (fac-598).
pub(super) fn read(state: &State, from: &Sender, f: &MemoryFile, now: i64) -> Decision {
    let mut d = Decision::ok(json!({"memory.read": {
        "memory": f.memory, "name": f.name, "description": f.description,
        "writer": f.writer, "origin": f.origin, "written_ms": f.written_ms, "text": f.text,
    }}));
    if matches!(from, Sender::Agent(_)) {
        let mut next = f.clone();
        next.reads += 1;
        next.last_read_ms = Some(now);
        d.journal("memory.read", None, json!({"memory": f.memory, "name": f.name}));
        d.changes.push(Change::PutMemoryFile(Box::new(next)));
        d.changes.push(tallied(state, &f.memory, |t| t.read += 1));
    }
    d
}

pub(super) fn write(state: &State, from: &Sender, w: Written<'_>, ctx: Ctx<'_>) -> Result<Decision, String> {
    known(w.memory, ctx)?;
    checked(&w)?;
    let old = state.memory_files.get(&key(w.memory, w.name));
    if old.is_none() && !w.new {
        let files = state.memory_files.values().filter(|f| f.memory == w.memory);
        if let Some(f) = super::similar::close_file(files, w.name, w.description) {
            return Err(format!(
                "close to existing file `{}` in memory `{}`: read it and rewrite it under its name, or write again with `--new`",
                f.name, f.memory
            ));
        }
    }
    let f = MemoryFile {
        memory: w.memory.into(),
        name: w.name.into(),
        description: w.description.trim().into(),
        text: w.text.into(),
        writer: from.label(),
        origin: old.map_or_else(|| from.label(), |o| o.origin.clone()),
        written_ms: ctx.now,
        reads: old.map_or(0, |o| o.reads),
        last_read_ms: old.and_then(|o| o.last_read_ms),
    };
    let mut d = Decision::ok(json!({"memory.write": {"memory": f.memory, "name": f.name, "rewrote": old.is_some()}}));
    d.journal("memory.write", None, json!({"memory": f.memory, "name": f.name, "by": f.writer}));
    d.changes.push(Change::PutMemoryFile(Box::new(f)));
    d.changes.push(tallied(state, w.memory, |t| t.written += 1));
    Ok(d)
}

fn checked(w: &Written<'_>) -> Result<(), String> {
    let slug = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    if w.name.is_empty() || w.name.len() > NAME_MAX || !w.name.chars().all(slug) {
        return Err(format!("a file's name is a slug of lowercase letters, digits and `-`, {NAME_MAX} at most"));
    }
    let description = w.description.trim();
    if description.is_empty() || description.chars().count() > DESCRIPTION_MAX || description.contains('\n') {
        return Err(format!("a file's description is one line of {DESCRIPTION_MAX} characters at most"));
    }
    if w.text.trim().is_empty() || w.text.len() > TEXT_MAX {
        return Err(format!("a file's text is {TEXT_MAX} bytes at most, and not empty"));
    }
    Ok(())
}

pub(super) fn delete(state: &State, f: &MemoryFile) -> Decision {
    let mut d = Decision::ok(json!({"memory.delete": {"memory": f.memory, "name": f.name}}));
    d.journal("memory.delete", None, json!({"memory": f.memory, "name": f.name, "why": "deleted"}));
    d.changes.push(Change::RemoveMemoryFile { key: f.key() });
    d.changes.push(tallied(state, &f.memory, |t| t.deleted += 1));
    d
}

// `memory.stats`: each memory's counts and files, or `only`'s.
pub(super) fn stats(state: &State, only: Option<&str>, config: &Config) -> Value {
    let order: Vec<&str> = std::iter::once(GENERAL).chain(config.ordered_projects().map(|p| p.as_str())).collect();
    let mut memories = held(state);
    memories.sort_by_key(|m| order.iter().position(|p| p == m).unwrap_or(usize::MAX));
    let listed: Vec<Value> = memories
        .into_iter()
        .filter(|m| only.is_none_or(|o| o == *m))
        .map(|m| {
            let t = tally_of(state, m);
            let files: Vec<Value> = state
                .memory_files
                .values()
                .filter(|f| f.memory == m)
                .map(|f| {
                    json!({"name": f.name, "description": f.description, "writer": f.writer, "origin": f.origin,
                           "written_ms": f.written_ms, "reads": f.reads, "last_read_ms": f.last_read_ms})
                })
                .collect();
            json!({"memory": m, "written": t.written, "read": t.read, "deleted": t.deleted,
                   "reviewed_ms": t.reviewed_ms, "files": files})
        })
        .collect();
    json!({"memory.stats": {"memories": listed}})
}

/// Main's Memory tab: `general`, then each project in the config's order, then any memory of a
/// project the config no longer has; seeing them reads nothing.
pub fn view(state: &State, offset_min: i32, ctx: Ctx<'_>) -> Vec<MemoryView> {
    let mut memories: Vec<&str> = vec![GENERAL];
    memories.extend(ctx.config.ordered_projects().map(|p| p.as_str()).filter(|p| *p != GENERAL));
    let mut gone = held(state);
    gone.retain(|m| !memories.contains(m));
    memories.extend(gone);
    memories.into_iter().map(|m| memory_view(state, m, offset_min)).collect()
}

fn memory_view(state: &State, memory: &str, offset_min: i32) -> MemoryView {
    let files = state
        .memory_files
        .values()
        .filter(|f| f.memory == memory)
        .map(|f| FileRow {
            name: f.name.clone(),
            description: f.description.clone(),
            reads: f.reads,
            last_read_ms: f.last_read_ms,
        })
        .collect();
    let tally = tally_of(state, memory);
    let reviewed = tally.reviewed_ms.map(|at| {
        let local = at.saturating_add(i64::from(offset_min) * 60_000);
        let (y, mo, d, ..) = crate::state::names::civil_from_ms(local);
        format!("{d:02}.{mo:02}.{y:04}")
    });
    MemoryView { tally, reviewed, files }
}
