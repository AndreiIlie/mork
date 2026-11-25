// Everything lives under the temporary folder, so the check never touches Andrei's own factory (1.11).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

use factory_core::state::State;
use factory_engine::jobs::launch;
use factory_engine::paths::Home;
use factory_types::config::{Cli, Effort, SOL};
use serde_json::{Value, json};

const SHORT: Duration = Duration::from_secs(30);

pub struct Scratch {
    pub root: PathBuf,
    pub home: Home,
    config: PathBuf,
    bin: PathBuf,
    app: Option<Child>,
    ended: bool,
    // Given back on every path out (M5.33).
    logins: super::logins::Logins,
}

impl Scratch {
    /// Makes the scratch home, its repository, its board and its config.
    pub fn new(real: &Home, bin: &Path) -> Result<Self, String> {
        let root = root_in(&std::env::temp_dir())?;
        let home = Home::new(root.join("home"));
        let mut s = Self {
            config: root.join("factory.toml"),
            root,
            home,
            bin: bin.to_owned(),
            app: None,
            ended: false,
            logins: Default::default(),
        };
        for d in
            ["home/agent-home", "repo", "board/backlog/tasks", "board/backlog/completed", "board/backlog/archive/tasks"]
        {
            std::fs::create_dir_all(s.root.join(d)).map_err(|e| format!("{d}: {e}"))?;
        }
        std::fs::write(
            s.root.join("board/backlog/config.yml"),
            "project_name: \"live\"\ntask_prefix: \"live\"\nstatuses: [\"Backlog\", \"In Progress\", \"Done\"]\n",
        )
        .map_err(|e| e.to_string())?;
        for (id, title, work) in [
            (CARD, "The-live-checks-Codex-turn", CODEX_WORK),
            (CLAUDE_CARD, "The-live-checks-Claude-turns", CLAUDE_FIRST),
        ] {
            std::fs::write(
                s.root.join("board/backlog/tasks").join(format!("{id} - {title}.md")),
                card(id, title, work),
            )
            .map_err(|e| e.to_string())?;
        }
        s.repo()?;
        s.logins = super::logins::Logins::copy_in(&real.agent_home(), &s.home.agent_home())?;
        s.foreman_walled(true)?;
        Ok(s)
    }

    /// Writes the scratch config, the foremen walled or with none (fac-25, fac-119): walled, they
    /// run the check's turns; with none, live-2's runs the deny list's.
    pub fn foreman_walled(&self, walled: bool) -> Result<(), String> {
        std::fs::write(&self.config, self.config_text(walled)?).map_err(|e| e.to_string())
    }

