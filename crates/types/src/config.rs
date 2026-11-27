//! The config, `mork.toml` (1.14), with the project table of 4.1 and the seats of 3.1.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod bench;
mod seats;

pub use crate::job::SliceConfig;
pub use crate::ports::Ports;
pub use seats::{
    AGY_3P_BUCKET, AGY_PRICES, CLAUDE_BLOCKED, CODEX_PRICES, Cli, Effort, MODEL_CATALOG, ModelOverride,
    OPERATOR_MODELS, OPUS, PickModel, Picks, ROSTER, ReviewKind, ReviewPicks, SEAT_DEFAULTS, SOL, Seat, SeatOverride,
    SeatPhase, Standing, Wall, agy_bucket, barred, cli_of_model, foreman_seat, moved_to_codex,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub mork: MorkConfig,
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
pub struct MorkConfig {
    #[serde(default)]
    pub project: Option<crate::ProjectId>,
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
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "d_port_range")]
    pub port_range: String,
    #[serde(default)]
    pub toolchains: Vec<Toolchain>,
    #[serde(default)]
    pub caches: Vec<String>,
    #[serde(default)]
    pub cache_files: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "d_poll")]
    pub poll_seconds: u64,
    /// The folder the operator installed Vite and sass in with npm (fac-258): the visual cards' mock host.
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
    #[serde(default = "d_fresh_context_percent")]
    pub fresh_context_percent: u8,
    /// Extra skills folder merged with the built-in skills at start (fac-721).
    #[serde(default)]
    pub skills: Option<String>,
}

/// Default, minimum, and maximum non-zero `[mork] context_cap` values in tokens (fac-681).
pub const CONTEXT_CAP_DEFAULT: u64 = 200_000;
pub const CONTEXT_CAP_MIN: u64 = 100_000;
pub const CONTEXT_CAP_MAX: u64 = 1_000_000;

impl MorkConfig {
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
    #[serde(default)]
    pub hide: bool,
    #[serde(default = "d_words")]
    pub words: Vec<String>,
}

impl Default for Internals {
    fn default() -> Self {
        Self { hide: false, words: d_words() }
    }
}

impl Config {
    pub fn project_git_author(&self, project: Option<&str>) -> Option<&str> {
        self.projects.get(project?)?.git_author.as_deref()
    }

    pub fn git_author(&self, project: Option<&str>) -> &str {
        self.project_git_author(project).unwrap_or(&self.git.author)
    }

