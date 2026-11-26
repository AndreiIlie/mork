//! What every job's wall is built from: the home, the running binary, the wall's mode, the
//! environment of 1.4.11.4. Made once at start.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mork_types::config::{Config, Seat, Toolchain};
use mork_types::job::{JobKind, JobSpec, Launch, WallMode};
use mork_types::{AgentKey, JobId};

use super::wall::{self, WallPlaces};
use crate::paths::Home;
use crate::{faults, files};

/// The wall's executable: `/usr/bin/bwrap`, or `MORK_TEST_BWRAP` (1.10.6).
pub fn bwrap_path() -> String {
    faults::test_env("MORK_TEST_BWRAP").unwrap_or_else(|| super::wall::BWRAP.into())
}

/// The wall's mode: full, or with the test settings `partial` or none (1.4.11.6).
pub fn wall_mode() -> WallMode {
    if faults::test_env("MORK_UNWALLED").as_deref() == Some("1") {
        WallMode::None
    } else if faults::test_env("MORK_TEST_WALL").as_deref() == Some("partial") {
        WallMode::Partial
    } else {
        WallMode::Full
    }
}

/// Why no wall can start here, if none can (1.4.11.6): fail closed.
pub fn wall_unavailable(mode: WallMode, bwrap: &str) -> Option<String> {
    if mode == WallMode::None {
        return None;
    }
    if !Path::new(bwrap).exists() {
        return Some(format!("no wall: {bwrap} is missing"));
    }
    let abi = super::wall_exec::landlock_abi();
    if abi < super::wall_exec::MIN_ABI && mode == WallMode::Full {
        return Some(format!("no wall: the Landlock ABI is {abi}, below {}", super::wall_exec::MIN_ABI));
    }
    None
}

/// Everything a job's spec is made from.
#[derive(Debug, Clone)]
pub struct Context {
    pub home: Home,
    pub config: Config,
    /// The running binary.
    pub bin: PathBuf,
    pub bwrap: String,
    pub mode: WallMode,
    /// How the supervisor starts every job: the engine's start chooses (fac-143).
    pub launch: Launch,
    /// The app's `$HOME` and `$TMPDIR`, hidden in every wall.
    pub app_home: String,
    pub app_tmp: String,
    /// The app's `$XDG_RUNTIME_DIR`, where Claude Code keeps its sockets (fac-265).
    pub app_runtime: String,
    /// The operator's own Codex home (`$CODEX_HOME`, else `~/.codex`), never read for a project's MCP
    /// servers (1.4.8, Codex 2).
    pub app_codex_home: PathBuf,
    pub lang: String,
    pub user: String,
    pub telemetry: BTreeMap<String, String>,
    /// The running binary's skills as the engine wrote them out at start (fac-88); none elsewhere.
    pub skills: Option<PathBuf>,
}

