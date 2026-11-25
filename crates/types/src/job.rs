//! A job's records: the files of its folder (1.4.2).

use crate::ids::{AgentKey, JobId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An ordinary job's format (`job.json`'s `format`), the one every build reads.
pub const JOB_FORMAT: u32 = 1;

/// The job formats this build can run.
pub const JOB_FORMATS: [u32; 1] = [JOB_FORMAT];

/// The kinds of job (1.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Turn,
    Check,
    Build,
    Bundle,
    Install,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Check => "check",
            Self::Build => "build",
            Self::Bundle => "bundle",
            Self::Install => "install",
        }
    }
}

/// One mount of a wall, in the order bwrap applies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Mount {
    /// The path, read-only, at the same place.
    Ro { path: String },
    /// The path, read-only, if it exists.
    RoTry { path: String },
    /// The path, read-write, at the same place.
    Rw { path: String },
    /// A source bound read-only at another place (`/run/credentials`).
    RoAt { src: String, dest: String },
    /// A source bound read-only at another place, if it exists (`/data`).
    RoTryAt { src: String, dest: String },
    /// An empty tmpfs.
    Tmpfs { path: String },
    /// A minimal `/dev`.
    Dev { path: String },
    /// A fresh `/proc`.
    Proc { path: String },
    /// A symbolic link, as the host has it (`/bin -> usr/bin`).
    Symlink { target: String, path: String },
    /// Remounts a path read-only once every bind is in place: the wall's own root, so a job
    /// writes only to the folders bound into it (1.4.2.2).
    RemountRo { path: String },
}

/// How a job is walled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallMode {
    /// The whole wall: mounts and every Landlock rule (ABI 6 or more).
    Full,
    /// Mounts, and whatever Landlock rules the kernel has (`FACTORY_TEST_WALL=partial`).
    Partial,
    /// No wall at all: a seat configured `wall = "none"` (fac-25), or every job with the tests'
    /// `FACTORY_UNWALLED=1`.
    None,
}

/// `job.json`: written by the engine before the supervisor starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSpec {
    pub format: u32,
    pub id: JobId,
    pub kind: JobKind,
    pub owner: AgentKey,
    pub argv: Vec<String>,
    pub cwd: String,
    pub mounts: Vec<Mount>,
    /// The whole environment of the job (the wall clears everything else, 1.4.11.4).
    pub env: BTreeMap<String, String>,
    /// TCP ports the job may connect to; `None`, any port: a turn's, and a suite's or checks'
    /// (fac-93).
    pub ports: Option<Vec<u16>>,
    pub wall: WallMode,
    /// The binary whose `wall-exec` starts the job inside the wall.
    pub factory_bin: String,
    /// The bwrap executable.
    pub bwrap: String,
    pub created_ms: i64,
    /// How the supervisor starts it: in the work slice, niced, or as it is (fac-143).
    #[serde(default)]
    pub launch: Launch,
    /// A turn whose CLI takes messages while it works (fac-389): its supervisor feeds its stdin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
}

/// How a live turn's stdin is spoken (fac-389): the prompt first, each message delivered after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cli", rename_all = "lowercase")]
pub enum Live {
    /// Claude's stream-json input, on its `session`.
    Claude { session: String },
    /// Codex's app-server: `thread` resumed, or a new one; `images` attached to the prompt. `model` and `effort`, its
    /// command line's, are said again on the thread, whose saved settings would win (fac-637); none before fac-637.
    Codex {
        thread: Option<String>,
        images: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<crate::config::Effort>,
    },
}

/// How a job's process is started, outside its wall (fac-143).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Launch {
    /// As it is: the tests' default, and a job.json written before fac-143.
    #[default]
    Plain,
    /// In `factory_work.slice`, through `systemd-run --user --scope`.
    Slice,
    /// Under `nice -n 19 ionice -c3`, where `systemd-run --user` isn't available.
    Nice,
}

/// `claim.json`: who claimed the job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub pid: i32,
    pub start_time: u64,
    pub boot_id: String,
}

/// `run.json`: the wall the supervisor started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInfo {
    pub bwrap_pid: i32,
    pub bwrap_start: u64,
    pub init_pid: i32,
    pub init_start: u64,
    /// `/proc/<init>/ns/pid`, as `pid:[4026532…]`; empty unwalled.
    pub pidns: String,
    /// Unwalled: the job's process group.
    pub pgid: Option<i32>,
}

/// `exit.json`: written once, after every process of the job is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitRecord {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub ended_ms: i64,
    pub stopped: bool,
    /// Processes that wouldn't die.
    pub left: Vec<i32>,
    /// Why the wall didn't start, when it didn't.
    pub error: Option<String>,
}

/// `[slice]` of `factory.toml`: the weights and memory cap of `factory_work.slice`, where every
/// job runs (fac-143).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceConfig {
    /// systemd's `CPUWeight`, 1 to 10000 against the desktop's 100.
    #[serde(default = "d_weight")]
    pub cpu_weight: u32,
    /// systemd's `IOWeight`, 1 to 10000 against the desktop's 100.
    #[serde(default = "d_weight")]
    pub io_weight: u32,
    /// systemd's `MemoryHigh`: a share of the RAM (`75%`) or a size (`24G`).
    #[serde(default = "d_memory_high")]
    pub memory_high: String,
}

impl Default for SliceConfig {
    fn default() -> Self {
        Self { cpu_weight: d_weight(), io_weight: d_weight(), memory_high: d_memory_high() }
    }
}

fn d_weight() -> u32 {
    20
}
fn d_memory_high() -> String {
    "75%".into()
}
