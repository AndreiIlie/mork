//! The door (fac-1): parameters are typed strictly per operation. `OPS` is where `mork ctl
//! door --help` comes from, and a test holds it equal to the types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `sql.select`: one read, run as written in a read-only session on any table its login may read,
/// its only change a LIMIT above the target's row cap lowered to the cap (fac-124).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlSelect {
    pub query: String,
}

/// `mongo.find`: a bounded find on any collection its read-only login may read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MongoFind {
    pub collection: String,
    #[serde(default)]
    pub filter: Map<String, Value>,
    #[serde(default)]
    pub projection: Map<String, Value>,
    #[serde(default)]
    pub sort: Map<String, Value>,
    #[serde(default)]
    pub limit: Option<u64>,
}

/// `http.get`: a GET at the configured API origin, through the target's whitelisted address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpGet {
    pub endpoint: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

/// `file.tail`: the last lines of a named log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileTail {
    pub log: String,
    #[serde(default)]
    pub lines: Option<u64>,
}

/// `file.cat`: a file under a named root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileCat {
    pub root: String,
    pub path: String,
}

/// `file.get`: a named-root file copied into the asking turn's `$MORK_IO`, or an orchestrator's private
/// `<mork home>/door/files/<call id>-<file name>`, kept 14 days (fac-173, fac-409).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileGet {
    pub root: String,
    pub path: String,
}

/// The most `file.get` copies in one call: a bigger file is refused, never cut (fac-173).
pub const FILE_GET_MAX: u64 = 20 * 1024 * 1024;

/// `sh.run`: a named command of the target, with its typed arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShRun {
    pub command: String,
    #[serde(default)]
    pub args: BTreeMap<String, Value>,
}

/// `misc.run`: any other command; it always comes to the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiscRun {
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    SqlSelect(SqlSelect),
    MongoFind(MongoFind),
    HttpGet(HttpGet),
    FileTail(FileTail),
    FileCat(FileCat),
    FileGet(FileGet),
    ShRun(ShRun),
    MiscRun(MiscRun),
}

/// Why a request isn't an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// No such operation: a new one is a code change, so a card.
    Unsupported(String),
    /// Its parameters aren't the operation's: what is wrong.
    Invalid(String),
}

fn typed<T: serde::de::DeserializeOwned>(op: &str, params: &Value) -> Result<T, OpError> {
    if !params.is_object() {
        return Err(OpError::Invalid(format!("`{op}` takes an object of parameters")));
    }
    serde_json::from_value(params.clone()).map_err(|e| OpError::Invalid(format!("`{op}`: {e}")))
}

impl Op {
    /// Reads `op` with its parameters, strictly: an unknown operation is unsupported, never a
    /// lesser one, and never the misc door.
    pub fn parse(op: &str, params: &Value) -> Result<Self, OpError> {
        Ok(match op {
            "sql.select" => Self::SqlSelect(typed(op, params)?),
            "mongo.find" => Self::MongoFind(typed(op, params)?),
            "http.get" => Self::HttpGet(typed(op, params)?),
            "file.tail" => Self::FileTail(typed(op, params)?),
            "file.cat" => Self::FileCat(typed(op, params)?),
            "file.get" => Self::FileGet(typed(op, params)?),
            "sh.run" => Self::ShRun(typed(op, params)?),
            "misc.run" => Self::MiscRun(typed(op, params)?),
            other => return Err(OpError::Unsupported(format!("no operation `{other}`: see mork ctl door --help"))),
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::SqlSelect(_) => "sql.select",
            Self::MongoFind(_) => "mongo.find",
            Self::HttpGet(_) => "http.get",
            Self::FileTail(_) => "file.tail",
            Self::FileCat(_) => "file.cat",
            Self::FileGet(_) => "file.get",
            Self::ShRun(_) => "sh.run",
            Self::MiscRun(_) => "misc.run",
        }
    }

    /// Its parameters as JSON, every key present.
    pub fn params(&self) -> Value {
        let v = match self {
            Self::SqlSelect(p) => serde_json::to_value(p),
            Self::MongoFind(p) => serde_json::to_value(p),
            Self::HttpGet(p) => serde_json::to_value(p),
            Self::FileTail(p) => serde_json::to_value(p),
            Self::FileCat(p) => serde_json::to_value(p),
            Self::FileGet(p) => serde_json::to_value(p),
            Self::ShRun(p) => serde_json::to_value(p),
            Self::MiscRun(p) => serde_json::to_value(p),
        };
        v.unwrap_or(Value::Null)
    }
}

/// JSON as one canonical text: keys sorted at every depth, no spaces. What an approval and a
/// natural key compare, whole.
pub fn canonical(v: &Value) -> String {
    // serde_json's map is ordered by key (no `preserve_order`), so its text is canonical.
    v.to_string()
}

/// One parameter of an operation, for its help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: &'static str,
    /// Its type, in words: `string`, `u64`, `object`, `string list`, `string map`, `typed map`.
    pub ty: &'static str,
    pub optional: bool,
    pub about: &'static str,
}