impl Context {
    /// Reads the app's environment.
    pub fn new(home: Home, config: Config, bin: PathBuf, telemetry: BTreeMap<String, String>) -> Self {
        let var = |k: &str, d: &str| std::env::var(k).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| d.to_owned());
        Self {
            home,
            config,
            bin,
            bwrap: bwrap_path(),
            mode: wall_mode(),
            launch: Launch::Plain,
            app_home: var("HOME", ""),
            app_tmp: var("TMPDIR", "/tmp"),
            app_runtime: var("XDG_RUNTIME_DIR", ""),
            app_codex_home: std::env::var_os("CODEX_HOME")
                .filter(|v| !v.is_empty())
                .map_or_else(|| PathBuf::from(var("HOME", "/")).join(".codex"), PathBuf::from),
            lang: var("LANG", "C.UTF-8"),
            user: var("USER", "mork"),
            telemetry,
            skills: None,
        }
    }

    /// The wall of a seat's turn: none when its seat says so (fac-25), else Mork's.
    pub fn wall_of(&self, seat: &Seat) -> WallMode {
        if seat.wall == mork_types::config::Wall::None { WallMode::None } else { self.mode }
    }

    pub fn bin_dir(&self) -> String {
        self.bin.parent().map(|p| p.display().to_string()).unwrap_or_default()
    }

    fn toolchains(&self, extra: &[Toolchain]) -> Vec<Toolchain> {
        self.config.mork.toolchains.iter().chain(extra).cloned().collect()
    }

    /// `PATH`: the binary's folder, each toolchain's `bin` in order, then the system's.
    pub fn path_var(&self, extra: &[Toolchain]) -> String {
        let mut parts = vec![self.bin_dir()];
        for t in self.toolchains(extra) {
            if let Some(bin) = &t.bin {
                let p = Path::new(&t.path).join(bin);
                parts.push(p.display().to_string());
            }
        }
        parts.push("/usr/local/bin:/usr/bin:/bin".into());
        parts.join(":")
    }

    /// The ports a project's job may reach: the config's and the project's own (1.4.11.3).
    pub fn ports(&self, project: Option<&str>) -> Vec<u16> {
        let mut ports = self.config.mork.ports.clone();
        ports.extend(
            project
                .and_then(|p| self.config.projects.get(p))
                .map(mork_types::config::ProjectConfig::port_list)
                .unwrap_or_default(),
        );
        ports.sort_unstable();
        ports.dedup();
        ports
    }

    /// The ports a job of no turn may reach: any for a suite's or checks', whose tests serve on
    /// loopback ports of their own (fac-93); else [`Self::ports`].
    pub fn work_ports(&self, kind: JobKind, project: Option<&str>) -> Option<Vec<u16>> {
        (kind != JobKind::Check).then(|| self.ports(project))
    }

    /// A project's toolchains, after the global ones (1.4.11.1).
    pub fn project_toolchains(&self, project: Option<&str>) -> Vec<Toolchain> {
        project.and_then(|p| self.config.projects.get(p)).map(|p| p.toolchains.clone()).unwrap_or_default()
    }

    /// The environment of 1.4.11.4, `MORK_IO` naming `io`: a seat's own folder for its turns (fac-654), the job's
    /// `io/` for any other job. A seat's `env` wins over `[mork] env`.
    pub fn env(
        &self,
        job: &JobId,
        owner: &AgentKey,
        seat: Option<&Seat>,
        extra: &[Toolchain],
        io: &Path,
    ) -> BTreeMap<String, String> {
        let s = |p: PathBuf| p.display().to_string();
        let mut e = BTreeMap::from([
            ("HOME".to_owned(), s(self.home.agent_home())),
            ("PATH".to_owned(), self.path_var(extra)),
            ("LANG".to_owned(), self.lang.clone()),
            ("USER".to_owned(), self.user.clone()),
            ("LOGNAME".to_owned(), self.user.clone()),
            ("SHELL".to_owned(), "/bin/bash".to_owned()),
            ("TMPDIR".to_owned(), "/tmp".to_owned()),
            (wall::BROWSERS_ENV.to_owned(), s(self.home.browsers())),
            ("MORK_JOB".to_owned(), job.as_str().to_owned()),
            ("MORK_IO".to_owned(), io.display().to_string()),
            ("MORK_AGENT".to_owned(), owner.as_str().to_owned()),
            ("MORK_AGENT_SOCK".to_owned(), s(self.home.agent_sock())),
        ]);
        // The test override of the Landlock ABI reaches `wall-exec` inside the wall, which
        // `--clearenv` would otherwise strip (only with `faults`).
        if let Some(abi) = faults::test_env("MORK_TEST_LANDLOCK_ABI") {
            e.insert("MORK_TEST_LANDLOCK_ABI".into(), abi);
        }
        e.extend(self.telemetry.clone());
        e.extend(self.config.mork.env.clone());
        if let Some(seat) = seat {
            e.extend(seat.env.clone());
            // No notes of a seat's own: its memory is the session, the record, the cards and the
            // manuals (fac-25, decision 4). The variable outranks the setting in Claude Code.
            if seat.cli == mork_types::config::Cli::Claude {
                e.insert("CLAUDE_CODE_DISABLE_DEFERRED_TOOL_RESUME".into(), "1".into());
                e.insert("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into());
                // Its planner, TaskCreate and the rest, which a headless Claude leaves off (fac-127).
                e.insert("CLAUDE_CODE_ENABLE_TODO_TOOLS".into(), "1".into());
            }
            // A turn runs the agy it was given: an update mid-turn would change it under the turn.
            if seat.cli == mork_types::config::Cli::Agy {
                e.insert("AGY_CLI_DISABLE_AUTO_UPDATE".into(), "true".into());
                if let Some(cap) = self.config.mork.context_cap() {
                    e.insert("AGY_CLI_AUTO_COMPACT_OVERRIDE".into(), cap.to_string());
                }
            }
            // Only a walled turn has the mount; an unwalled one reads the folder itself (fac-27, decision 2).
            if let Some(c) = &seat.credentials {
                let at = if self.wall_of(seat) == WallMode::None { c.clone() } else { "/run/credentials".into() };
                e.insert("MORK_CREDENTIALS".into(), at);
            }
        }
        e
    }

    /// The common wall places and explicit binds; a failed secret scan prevents the wall from starting.
    pub fn places(
        &self,
        io: &Path,
        read_only: Vec<String>,
        read_write: Vec<String>,
        credentials: Option<String>,
        extra: &[Toolchain],
    ) -> Result<WallPlaces, String> {
        let mut places = self.unwalled_places(io, read_only, read_write, credentials, extra);
        if self.mode == WallMode::None {
            return Ok(places);
        }
        places.system = wall::system().map_err(|e| e.to_string())?;
        places.peer_clones = files::child_dirs(&self.home.clones())
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|p| p.display().to_string())
            .filter(|p| !places.read_only.contains(p) && !places.read_write.contains(p))
            .collect();
        let empty_file = self.home.run_dir().join("empty");
        files::write_whole(&empty_file, b"").map_err(|e| e.to_string())?;
        places.hidden = self.hidden_folders();
        places.masked = self.hidden_names(extra)?;
        places.empty_file = empty_file.display().to_string();
        Ok(places)
    }

    /// The base mount metadata, including io/, without host enumeration or a secret scan.
    pub fn unwalled_places(
        &self,
        io: &Path,
        read_only: Vec<String>,
        read_write: Vec<String>,
        credentials: Option<String>,
        extra: &[Toolchain],
    ) -> WallPlaces {
        let s = |p: PathBuf| p.display().to_string();
        let mut ro = vec![self.bin_dir(), s(self.home.builds())];
        ro.extend(self.toolchains(extra).into_iter().map(|t| t.path));
        ro.extend(read_only);
        let mut rw = vec![io.display().to_string()];
        rw.extend(read_write);
        let fixtures = self.home.root().join("fixtures");
        if faults::TEST_SETTINGS && fixtures.is_dir() {
            rw.push(fixtures.display().to_string());
        }
        WallPlaces {
            sealed: Some(s(self.home.root().to_path_buf())),
            agent_home: s(self.home.agent_home()),
            browsers: Some(s(self.home.browsers())),
            run_dir: s(self.home.run_dir()),
            read_only: ro,
            read_write: rw,
            credentials,
            ..WallPlaces::default()
        }
    }

    fn hidden_folders(&self) -> Vec<String> {
        [self.app_home.as_str(), self.app_tmp.as_str(), self.app_runtime.as_str(), "/var/crash"]
            .into_iter()
            .filter(|p| !p.is_empty())
            .filter(|p| !["/run", "/tmp", "/var/tmp", "/proc", "/dev"].iter().any(|r| Path::new(p).starts_with(r)))
            // A folder missing on the host hides nothing, and its mount point can't be made in the read-only /var.
            .filter(|p| Path::new(p).is_dir())
            .map(str::to_owned)
            .collect()
    }

    fn hidden_names(&self, extra: &[Toolchain]) -> Result<Vec<files::HiddenPath>, String> {
        let accounts = self.home.login_home().join(".claude/accounts");
        let mut hidden =
            vec![files::HiddenPath { path: std::fs::canonicalize(&accounts).unwrap_or(accounts), directory: true }];
        let tokens = self
            .config
            .projects
            .values()
            .flat_map(|p| [p.forge.token_path(), p.tickets.as_ref().map(|t| t.token())])
            .flatten();
        for token in tokens {
            let path = Path::new(token);
            match std::fs::canonicalize(path) {
                Ok(path) if path.is_file() => hidden.push(files::HiddenPath { path, directory: false }),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("cannot mask {}: {e}", path.display())),
            }
        }
        for entry in self.toolchains(extra) {
            hidden.extend(
                files::hidden_names(Path::new(&entry.path), &entry.hide)
                    .map_err(|e| format!("cannot scan {}: {e}", entry.path))?,
            );
        }
        Ok(hidden)
    }

    // why: a job's spec has this many independent parts; a struct for them would only rename them.
    #[allow(clippy::too_many_arguments)]
    pub fn spec(
        &self,
        id: &JobId,
        kind: JobKind,
        owner: &AgentKey,
        argv: Vec<String>,
        cwd: &str,
        places: &WallPlaces,
        env: BTreeMap<String, String>,
        ports: Option<Vec<u16>>,
        now: i64,
    ) -> JobSpec {
        JobSpec {
            format: mork_types::job::JOB_FORMAT,
            id: id.clone(),
            kind,
            owner: owner.clone(),
            argv,
            cwd: cwd.to_owned(),
            mounts: wall::mounts(places),
            env,
            ports,
            net: None,
            wall: self.mode,
            mork_bin: self.bin.display().to_string(),
            bwrap: self.bwrap.clone(),
            created_ms: now,
            launch: self.launch,
            live: None,
        }
    }
}
