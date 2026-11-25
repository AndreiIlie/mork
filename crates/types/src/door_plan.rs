//! Request values travel as data, never as shell text; an approval binds to the whole envelope
//! (fac-1). A plan names a credential, never its values.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::door_config::{Client, Login};

/// The dispatcher's wire version.
pub const WIRE: u32 = 2;

/// One executable step, as the dispatcher runs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Plan {
    /// `mysql --batch --database=<database>` as the login, the query on stdin, at `host:port` as
    /// the target sees it.
    Sql { login: Login, host: String, port: u16, database: String, query: String },
    /// `mongoexport` as the login, a find of `limit` documents, at `host:port` as the target sees
    /// it.
    Mongo {
        login: Login,
        host: String,
        port: u16,
        database: String,
        auth_database: Option<String>,
        collection: String,
        query: Value,
        fields: Vec<String>,
        sort: Value,
        limit: u64,
    },
    /// A GET of `path` (its query included) at `origin`, with a token for the client's key and
    /// secret: both requests by `curl`, through the target.
    Libra { client: Client, origin: String, token_path: String, scope: String, path: String },
    /// A regular file under a root, opened beneath it: no symlink, no `..`.
    Read { root: String, path: String },
    /// A regular file under a root, opened as `Read` is, copied whole into the request's `into`.
    Get { root: String, path: String },
    /// The last `lines` lines of a log.
    Tail { path: String, lines: u64 },
    /// An argv: a named command's, or the misc door's.
    Run { argv: Vec<String> },
}

impl Plan {
    /// Which permission of the route's it needs.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sql { .. } => "sql",
            Self::Mongo { .. } => "mongo",
            Self::Libra { .. } => "libra",
            Self::Read { .. } | Self::Get { .. } | Self::Tail { .. } => "file",
            Self::Run { .. } => "sh",
        }
    }
}

/// Where a plan runs, by the engine's own dispatcher, allowed these kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "via", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Route {
    /// On this machine.
    Local { allow: Vec<String> },
    /// Through Andrei's own ssh alias, as its configured login: a command runs there, a service
    /// is reached through a forward of that connection. `misc`: the misc door's any command.
    Ssh { alias: String, allow: Vec<String>, misc: bool },
}

impl Route {
    pub fn allow(&self) -> &[String] {
        match self {
            Self::Local { allow } | Self::Ssh { allow, .. } => allow,
        }
    }
}

/// A credential's two values, sent only on the dispatcher's stdin as the call is dispatched.
/// Its `Debug` shows neither.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    pub name: String,
    pub secret: String,
}

impl std::fmt::Debug for Pair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pair(‹secret›)")
    }
}

/// A call's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub max_bytes: u64,
    /// Rows, documents or lines kept; one more is read, to know it was cut.
    pub max_items: u64,
    pub timeout_ms: u64,
}

/// Everything a call is: what an approval binds to, compared whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub target: String,
    pub route: Route,
    pub plan: Plan,
    pub bounds: Bounds,
}

/// The line the dispatcher reads: a plan to run, or a question about a call's receipt.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchRequest {
    pub v: u32,
    /// The call's id: the engine's nonce and the action, so a receipt is this call's alone.
    pub call: String,
    #[serde(default)]
    pub route: Option<Route>,
    #[serde(default)]
    pub plan: Option<Plan>,
    /// Where a `Get` writes its file: a new one, in the asking turn's `$FACTORY_IO`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
    /// The plan's credential, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<Pair>,
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Asks for the receipt of `call` instead of running anything.
    #[serde(default)]
    pub receipt: bool,
    /// The engine's own secret values and the access file's, so the receipt is masked before it
    /// is written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redact: Vec<String>,
    /// The receipts store a call was sent to, when a receipt is asked of it: only that store's
    /// word that it never got the call counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
}

impl std::fmt::Debug for DispatchRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DispatchRequest")
            .field("v", &self.v)
            .field("call", &self.call)
            .field("route", &self.route)
            .field("plan", &self.plan)
            .field("credential", &self.credential)
            .field("bounds", &self.bounds)
            .field("receipt", &self.receipt)
            .field("redact", &self.redact.len())
            .field("store", &self.store)
            .finish()
    }
}

/// What the dispatcher ran: its exit, its output (already cut and redacted), and how long it took.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ran {
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub timed_out: bool,
    pub elapsed: u64,
    /// A SQL query cut at its deadline or a cap: whether its server took the kill. `false`: it
    /// may still run there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<bool>,
    /// Its ssh connection ended with no conclusive result: it may have run there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lost: bool,
    /// A SQL query's own time on its server, in microseconds: MySQL's profile of the statement,
    /// sending its rows included (fac-92). None when it didn't end whole there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_us: Option<u64>,
    /// The SQL client was cut at its output cap before timing could be read (fac-424).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_at_cap: Option<bool>,
    /// A SQL read's server session, as it was recorded before its query ran (fac-183).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
}

/// A read's server session, recorded before its query runs (fac-183); past `expires_ms` (UTC)
/// the connection id may name another's connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub session: u64,
    pub limit_ms: u64,
    pub until_ms: i64,
}

/// The dispatcher's answer line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchAnswer {
    pub v: u32,
    #[serde(default)]
    pub ran: Option<Ran>,
    /// It ran nothing: why.
    #[serde(default)]
    pub refused: Option<String>,
    /// A receipt asked for: whether the call reached the dispatcher at all. `false` is the
    /// dispatcher's durable word that it never ran.
    #[serde(default)]
    pub received: Option<bool>,
    /// Its receipts store's identity, made with the store: one made again is another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    /// A receipt asked for with its read's route, plan and credential: whether the read's recorded
    /// server session was killed or already gone (`false`: it may still run), or none recorded (fac-183).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<bool>,
    /// That read's recorded session, killed or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
}
