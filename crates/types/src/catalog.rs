//! The command catalog (1.13): one declaration per command drives strict parsing (1.6.6), `mork ctl`'s
//! subcommands (1.7.1) and `core`'s dispatch to the concern that decides it.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Str,
    U64,
    /// A whole number, or a name or text (`plan`'s deliverable, `plan.task`'s task).
    Pick,
    Bool,
    StrList,
    StrOrList,
    /// A JSON object (`door`'s `params`).
    Object,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub ty: FieldType,
    pub optional: bool,
}

/// The concern of `core` that decides a command, or the engine, which answers it itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// Its reads, builds, pins, reloads and door calls: never decided in `core`.
    Engine,
    /// Merging, and the test slots.
    Queue,
    /// The door's asks.
    Door,
    /// A card's run, from its start to its end.
    Lifecycle,
    /// Review cards, and our cards' PR reviews.
    Reviews,
    /// Messages, asks, and what the operator hands the orchestrators.
    Messages,
    /// Seats: children, their models and sessions.
    Agents,
    /// The boards' card files.
    Board,
    /// Reads of the state.
    Views,
    /// The shared memory and the projects' notes.
    Memory,
    /// Held outward actions.
    Maintenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub area: Area,
    pub fields: &'static [Field],
    /// Only a build with `faults` knows it (`debug.sleep`, 1.10.6).
    pub faults_only: bool,
    /// It acts on nothing outward, so a reload under way doesn't refuse it (1.8.6.2).
    pub read: bool,
}

const fn req(name: &'static str, ty: FieldType) -> Field {
    Field { name, ty, optional: false }
}
const fn opt(name: &'static str, ty: FieldType) -> Field {
    Field { name, ty, optional: true }
}
const fn cmd(name: &'static str, area: Area, fields: &'static [Field]) -> CommandSpec {
    CommandSpec { name, area, fields, faults_only: false, read: false }
}
const fn read(name: &'static str, area: Area, fields: &'static [Field]) -> CommandSpec {
    CommandSpec { name, area, fields, faults_only: false, read: true }
}

use Area::{Agents, Board, Door, Engine, Lifecycle, Maintenance, Memory, Messages, Queue, Reviews, Views};
use FieldType::{Bool, Object, Pick, Str, StrList, StrOrList, U64};