    fn repo(&self) -> Result<(), String> {
        let repo = self.root.join("repo");
        std::fs::write(repo.join("README.md"), "The live check's scratch repository.\n").map_err(|e| e.to_string())?;
        let id = ["-c", "user.name=Live Check", "-c", "user.email=live@example.invalid"];
        for args in
            [vec!["init", "-q", "-b", "master"], vec!["add", "-A"], [&id[..], &["commit", "-qm", "Start"]].concat()]
        {
            let mut argv = vec!["git"];
            argv.extend(args);
            let out = launch::short(self.home.root(), &argv, &repo, SHORT).map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!("git {}: {}", argv[1..].join(" "), String::from_utf8_lossy(&out.stderr).trim()));
            }
        }
        Ok(())
    }

    fn config_text(&self, foreman_walled: bool) -> Result<String, String> {
        let real: toml::Table = factory_engine::config::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        scratch_config(&real, &self.root, foreman_walled)
    }

    pub fn config_path(&self) -> &Path {
        &self.config
    }

    /// Starts the factory (`--no-window`) and waits for it to be ready.
    pub fn start(&mut self) -> Result<(), String> {
        let env = vec![
            ("FACTORY_HOME".to_owned(), self.home.root().display().to_string()),
            ("FACTORY_CONFIG".to_owned(), self.config.display().to_string()),
        ];
        let argv = vec![self.bin.display().to_string(), "--no-window".into()];
        let log = self.root.join("factory.log");
        self.app = Some(launch::child(self.home.root(), &argv, &env, &log).map_err(|e| e.to_string())?);
        let start = factory_engine::clock::instant();
        while factory_engine::clock::elapsed(start) < Duration::from_secs(120) && !interrupted() {
            if self.ask(&json!({"status": {}})).is_ok() {
                return Ok(());
            }
            if self.app.as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_some()) {
                return Err(format!("it exited: {}", tail(&log)));
            }
            factory_engine::clock::sleep(Duration::from_millis(300));
        }
        Err(format!("not ready in 2 minutes: {}", tail(&log)))
    }

    /// Ends the factory cleanly and starts it again, its config read anew.
    pub fn restart(&mut self) -> Result<(), String> {
        self.end_app();
        self.start()
    }

    /// Kills the factory with SIGKILL, as a crash would.
    pub fn kill(&mut self) {
        if let Some(mut c) = self.app.take() {
            let _ = factory_sys::kill(i32::try_from(c.id()).unwrap_or(0), 9);
            let _ = c.wait();
        }
    }

    /// Ends the check on any path (M5.18): jobs stopped, factory ended with SIGTERM; the folder is removed
    /// only once no job runs, else it is kept and why is said.
    pub fn end(&mut self) -> Result<(), String> {
        if self.ended {
            return Ok(());
        }
        self.ended = true;
        self.stop_jobs();
        self.end_app();
        let left = self.supervisors_down();
        // The logins a scratch CLI renewed go back before the folder goes (M5.33).
        for line in self.logins.give_back() {
            println!("note    {line}");
        }
        if !left.is_empty() {
            return Err(format!("still running: {left:?}; {} is kept", self.root.display()));
        }
        std::fs::remove_dir_all(&self.root).map_err(|e| format!("{}: {e}", self.root.display()))
    }

    pub(super) fn stop_jobs(&self) {
        let unsettled = |s: &Self| {
            s.state().map(|st| st.jobs.into_values().filter(|j| j.settled.is_none()).collect()).unwrap_or_default()
        };
        let jobs: Vec<factory_types::state::Job> = unsettled(self);
        if jobs.is_empty() || self.app.is_none() {
            return;
        }
        for j in &jobs {
            let _ = self.ask(&json!({"stop": {"agent": j.owner.as_str()}}));
        }
        let start = factory_engine::clock::instant();
        while !unsettled(self).is_empty() && factory_engine::clock::elapsed(start) < Duration::from_secs(30) {
            factory_engine::clock::sleep(Duration::from_millis(300));
        }
    }

    // SIGTERM first (a supervisor stops its wall), SIGKILL 10 s on. A pid is signalled only while
    // its start time and boot are the recorded ones: a pid reused since is someone else's (1.4.4, M5.28).
    fn supervisors_down(&self) -> Vec<i32> {
        let first = self.recorded_alive();
        for p in &first {
            let _ = factory_sys::kill(*p, 15);
        }
        let start = factory_engine::clock::instant();
        while !self.recorded_alive().is_empty() && factory_engine::clock::elapsed(start) < Duration::from_secs(10) {
            factory_engine::clock::sleep(Duration::from_millis(200));
        }
        for p in self.recorded_alive() {
            let _ = factory_sys::kill(p, 9);
        }
        factory_engine::clock::sleep(Duration::from_millis(500));
        self.recorded_alive()
    }

    // Only the processes their records name: a pid reused since is someone else's. A zombie has
    // ended, though its start time stays: unwalled, a subreaper ancestor may never reap it (fac-131).
    fn recorded_alive(&self) -> Vec<i32> {
        use factory_engine::jobs::procs::{boot_id, start_time};
        let boot = boot_id();
        let dirs = std::fs::read_dir(self.home.jobs()).into_iter().flatten().flatten();
        dirs.flat_map(|e| {
            let d = factory_engine::jobs::folder::JobDir::new(e.path());
            let claim = d.claim().ok().flatten().filter(|c| c.boot_id == boot);
            let run = claim.as_ref().and_then(|_| d.run().ok().flatten());
            let recorded = [
                claim.map(|c| (c.pid, c.start_time)),
                run.as_ref().map(|r| (r.bwrap_pid, r.bwrap_start)),
                run.map(|r| (r.init_pid, r.init_start)),
            ];
            recorded
                .into_iter()
                .flatten()
                .filter(|(p, t)| *p > 0 && start_time(*p) == Some(*t) && alive(*p))
                .map(|(p, _)| p)
        })
        .collect()
    }

    fn end_app(&mut self) {
        if let Some(mut c) = self.app.take() {
            let _ = factory_sys::kill(i32::try_from(c.id()).unwrap_or(0), 15);
            let start = factory_engine::clock::instant();
            while c.try_wait().ok().flatten().is_none()
                && factory_engine::clock::elapsed(start) < Duration::from_secs(10)
            {
                factory_engine::clock::sleep(Duration::from_millis(100));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// One command over `factory.sock`: its answer, or the refusal's words.
    pub fn ask(&self, cmd: &Value) -> Result<Value, String> {
        let mut stream = UnixStream::connect(self.home.factory_sock()).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(SHORT)).map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        stream.write_all(format!("{}\n", json!({"id": 1, "cmd": cmd})).as_bytes()).map_err(|e| e.to_string())?;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("the connection closed".into());
            }
            let v: Value = serde_json::from_str(&line).unwrap_or_default();
            if v["id"] == 1 {
                return v
                    .get("ok")
                    .cloned()
                    .ok_or_else(|| v["err"]["message"].as_str().unwrap_or("refused").to_owned());
            }
        }
    }

    /// The scratch factory's state, when it can be read now.
    pub fn state(&self) -> Option<State> {
        factory_engine::store::Store::open_read_only(&self.home.root().join("state.db")).ok()?.load().ok()
    }

    /// The journal's lines of a kind, with their data.
    pub fn events(&self, kind: &str) -> Vec<Value> {
        let Ok(store) = factory_engine::store::Store::open_read_only(&self.home.root().join("state.db")) else {
            return Vec::new();
        };
        store.events().unwrap_or_default().into_iter().filter(|e| e.2 == kind).map(|e| e.3).collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

/// Whether a process lives (a zombie doesn't).
pub fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|t| !t.contains(") Z "))
}