/// One operation, for its help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpSpec {
    pub name: &'static str,
    /// What it returns: `rows`, `text` or `json`.
    pub returns: &'static str,
    pub about: &'static str,
    pub params: &'static [ParamSpec],
}

const fn req(name: &'static str, ty: &'static str, about: &'static str) -> ParamSpec {
    ParamSpec { name, ty, optional: false, about }
}
const fn opt(name: &'static str, ty: &'static str, about: &'static str) -> ParamSpec {
    ParamSpec { name, ty, optional: true, about }
}

/// Every operation, with its parameters.
pub const OPS: &[OpSpec] = &[
    OpSpec {
        name: "sql.select",
        returns: "rows",
        about: "one read on any table the login may read, except information_schema and the sys schema, \
                run as written in a read-only session: \
                SELECT, WITH, TABLE, VALUES, SHOW, DESCRIBE or EXPLAIN; no write, INTO, locking read \
                (but under an EXPLAIN without ANALYZE) or second statement, in the query or an \
                unnumbered executable comment. Versioned executable comments are refused. Optimizer hints are \
                allowed only immediately after SELECT in any query block. A LIMIT over max_rows is \
                lowered to it; rows past max_rows are cut",
        params: &[req(
            "query",
            "string",
            "the read, as MySQL reads it; a string is written in single quotes, an apostrophe inside it doubled",
        )],
    },
    OpSpec {
        name: "mongo.find",
        returns: "json",
        about: "a bounded find on any collection its read-only login may read; no collection or operator allowlist",
        params: &[
            req("collection", "string", "any collection the login may read"),
            opt(
                "filter",
                "object",
                "a query document, including $expr; code-executing operators ($where, $function, $accumulator and internal JS equivalents) are refused; literal values are data",
            ),
            opt("projection", "object", "fields to include, each 1"),
            opt("sort", "object", "fields, each 1 or -1"),
            opt("limit", "u64", "at most this many documents"),
        ],
    },
    OpSpec {
        name: "http.get",
        returns: "json",
        about: "any GET path at the configured HTTPS origin using OAuth2 client credentials",
        params: &[
            req("endpoint", "string", "an endpoint alias/template or any GET path (`/…`); no approval ask"),
            opt(
                "params",
                "string map",
                "string values percent-encoded into template slots; all other keys and values become query parameters",
            ),
        ],
    },
    OpSpec {
        name: "file.tail",
        returns: "text",
        about: "the last lines of a named log",
        params: &[req("log", "string", "a named log"), opt("lines", "u64", "how many lines")],
    },
    OpSpec {
        name: "file.cat",
        returns: "text",
        about: "a regular file under a named root; no symlink, no `..`",
        params: &[req("root", "string", "a named root"), req("path", "string", "relative to the root")],
    },
    OpSpec {
        name: "file.get",
        returns: "file",
        about: "a regular file under a named root, copied byte for byte into your io/ folder, $MORK_IO, or for an orchestrator outside a turn into <mork home>/door/files/<call id>-<file name> (private, kept 14 days), up to 20 MB; no symlink, no `..`",
        params: &[req("root", "string", "a named root"), req("path", "string", "relative to the root")],
    },
    OpSpec {
        name: "sh.run",
        returns: "text",
        about: "a named command of the target, its arguments typed and passed as literal arguments",
        params: &[req("command", "string", "a named command"), opt("args", "typed map", "its arguments")],
    },
    OpSpec {
        name: "misc.run",
        returns: "text",
        about: "any other command, as an argv; it always comes to the operator, every time",
        params: &[req("argv", "string list", "the program and its arguments")],
    },
];

/// One output body (rows with columns, text, or JSON), truncation, and elapsed milliseconds.
/// Database answers additionally expose optional server microseconds as `server_us` (fac-92).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
    pub truncated: bool,
    /// Milliseconds the dispatcher took to run it, its connections included: the checks before
    /// it and the receipts after it aren't.
    pub elapsed: u64,
}

/// The door's answers' `status`.
pub mod status {
    pub const DONE: &str = "done";
    /// It ran and failed: the target's own words.
    pub const FAILED: &str = "failed";
    pub const ASK_RAISED: &str = "ask raised";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const NOT_ALLOWED: &str = "not allowed";
    /// Its outcome is unknown, and it is held for the operator: it is never run again blind.
    pub const HELD: &str = "held";
    /// Pending dispatch or running: the answer comes when it ends.
    pub const UNDER_WAY: &str = "under way";
    /// Its request waited 60 seconds without starting: nothing ran, and it may be asked again.
    pub const BUSY: &str = "busy";
}