/// Every command with its deciding area and fields, the reads among them, and `debug.sleep`.
pub const CATALOG: &[CommandSpec] = &[
    read("watch", Engine, &[req("places", StrList), opt("tail", U64), opt("full", Bool)]),
    read("hook", Engine, &[]),
    read("wakes", Engine, &[]),
    read("history", Engine, &[req("place", Str), req("before", U64), req("count", U64), opt("turns", Bool)]),
    read("history.search", Engine, &[req("words", Str)]),
    read(
        "history.journal",
        Engine,
        &[
            req("card", Str),
            req("count", U64),
            opt("seq", U64),
            opt("at", U64),
            opt("before", U64),
            opt("kinds", StrList),
        ],
    ),
    read("status", Engine, &[opt("jobs", Bool), opt("all", Bool), opt("card", Str)]),
    read("usage", Engine, &[]),
    cmd("claude.account", Engine, &[req("name", Str)]),
    read("projects", Views, &[]),
    cmd("lookback", Lifecycle, &[req("project", Str), opt("read", Str)]),
    cmd("build", Engine, &[req("commit", Str)]),
    cmd("pin", Engine, &[req("commit", Str)]),
    cmd("reload", Engine, &[opt("target", Str)]),
    cmd(
        "say",
        Messages,
        &[
            req("place", Str),
            opt("to", Str),
            req("text", Str),
            opt("images", StrList),
            opt("kind", Str),
            opt("delivery", Str),
            opt("choice", U64),
        ],
    ),
    cmd("message.send-now", Messages, &[req("place", Str), req("seq", U64)]),
    cmd("dm", Agents, &[req("card", Str), req("role", Str)]),
    cmd(
        "spawn",
        Agents,
        &[
            req("card", Str),
            req("role", Str),
            req("task", Str),
            opt("model", Str),
            opt("effort", Str),
            opt("cli", Str),
            opt("topic", Str),
        ],
    ),
    cmd("ack", Messages, &[req("ask", Str)]),
    cmd(
        "start",
        Lifecycle,
        &[
            req("card", Str),
            opt("mode", Str),
            opt("visual", Bool),
            opt("model", Str),
            opt("effort", Str),
            opt("delegated", Bool),
            opt("self-sustained", Bool),
            opt("no-planner", Bool),
            opt("carries", StrList),
        ],
    ),
    cmd("approve", Lifecycle, &[req("card", Str)]),
    cmd("mock", Lifecycle, &[req("card", Str)]),
    cmd("brief", Lifecycle, &[req("card", Str), opt("words", Str)]),
    cmd("merge", Queue, &[req("card", Str)]),
    cmd("stop", Lifecycle, &[opt("card", Str), opt("agent", Str)]),
    cmd("resume", Lifecycle, &[opt("card", Str), opt("agent", Str)]),
    cmd("fresh", Agents, &[req("agent", Str)]),
    cmd("pick", Agents, &[req("agent", Str), opt("model", Str), opt("effort", Str), opt("cli", Str)]),
    cmd("close", Lifecycle, &[req("card", Str), req("why", Str)]),
    cmd("done", Lifecycle, &[req("card", Str)]),
    cmd("preview", Lifecycle, &[req("card", Str), opt("action", Str)]),
    cmd("demo", Lifecycle, &[req("card", Str)]),
    read("prs", Views, &[req("project", Str)]),
    cmd("review", Reviews, &[req("project", Str), req("pr", U64)]),
    cmd("review.merge", Reviews, &[req("project", Str), req("pr", U64)]),
    cmd("review.branch", Reviews, &[req("project", Str), req("branch", Str)]),
    cmd("post", Reviews, &[req("card", Str)]),
    cmd("resolve", Reviews, &[req("card", Str), req("n", U64), opt("declined", Bool), opt("dropped", Str)]),
    cmd("answer", Reviews, &[req("card", Str), req("n", U64), req("text", Str)]),
    cmd("rereview", Reviews, &[req("card", Str)]),
    cmd("triage", Reviews, &[req("card", Str), opt("fix", StrList), opt("decline", StrList), opt("foreman", StrList)]),
    cmd("deliverables", Lifecycle, &[req("card", Str), req("names", StrOrList)]),
    cmd("plan", Lifecycle, &[req("card", Str), req("deliverable", Pick), req("tasks", StrOrList)]),
    cmd("plan.task", Lifecycle, &[req("card", Str), req("deliverable", Pick), req("task", Pick)]),
    read("card.show", Views, &[req("card", Str)]),
    read("findings", Views, &[req("card", Str)]),
    cmd("card.create", Board, &[req("project", Str), req("title", Str), req("description", Str), opt("id", Str)]),
    cmd(
        "card.change",
        Board,
        &[
            req("card", Str),
            opt("note", Str),
            opt("title", Str),
            opt("status", Str),
            opt("priority", Str),
            opt("labels", StrList),
            opt("parent", Str),
            opt("acceptance", Str),
            opt("description", Str),
            opt("decision", Str),
            opt("withdraw", Str),
        ],
    ),
    cmd("card.close", Board, &[req("card", Str), req("as", Str), req("note", Str)]),
    read("actions", Views, &[]),
    cmd("action.retry", Maintenance, &[req("action", Str)]),
    cmd("action.drop", Maintenance, &[req("action", Str)]),
    cmd("action.done", Maintenance, &[req("action", Str), req("note", Str)]),
    cmd("test.lease", Queue, &[req("kind", Str), req("command", StrList)]),
    cmd("test.resume", Queue, &[req("lease", Str)]),
    cmd("test.done", Queue, &[req("lease", Str), req("exit", U64)]),
    cmd("door", Engine, &[req("target", Str), req("op", Str), req("params", Object)]),
    cmd("door.once", Door, &[req("ask", Str)]),
    cmd("door.no", Door, &[req("ask", Str)]),
    read("door.help", Engine, &[opt("target", Str)]),
    cmd("give", Messages, &[req("ask", Str)]),
    cmd("opinion", Messages, &[req("ask", Str)]),
    cmd("delegate", Messages, &[req("card", Str), opt("off", Bool), opt("self-sustained", Bool)]),
    cmd("auto", Messages, &[req("project", Str), opt("off", Bool)]),
    cmd("visual", Lifecycle, &[req("card", Str), opt("off", Bool)]),
    cmd("memory.read", Memory, &[req("memory", Str), req("name", Str)]),
    read("memory.search", Memory, &[req("words", Str)]),
    cmd(
        "memory.write",
        Memory,
        &[req("memory", Str), req("name", Str), req("description", Str), req("text", Str), opt("new", Bool)],
    ),
    cmd("memory.delete", Memory, &[req("memory", Str), req("name", Str)]),
    read("memory.stats", Memory, &[opt("memory", Str)]),
    cmd(
        "notes.add",
        Memory,
        &[
            req("kind", Str),
            req("text", Str),
            opt("file", StrOrList),
            opt("title", Str),
            opt("line", U64),
            opt("why", Str),
            opt("project", Str),
        ],
    ),
    read("notes", Memory, &[req("project", Str)]),
    cmd("notes.delete", Memory, &[req("project", Str), req("ids", StrList)]),
    cmd("orchestrator.review", Messages, &[req("section", Str), req("target", Str)]),
    CommandSpec { name: "debug.sleep", area: Engine, fields: &[req("ms", U64)], faults_only: true, read: true },
];