    pub fn forge_label(&self, project: &crate::ProjectId) -> &'static str {
        self.projects.get(project).filter(|p| p.forge.hosted()).map_or("the forge", |p| p.forge.label())
    }

    /// Configured project names in file order, then any omitted names alphabetically.
    pub fn ordered_projects(&self) -> impl Iterator<Item = &crate::ProjectId> {
        let rest = self.projects.keys().filter(|name| !self.project_order.contains(name));
        self.project_order.iter().filter(|name| self.projects.contains_key(*name)).chain(rest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BoardConfig {
    Markdown { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeMode {
    #[default]
    Manual,
    Approval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColleagueReview {
    #[default]
    Standard,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ForgeConfig {
    Local {
        backup: Option<String>,
    },
    Github {
        owner: String,
        repo: String,
        token: String,
        approver: Option<String>,
        review_tag: Option<String>,
        #[serde(default)]
        merge: MergeMode,
    },
    Bitbucket {
        workspace: String,
        slug: String,
        review_tag: Option<String>,
        #[serde(default)]
        merge: MergeMode,
    },
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self::Local { backup: None }
    }
}

impl ForgeConfig {
    pub fn merges_on_approval(&self) -> bool {
        matches!(
            self,
            Self::Bitbucket { merge: MergeMode::Approval, .. } | Self::Github { merge: MergeMode::Approval, .. }
        )
    }

    pub fn hosted(&self) -> bool {
        !matches!(self, Self::Local { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::Bitbucket { .. } => "Bitbucket",
            Self::Github { .. } => "GitHub",
        }
    }

    pub fn skill(&self) -> &'static str {
        match self {
            Self::Local { .. } => "joins-and-the-train",
            Self::Bitbucket { .. } | Self::Github { .. } => "pull-requests",
        }
    }

    pub fn repo_key(&self) -> Option<String> {
        match self {
            Self::Local { .. } => None,
            Self::Bitbucket { workspace, slug, .. } => Some(format!("{workspace}/{slug}")),
            Self::Github { owner, repo, .. } => Some(format!("github.com/{owner}/{repo}")),
        }
    }

    pub fn pr_link(&self, pr: u64) -> Option<String> {
        match self {
            Self::Local { .. } => None,
            Self::Bitbucket { workspace, slug, .. } => {
                Some(format!("https://bitbucket.org/{workspace}/{slug}/pull-requests/{pr}"))
            }
            Self::Github { owner, repo, .. } => Some(format!("https://github.com/{owner}/{repo}/pull/{pr}")),
        }
    }

    pub fn has_review_trigger(&self) -> bool {
        self.review_tag().is_some()
    }

    pub fn review_tag(&self) -> Option<&str> {
        match self {
            Self::Bitbucket { review_tag, .. } | Self::Github { review_tag, .. } => review_tag.as_deref(),
            Self::Local { .. } => None,
        }
    }

    pub fn token_path(&self) -> Option<&str> {
        match self {
            Self::Github { token, .. } => Some(token),
            _ => None,
        }
    }

    pub fn backup(&self) -> Option<&String> {
        match self {
            Self::Local { backup } => backup.as_ref(),
            Self::Bitbucket { .. } | Self::Github { .. } => None,
        }
    }

    pub fn project_view(&self) -> serde_json::Value {
        match self {
            Self::Local { .. } => serde_json::json!({"kind": "local"}),
            Self::Github { owner, repo, .. } => serde_json::json!({"kind":"github", "owner":owner, "repo":repo}),
            Self::Bitbucket { workspace, slug, .. } => {
                serde_json::json!({"kind":"bitbucket", "workspace":workspace, "slug":slug})
            }
        }
    }
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
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TicketsConfig {
    AzureDevops { api: String, page: String, token: String },
}

impl TicketsConfig {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AzureDevops { .. } => "Azure DevOps",
        }
    }
    pub fn max_id(&self) -> u64 {
        match self {
            Self::AzureDevops { .. } => i32::MAX as u64,
        }
    }
    pub fn api(&self) -> &str {
        match self {
            Self::AzureDevops { api, .. } => api,
        }
    }
    pub fn page(&self) -> &str {
        match self {
            Self::AzureDevops { page, .. } => page,
        }
    }
    pub fn token(&self) -> &str {
        match self {
            Self::AzureDevops { token, .. } => token,
        }
    }
}

/// `[projects.<name>]` (4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub passes: BTreeMap<crate::lookback::Read, crate::lookback::PassConfig>,
    pub repo: String,
    pub git_author: Option<String>,
    #[serde(default = "d_base")]
    pub base: String,
    #[serde(default)]
    pub forge: ForgeConfig,
    #[serde(default)]
    pub colleague_review: ColleagueReview,
    pub board: BoardConfig,
    #[serde(default)]
    pub statuses: Statuses,
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default)]
    pub install: Option<String>,
    /// A long-lived shell command each full card's clone serves its preview with (fac-196), run
    /// after `install` in a wall of its own, `MORK_PORT` its card's port; none, no preview.
    #[serde(default)]
    pub preview: Option<String>,
    pub test: String,
    /// The command `mork test --file <path>…` runs, the paths appended to it as the checks' are
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
    pub domains: Vec<String>,
    #[serde(default)]
    pub host_ports: Vec<Ports>,
    #[serde(default)]
    pub toolchains: Vec<Toolchain>,
    #[serde(default = "d_slots")]
    pub slots: u64,
    /// A folder of datasets, bound read-only at `/data` in its cards' walls when it exists (fac-173).
    pub data: Option<String>,
    /// The Claude Code session a haiku wakes for this project (fac-265).
    #[serde(default)]
    pub orchestrator_session: Option<String>,
    /// Whether this project's orchestrator reads queued wakes with `mork ctl wakes` (fac-663).
    #[serde(default)]
    pub orchestrator_waits: bool,
    /// Issue-tracker URLs for a project whose cards and PRs track external tickets (fac-721).
    #[serde(default)]
    pub tickets: Option<TicketsConfig>,
}

impl ProjectConfig {
    /// Every port its `ports` names, a range's each, in order (4.1, 1.10.10).
    pub fn port_list(&self) -> Vec<u16> {
        self.ports.iter().flat_map(|p| p.from..=p.to).collect()
    }
}

/// The keys each table must have (1.14, 4.1): the table, then its keys.
pub const REQUIRED: &[(&str, &[&str])] = &[("mork", &[]), ("git", &["author"])];
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
    let mut w: Vec<String> = ["Claude", "Codex", "Anthropic", "OpenAI", "foreman", "fable", "astra"]
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
fn d_fresh_context_percent() -> u8 {
    70
}

fn d_context_cap() -> u64 {
    CONTEXT_CAP_DEFAULT
}