/// Whether SIGINT or SIGTERM asked the check to end: it stops where it is, and its clean-up
/// runs (M5.28).
pub fn interrupted() -> bool {
    factory_sys::term_requested()
}

/// The scratch board's card: a report card, its foreman the check's Codex turn (fac-25).
pub const CARD: &str = "live-1";

/// The Codex turn's work: a minute, asked by the card its foreman reports on.
pub const CODEX_WORK: &str = "Run `sleep 60` in the foreground and wait for it to finish, then report one line: done.";

/// The scratch board's second card: a report card, its foreman picked onto Claude for the check's
/// Claude turns (fac-119).
pub const CLAUDE_CARD: &str = "live-2";

// What the second card asks of its first turn, the one before the pick reaches it: a line.
const CLAUDE_FIRST: &str = "Report one line: ready.";

fn card(id: &str, title: &str, work: &str) -> String {
    let (upper, title) = (id.to_uppercase(), title.replace('-', " ").replace("checks", "check's"));
    format!(
        "---\nid: {upper}\ntitle: {title}\nstatus: Backlog\nassignee: []\ncreated_date: '2026-10-17 00:00'\nlabels: []\ndependencies: []\npriority: p3\n---\n\n## Description\n\n<!-- SECTION:DESCRIPTION:BEGIN -->\nThe factory's live check. {work} Nothing else is asked.\n<!-- SECTION:DESCRIPTION:END -->\n"
    )
}

// The scratch root, made: a short name, so its deepest socket binds under the train's host alias
// (fac-131).
pub(crate) fn root_in(base: &Path) -> Result<PathBuf, String> {
    loop {
        let root = base.join(format!("live-{}", &factory_engine::random::uuid()[..6]));
        match std::fs::create_dir(&root) {
            Ok(()) => return Ok(root),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("{}: {e}", root.display())),
        }
    }
}

fn tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(5)..].join(" | ")
}

// The scratch config: the real one's toolchains, `[factory] env`, `[git]` and seats, so the
// real CLIs are reached as the real factory reaches them.
fn scratch_config(real: &toml::Table, root: &Path, foreman_walled: bool) -> Result<String, String> {
    let mut factory = toml::Table::new();
    factory.insert("project".into(), "live".into());
    for key in ["toolchains", "env"] {
        if let Some(v) = real.get("factory").and_then(|f| f.get(key)) {
            factory.insert(key.into(), v.clone());
        }
    }
    let mut out = toml::Table::new();
    out.insert("factory".into(), factory.into());
    let git = real.get("git").cloned().unwrap_or_else(|| {
        toml::Value::Table(toml::Table::from_iter([("author".to_owned(), "Live Check <live@example.invalid>".into())]))
    });
    out.insert("git".into(), git);
    let mut seats = real.get("seats").and_then(toml::Value::as_table).cloned().unwrap_or_default();
    let foreman = seats.entry("foreman").or_insert_with(|| toml::Table::new().into());
    if let Some(t) = foreman.as_table_mut() {
        // The Codex leg's live-1 starts as the foreman, so the foreman is Codex whatever the
        // real config or the roster says; live-2 is picked onto Claude.
        t.insert("cli".into(), Cli::Codex.as_str().into());
        t.insert("model".into(), SOL.into());
        t.insert("effort".into(), Effort::High.as_str().into());
        t.remove("advisor");
        t.insert("wall".into(), if foreman_walled { "full" } else { "none" }.into());
    }
    out.insert("seats".into(), seats.into());
    let mut live = toml::Table::new();
    live.insert("repo".into(), root.join("repo").display().to_string().into());
    live.insert("board".into(), root.join("board").display().to_string().into());
    live.insert("test".into(), "true".into());
    out.insert("projects".into(), toml::Value::Table(toml::Table::from_iter([("live".to_owned(), live.into())])));
    toml::to_string(&out).map_err(|e| e.to_string())
}