/// Finds a command by name; `debug.sleep` only when `faults` is on.
pub fn find(name: &str, faults: bool) -> Option<&'static CommandSpec> {
    CATALOG.iter().find(|c| c.name == name && (faults || !c.faults_only))
}

/// Why a command is invalid, naming the field (1.6.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub field: String,
    pub message: String,
}

fn invalid(field: &str, message: String) -> Invalid {
    Invalid { field: field.to_owned(), message }
}

fn type_ok(v: &Value, ty: FieldType) -> bool {
    match ty {
        FieldType::Str => v.is_string(),
        FieldType::U64 => v.is_u64(),
        FieldType::Pick => v.is_u64() || v.is_string(),
        FieldType::Bool => v.is_boolean(),
        FieldType::StrList => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
        FieldType::StrOrList => v.is_string() || type_ok(v, FieldType::StrList),
        FieldType::Object => v.is_object(),
    }
}

/// The expected type, in words.
pub fn type_name(ty: FieldType) -> &'static str {
    match ty {
        FieldType::Str => "a string",
        FieldType::U64 => "a whole number from 0",
        FieldType::Pick => "a number from 1, or a name",
        FieldType::Bool => "a boolean",
        FieldType::StrList => "a list of strings",
        FieldType::StrOrList => "a string or a list of strings",
        FieldType::Object => "a JSON object",
    }
}

/// Formats the arguments shown beside a slash command in web command help.
pub fn slash_fields(spec: &CommandSpec) -> String {
    spec.fields
        .iter()
        .map(|f| {
            let list = if f.ty == FieldType::StrList { "…" } else { "" };
            if f.optional && f.ty == FieldType::Bool {
                format!("[--{}]", f.name)
            } else if f.optional {
                format!("[--{} <{}{list}>]", f.name, f.name)
            } else {
                format!("<{}{list}>", f.name)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A command's signature as `mork ctl` takes it: its name, its required fields in order, then
/// its optional ones. Help, the API index and the reference all show this one line (fac-14).
pub fn signature(spec: &CommandSpec) -> String {
    let mut line = format!("mork ctl {}", spec.name);
    for f in spec.fields {
        let t = type_name(f.ty);
        line.push_str(&if spec.name == "spawn" && f.name == "task" {
            format!(" (<task: {t}> | --task-file <path, or - for stdin>)")
        } else if f.optional && f.ty == FieldType::Bool {
            format!(" [--{}]", f.name)
        } else if f.optional {
            format!(" [--{} <{t}>]", f.name)
        } else {
            format!(" <{}: {t}>", f.name)
        });
    }
    line
}

/// Checks a command object (`{"<name>": {fields…}}`) against the catalog. An unknown
/// command, an unknown or missing field, or a value of the wrong type is refused.
pub fn validate(cmd: &Value, faults: bool) -> Result<&'static CommandSpec, Invalid> {
    let obj = cmd.as_object().ok_or_else(|| invalid("cmd", "the command must be an object".into()))?;
    if obj.len() != 1 {
        return Err(invalid("cmd", "the command must name exactly one command".into()));
    }
    let Some((name, body)) = obj.iter().next() else { return Err(invalid("cmd", "no command".into())) };
    let spec = find(name, faults).ok_or_else(|| invalid("cmd", format!("unknown command `{name}`")))?;
    let fields = body.as_object().ok_or_else(|| invalid(name, format!("`{name}` takes an object of fields")))?;
    for key in fields.keys() {
        if !spec.fields.iter().any(|f| f.name == key) {
            return Err(invalid(key, format!("`{name}` has no field `{key}`")));
        }
    }
    for f in spec.fields {
        match fields.get(f.name) {
            None if !f.optional => return Err(invalid(f.name, format!("`{name}` needs `{}`", f.name))),
            Some(v) if !type_ok(v, f.ty) => {
                return Err(invalid(f.name, format!("`{}` must be {}", f.name, type_name(f.ty))));
            }
            _ => {}
        }
    }
    Ok(spec)
}
