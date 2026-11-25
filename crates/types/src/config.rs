//! The config, `factory.toml` (1.14), with the project table of 4.1 and the seats of 3.1.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod seats;

pub use crate::job::SliceConfig;
pub use crate::ports::Ports;
pub use seats::{
    AGY_3P_BUCKET, AGY_PRICES, ANDREI_MODELS, CLAUDE_BLOCKED, CODEX_PRICES, Cli, Effort, MODEL_CATALOG, ModelOverride,
    OPUS, PickModel, Picks, ROSTER, ReviewKind, ReviewPicks, SEAT_DEFAULTS, SOL, Seat, SeatOverride, SeatPhase,
    Standing, Wall, agy_bucket, barred, cli_of_model, foreman_seat, moved_to_codex,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub factory: FactoryConfig,
    pub git: GitConfig,
    #[serde(default)]
    pub internals: Internals,
    #[serde(default)]
    pub slice: SliceConfig,
    #[serde(default)]
    pub seats: BTreeMap<String, SeatOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picks: Option<Picks>,
    #[serde(default)]
    pub projects: BTreeMap<crate::ProjectId, ProjectConfig>,
    /// Saved Claude accounts: `<name> = "<pool header>"` (fac-721).
    #[serde(default)]
    pub claude_accounts: BTreeMap<String, String>,
    /// Window-owned SSH web forwards, keyed by name (fac-721).
    #[serde(default)]
    pub tunnels: BTreeMap<String, TunnelConfig>,
    /// The projects in the file's order, for every project list; empty, their names' order.
    #[serde(skip)]
    pub project_order: Vec<crate::ProjectId>,
}

/// A toolchain bound read-only into every wall; `bin`, a folder inside it, joins `PATH`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    pub path: String,
    #[serde(default)]
    pub bin: Option<String>,
    #[serde(default)]
    pub hide: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactoryConfig {
    pub project: crate::ProjectId,
    #[serde(default = "d_editor")]
    pub editor: String,
    #[serde(default = "d_true")]
    pub animations: bool,
    #[serde(default = "d_true")]
    pub notifications: bool,
    #[serde(default = "d_max_test_runs")]
    pub max_test_runs: u64,
    #[serde(default = "d_ports")]
    pub ports: Vec<u16>,
    #[serde(default = "d_port_range")]
    pub port_range: String,
    #[serde(default)]
    pub toolchains: Vec<Toolchain>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "d_poll")]
    pub poll_seconds: u64,
    /// The folder Andrei installed Vite and sass in with npm (fac-258): the visual cards' mock host.
    #[serde(default)]
    pub vite: Option<String>,
    /// Loopback TCP port for the local web bridge (fac-671); none disables the bridge, 0 picks an ephemeral port.
    #[serde(default)]
    pub web_port: Option<u16>,
    /// Folder of built static web UI files served by the local web bridge (fac-671); none serves no pages.
    #[serde(default)]
    pub web_assets: Option<String>,
    /// Shared auto-compaction context cap in tokens for Claude and agy seats (fac-681); 0 disables the cap.
    #[serde(default = "d_context_cap")]
    pub context_cap: u64,
    /// Extra skills folder merged with the built-in skills at start (fac-721).
    #[serde(default)]
    pub skills: Option<String>,
}

/// Default, minimum, and maximum non-zero `[factory] context_cap` values in tokens (fac-681).
pub const CONTEXT_CAP_DEFAULT: u64 = 200_000;
pub const CONTEXT_CAP_MIN: u64 = 100_000;
pub const CONTEXT_CAP_MAX: u64 = 1_000_000;

impl FactoryConfig {
    /// Active auto-compaction context cap in tokens, or none when disabled (`0`) (fac-681).
    pub fn context_cap(&self) -> Option<u64> {
        (self.context_cap > 0).then_some(self.context_cap)
    }

    /// Displayed context window denominator: the cap on agy, or `min(reported, cap)` on Claude (fac-681).
    pub fn effective_window(&self, reported: Option<u64>) -> Option<u64> {
        match (reported, self.context_cap()) {
            (Some(w), Some(cap)) => Some(w.min(cap)),
            (None, Some(cap)) => Some(cap),
            (reported, None) => reported,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitConfig {
    pub author: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Internals {
    #[serde(default = "d_words")]
    pub words: Vec<String>,
}

impl Default for Internals {
    fn default() -> Self {
        Self { words: d_words() }
    }
}

impl Config {
    /// Configured project names in file order, then any omitted names alphabetically.
    pub fn ordered_projects(&self) -> impl Iterator<Item = &crate::ProjectId> {
        let rest = self.projects.keys().filter(|name| !self.project_order.contains(name));
        self.project_order.iter().filter(|name| self.projects.contains_key(*name)).chain(rest)
    }
}

/// `{ workspace, slug }` of a Bitbucket project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BitbucketRepo {
    pub workspace: String,
    pub slug: String,
}

/// `statuses`: the board statuses a started and a merged card move to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statuses {
    #[serde(default = "d_started")]
    pub started: String,
    #[serde(default = "d_merged")]
    pub merged: String,
}

impl Default for Statuses {
    fn default() -> Self {
        Self { started: d_started(), merged: d_merged() }
    }
}

/// `checks`: the checks' command and their folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checks {
    pub command: String,
    pub folder: String,
}

/// `[tunnels.<name>]`: a window-owned SSH web forward (fac-721).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelConfig {
    pub alias: String,
    pub port: u16,
    pub remote_port: u16,
    pub url: String,
}