/// The words that start a read's `failed` answer when its outcome is unclear: it is cut, and
/// asking again runs it as a new call.
pub const CUT: &str = "cut, ask again";

/// What an unclear outcome says of itself; a read's answer says the first half only.
pub const NEVER_BLIND: &str = "its outcome is unknown, and it is never run again blind";

/// Why a query cut at its deadline or cap may still run on its server, which didn't take the kill.
pub const UNSTOPPED: &str =
    "its query was cut at its deadline or cap and its server didn't take the kill: it may still run there";

/// Whether an operation only reads. Only a call that can change something (`sh.run`, `misc.run`)
/// is held for the operator when its outcome is unclear; a read fails and is asked again.
pub fn is_read(op: &str) -> bool {
    matches!(op, "sql.select" | "mongo.find" | "file.cat" | "file.get" | "file.tail" | "http.get")
}

/// A read stopped because its caller went (fac-183): its words begin so, and its result says
/// `caller_went`. It answers no later request: the same one asked again runs anew.
pub const CALLER_WENT: &str = "stopped: its caller went away";

/// The words an ask's requester is answered with.
pub const WAITING: &str = "waiting for the operator";

/// The one line every role file carries (fac-1, decision 11).
pub const ROLE_LINE: &str = "For anything outside your wall, see `mork ctl door --help`; misc.run asks the operator.";

/// `mork ctl door --help`, made from `OPS`.
pub fn help() -> String {
    let mut out = String::from(
        "mork ctl door --target <target> --op <op> --params '<JSON object>'\n\n\
         mork ctl door --target staging --op sql.select --params \"$(cat <<'JSON'\n\
         {\"query\": \"SELECT id FROM app.users WHERE name = 'O''Neil'\"}\n\
         JSON\n\
         )\"\n\n\
         Acts on a machine outside your wall, through the engine. A resolved call the target's\n\
         allowlist permits runs directly, and the answer is\n\
         {\"door\": {\"status\": \"done\", rows|text|json, \"truncated\", \"elapsed\" (ms)}}.\n\
         Calls run one at a time per target, in ask order. A queued request gets a waiting line\n\
         on stderr; its window step also says it waits. The line names the running call's action id,\n\
         requester, op, elapsed milliseconds and time limit, and how many calls wait ahead; never its parameters.\n\
         Each request waits at most 60 seconds before its call starts, then answers `busy`: its `why`\n\
         carries those same facts, says nothing ran and says to ask again. Requests sharing a call\n\
         each get their own waiting line and 60 seconds; the call is dropped when its last waiter times out.\n\
         A queued read whose last caller disconnects (a restart included) is dropped at once. A started call keeps running,\n\
         except a sql.select, which is stopped when its last caller disconnects. A busy or caller-gone\n\
         call answers no later request: asking again makes a new call, in the same turn too.\n\
         A queued non-read call nobody waits on, including an approved misc.run or a sh.run pending across a restart,\n\
         has no queue time limit and runs when its target is free.\n\
         Direct calls never post news when their caller disconnects; a same-turn retry still gets a finished call's answer.\n\
         An outside call, including every misc.run, raises the operator's ask (`ask raised`, waiting for\n\
         the operator); his `once` runs it once, his `no` refuses it, and the outcome arrives in your next\n\
         turn's news (as does a held call's outcome). An op not listed here is `unsupported`; what no adapter may do is `not allowed`.\n\
         A call is never repeated blind: the same call in the same turn is the same call. A read whose outcome is\n\
         unclear answers `failed`, \"cut, ask again\": asking again runs it anew; an unclear sh.run or misc.run is `held`.\n\
         A sql.select whose server didn't take its kill says it may still be running there: the same query is\n\
         `not allowed` until it has ended (each ask tries the kill again); your other calls run.\n\
         \"elapsed\" is the run's, its connections (ssh, the client's login) included, not the engine's bookkeeping;\n\
         a database's answer also has \"server_us\": the statement's own microseconds on its server\n\
         (divide by 1000 to set it beside \"elapsed\"), null when it can't be told: unknown, not cheap.\n\
         A sql.select answer cut at the target's size cap has null \"server_us\" and \"server_us_why\":\n\
         the answer was cut at that cap before the query's time could be read from its server.\n\
         A read that completed and was only trimmed afterwards keeps its server time.\n\
         Calls run as the operator's own logins (his ssh aliases, his accounts on the services), which\n\
         could do more: Mork holds them to what is listed here.\n\
         mork ctl door --target <target> --help lists a target's services, logs and commands.\n\nOperations:\n",
    );
    for op in OPS {
        out.push_str(&format!("  {} → {}: {}\n", op.name, op.returns, op.about));
        for p in op.params {
            let o = if p.optional { ", optional" } else { "" };
            out.push_str(&format!("      {} ({}{o}): {}\n", p.name, p.ty, p.about));
        }
    }
    out
}
