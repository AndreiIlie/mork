//! Strict engine-only target configuration, with credential references instead of secret values.
//! Targets use the operator's own access (fac-1, fac-86, fac-124).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const TARGETS_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetsFile {
    pub version: u32,
    /// The operator's access file, an absolute path: the `[access:<name>]` sections credentials are
    /// read from, by the engine alone, as a call is dispatched.
    #[serde(default)]
    pub access: Option<String>,
    #[serde(default)]
    pub defaults: Limits,
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
}

/// `[defaults]`: every call's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default = "d_bytes")]
    pub max_bytes: u64,
    #[serde(default = "d_rows")]
    pub max_rows: u64,
    #[serde(default = "d_rows")]
    pub max_documents: u64,
    #[serde(default = "d_lines")]
    pub max_lines: u64,
    #[serde(default = "d_op_ms")]
    pub timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: d_bytes(),
            max_rows: d_rows(),
            max_documents: d_rows(),
            max_lines: d_lines(),
            timeout_ms: d_op_ms(),
        }
    }
}

/// A target's own bounds: each may only lower the default, but on a read-only server's SQL
/// (`max_rows` there is the only cap; `[targets.<id>.sql] timeout_ms` its timeout).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsLowered {
    pub max_bytes: Option<u64>,
    pub max_rows: Option<u64>,
    pub max_documents: Option<u64>,
    pub max_lines: Option<u64>,
    pub timeout_ms: Option<u64>,
}

impl Limits {
    /// These bounds, lowered by a target's own.
    pub fn lowered(self, by: LimitsLowered) -> Self {
        let low = |d: u64, t: Option<u64>| t.map_or(d, |t| t.min(d));
        Self {
            max_bytes: low(self.max_bytes, by.max_bytes),
            max_rows: low(self.max_rows, by.max_rows),
            max_documents: low(self.max_documents, by.max_documents),
            max_lines: low(self.max_lines, by.max_lines),
            timeout_ms: low(self.timeout_ms, by.timeout_ms),
        }
    }
}

/// How a target is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// On this machine.
    Local,
    /// Through the operator's own ssh alias, as its login: commands run there with the box's own tools,
    /// services are reached through a forward of that connection.
    Ssh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub description: String,
    pub transport: Transport,
    #[serde(default)]
    pub ssh: Option<Ssh>,
    #[serde(default)]
    pub limits: LimitsLowered,
    #[serde(default)]
    pub sql: Option<Sql>,
    #[serde(default)]
    pub mongo: Option<Mongo>,
    #[serde(default)]
    pub http: Option<Http>,
    #[serde(default)]
    pub file: Option<Files>,
    #[serde(default)]
    pub sh: BTreeMap<String, ShCommand>,
    /// The misc door: any command over the target's alias, which the operator approves each time.
    #[serde(default)]
    pub misc: bool,
}

/// `[targets.<id>.ssh]`: one of the operator's own ssh aliases. Its host, login and keys are his ssh
/// configuration's; Mork names none of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ssh {
    pub alias: String,
}

/// A login in the access file: its section, and the names of the fields that hold the user and
/// the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    pub section: String,
    #[serde(default = "d_user")]
    pub user: String,
    #[serde(default = "d_password")]
    pub password: String,
}

/// An API client in the access file: its section, and the names of the fields that hold the key
/// and the secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub section: String,
    #[serde(default = "d_key")]
    pub key: String,
    #[serde(default = "d_secret")]
    pub secret: String,
}

/// `[targets.<id>.sql]`: MySQL, read by the `mysql` client on this machine. `host` and `port`
/// are the server's as the target sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sql {
    pub login: Login,
    #[serde(default = "d_host")]
    pub host: String,
    #[serde(default = "d_mysql")]
    pub port: u16,
    pub database: String,
    /// The operator's declaration that this SQL server refuses writes (fac-92): only the target's own
    /// `limits.max_rows` caps its rows, and its `timeout_ms` may pass the default.
    #[serde(default)]
    pub read_only: bool,
    /// A read-only server's SQL timeout, above `[defaults]` if need be: up to an hour.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Target-specific schema guidance shown in `door.help` and appended to schema refusals (fac-721).
    #[serde(default)]
    pub schema_hint: Option<String>,
}

/// `[targets.<id>.mongo]`: `find`, by `mongoexport` on this machine. `host` and `port` are the
/// server's as the target sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mongo {
    pub login: Login,
    #[serde(default = "d_host")]
    pub host: String,
    #[serde(default = "d_mongo")]
    pub port: u16,
    pub database: String,
    /// The database the login is defined in, when it isn't `database`.
    #[serde(default)]
    pub auth_database: Option<String>,
}

/// `[targets.<id>.http]`: an HTTPS API reached through the target. OAuth2 client credentials are exchanged
/// for a bearer token at the same origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    pub client: Client,
    /// `https://<host>:<port>`: the token and the API both.
    pub origin: String,
    #[serde(default = "d_token_path")]
    pub token_path: String,
    pub scope: Option<String>,
    #[serde(default)]
    pub endpoints: BTreeMap<String, Endpoint>,
}

/// An HTTP GET alias: `{name}` parameters fill its path; all other parameters form its query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    #[serde(default)]
    pub about: String,
    pub path: String,
}

/// `[targets.<id>.file]`: named roots to read under, named logs to tail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Files {
    #[serde(default)]
    pub roots: BTreeMap<String, String>,
    #[serde(default)]
    pub logs: BTreeMap<String, String>,
}

/// `[targets.<id>.sh.<name>]`: a fixed argv; an element `{arg}` is replaced, whole, by that
/// typed argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShCommand {
    #[serde(default)]
    pub about: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub args: BTreeMap<String, ArgType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgType {
    /// Letters, digits and `_.@:-`, up to 128.
    Word,
    /// A whole number from 0.
    U64,
    /// `YYYY-MM-DD`.
    Date,
    /// Any printable text up to 4096, passed as one literal argument.
    Path,
    /// Any printable text up to 1024, passed as one literal argument.
    Text,
}

impl ArgType {
    pub fn word(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::U64 => "u64",
            Self::Date => "date",
            Self::Path => "path",
            Self::Text => "text",
        }
    }
}

fn d_bytes() -> u64 {
    65_536
}
fn d_rows() -> u64 {
    100
}
fn d_lines() -> u64 {
    200
}
fn d_op_ms() -> u64 {
    30_000
}
fn d_user() -> String {
    "user".into()
}
fn d_password() -> String {
    "password".into()
}
fn d_key() -> String {
    "key".into()
}
fn d_secret() -> String {
    "secret".into()
}
fn d_host() -> String {
    "127.0.0.1".into()
}
fn d_mysql() -> u16 {
    3306
}
fn d_mongo() -> u16 {
    27017
}
fn d_token_path() -> String {
    "/oauth2/token".into()
}