/// `[projects.<name>.tickets]`: a project's issue-tracker API and ticket page URL prefix (fac-721).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketsConfig {
    pub api: String,
    pub page: String,
}

/// `[projects.<name>]` (4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub repo: String,
    #[serde(default = "d_base")]
    pub base: String,
    #[serde(default)]
    pub bitbucket: Option<BitbucketRepo>,
    /// A local project's remote for non-forced pushes of the base after each landing.
    #[serde(default)]
    pub backup: Option<String>,
    pub board: String,
    #[serde(default)]
    pub statuses: Statuses,
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default)]
    pub install: Option<String>,
    /// A long-lived shell command each full card's clone serves its preview with (fac-196), run
    /// after `install` in a wall of its own, `FACTORY_PORT` its card's port; none, no preview.
    #[serde(default)]
    pub preview: Option<String>,
    pub test: String,
    /// The command `factory test --file <path>…` runs, the paths appended to it as the checks' are
    /// (fac-154); none, and `--file` is refused. The merge train never uses it.
    #[serde(default)]
    pub test_file: Option<String>,
    /// The suite's exits that say its infrastructure failed, not its code (a test database down):
    /// the merge queue holds on them for `/resume`, as on 126, 127 or a signal (fac-10).
    #[serde(default)]
    pub infra_exits: Vec<u8>,
    #[serde(default)]
    pub checks: Option<Checks>,
    /// Ports its walls may reach besides the config's: numbers and `"<from>-<to>"` ranges (4.1).
    #[serde(default)]
    pub ports: Vec<Ports>,
    #[serde(default)]
    pub toolchains: Vec<Toolchain>,
    #[serde(default = "d_slots")]
    pub slots: u64,
    /// A folder of datasets, bound read-only at `/data` in its cards' walls when it exists (fac-173).
    pub data: Option<String>,
    /// The Claude Code session a haiku wakes for this project (fac-265).
    #[serde(default)]
    pub orchestrator_session: Option<String>,
    /// Whether this project's orchestrator reads queued wakes with `factory ctl wakes` (fac-663).
    #[serde(default)]
    pub orchestrator_waits: bool,
    /// Issue-tracker URLs for a project whose cards and PRs track external tickets (fac-721).
    #[serde(default)]
    pub tickets: Option<TicketsConfig>,
    /// Pull request title tag that triggers automatic review into any destination branch (fac-721).
    #[serde(default)]
    pub review_tag: Option<String>,
}

impl ProjectConfig {
    /// Every port its `ports` names, a range's each, in order (4.1, 1.10.10).
    pub fn port_list(&self) -> Vec<u16> {
        self.ports.iter().flat_map(|p| p.from..=p.to).collect()
    }
}

/// The keys each table must have (1.14, 4.1): the table, then its keys.
pub const REQUIRED: &[(&str, &[&str])] = &[("factory", &["project"]), ("git", &["author"])];
/// A project's required keys.
pub const PROJECT_REQUIRED: &[&str] = &["repo", "board", "test"];

fn d_editor() -> String {
    "code -g {file}:{line}".into()
}
fn d_true() -> bool {
    true
}
fn d_max_test_runs() -> u64 {
    3
}
fn d_ports() -> Vec<u16> {
    vec![443, 80, 53]
}
fn d_port_range() -> String {
    "41000-41999".into()
}
fn d_poll() -> u64 {
    60
}

fn d_words() -> Vec<String> {
    let mut w: Vec<String> = ["Claude", "Codex", "Anthropic", "OpenAI", "the factory", "foreman", "fable", "astra"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let seats = SEAT_DEFAULTS.iter().filter(|s| s.0 != "echo").map(|s| s.3);
    for label in seats.chain(MODEL_CATALOG.iter().map(|m| m.2)) {
        if !w.iter().any(|x| x == label) {
            w.push(label.to_owned());
        }
    }
    w
}
fn d_started() -> String {
    "In Progress".into()
}
fn d_merged() -> String {
    "Done".into()
}
fn d_base() -> String {
    "master".into()
}
fn d_slots() -> u64 {
    2
}
fn d_context_cap() -> u64 {
    CONTEXT_CAP_DEFAULT
}
