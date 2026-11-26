// why: test code: a failed unwrap is the test failing; each test binary uses part of this.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use mork_engine::jobs::folder::ShortTemp;
use mork_fakes::TestHome;

pub mod builds;
pub mod door;
pub mod waits;
use serde_json::{Value, json};

pub fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mork"))
}

/// Waits until `f` holds, or fails the test naming `what`.
pub fn wait_until(what: &str, limit: Duration, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < limit {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out after {limit:?} waiting for {what}");
}

/// A test's mork.
pub struct Rig {
    pub t: TestHome,
    pub env: Vec<(String, String)>,
    pub app: Option<Child>,
    door_home: Option<ShortTemp>,
}

impl Rig {
    pub fn new() -> Self {
        let t = TestHome::new();
        t.link_fakes(&exe());
        std::fs::create_dir_all(t.mork_home().join("agent-home")).unwrap();
        std::fs::create_dir_all(t.root.join("mork-repo")).unwrap();
        std::fs::create_dir_all(t.mork_home().join("fixtures")).unwrap();
        let mut rig = Self { t, env: Vec::new(), app: None, door_home: None };
        rig.write_config(&rig.config(""));
        // The app's own commands (`notify-send`) find the fakes, and log where the walls' do.
        let path = format!("{}:{}", rig.t.bin().display(), std::env::var("PATH").unwrap_or_default());
        rig.set_env("PATH", &path);
        let log = rig.log_path().display().to_string();
        rig.set_env("FAKE_LOG", &log);
        rig
    }

    pub fn fhome(&self) -> PathBuf {
        self.door_home.as_ref().map_or_else(|| self.t.mork_home(), |alias| alias.path().to_owned())
    }
    pub fn agent_home(&self) -> PathBuf {
        self.fhome().join("agent-home")
    }
    fn fixture_folder(&self) -> PathBuf {
        if cfg!(feature = "faults") { self.fhome().join("fixtures") } else { self.agent_home() }
    }
    pub fn scenario_path(&self) -> PathBuf {
        self.fixture_folder().join("scenario.json")
    }
    pub fn log_path(&self) -> PathBuf {
        self.fixture_folder().join("fake.log")
    }

    /// The test config, with `extra` appended. The foreman is pinned to Codex, which the scenarios script,
    /// unless `extra` sets its own `[seats.foreman]`.
    pub fn config(&self, extra: &str) -> String {
        let pin = if extra.contains("[seats.foreman]") { "" } else { CODEX_FOREMAN };
        self.config_with(&format!("{extra}\n{pin}"))
    }

    /// The test config with the default seats, `extra` appended.
    pub fn config_with(&self, extra: &str) -> String {
        format!(
            "[mork]\nproject = \"mork\"\ntoolchains = [{{ path = \"{bin}\", bin = \".\" }}]\n\
             env = {{ FAKE_SCENARIO = \"{sc}\", FAKE_LOG = \"{log}\" }}\n\
             [git]\nauthor = \"Mork Test <test@example.invalid>\"\n\
             [projects.mork]\nrepo = \"{repo}\"\nboard = {{ kind = \"markdown\", path = \"{repo}\" }}\ntest = \"true\"\n{extra}",
            bin = self.t.bin().display(),
            sc = self.scenario_path().display(),
            log = self.log_path().display(),
            repo = self.t.root.join("mork-repo").display(),
        )
    }

    pub fn write_config(&self, text: &str) {
        std::fs::write(self.t.config_dir().join("mork.toml"), text).unwrap();
    }

    /// Writes the fakes' scenario (1.10.2). Its last run passes each review round a gated card's
    /// review card is asked, where no run of the test's own answers it first (fac-95).
    pub fn scenario(&self, runs: Value) {
        let mut runs = runs.as_array().cloned().unwrap_or_default();
        runs.push(json!({"fake": "codex", "phrase": "Review round ", "steps": [{"do": "thread-started"},
            {"do": "codex-reply", "value": reply("review-pass", "Reviewed", json!([]))}]}));
        std::fs::write(self.scenario_path(), json!({ "runs": runs }).to_string()).unwrap();
    }

    /// The fakes' run log, filtered to a fake's name.
    pub fn runs(&self, name: &str) -> Vec<Value> {
        mork_fakes::runlog::read(&self.log_path()).into_iter().filter(|l| l["name"] == name).collect()
    }

    pub fn set_env(&mut self, k: &str, v: &str) {
        self.env.retain(|(a, _)| a != k);
        self.env.push((k.into(), v.into()));
    }

    /// A command of Mork binary with the test's environment.
    pub fn command(&self, args: &[&str]) -> Command {
        self.command_at(exe(), args)
    }

    /// A command through a selected executable, with this test's environment.
    pub fn command_at(&self, bin: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Command {
        let mut c = Command::new(bin);
        c.args(args).envs(self.t.env()).envs(self.env.iter().map(|(a, b)| (a, b)));
        c.env_remove("MORK_AGENT_SOCK");
        c
    }

    /// Starts `mork --no-window`, and waits until it takes commands.
    pub fn start(&mut self) {
        self.spawn();
        self.wait_ready();
    }

    /// Starts it without waiting.
    pub fn spawn(&mut self) {
        let c = self.command(&["--no-window"]);
        self.spawn_cmd(c);
    }

    /// The app's log files, all starts together.
    pub fn app_log(&self) -> String {
        let mut out = String::new();
        for e in std::fs::read_dir(&self.t.root).unwrap().flatten() {
            if e.file_name().to_string_lossy().starts_with("app-") {
                out.push_str(&std::fs::read_to_string(e.path()).unwrap_or_default());
            }
        }
        out
    }

    fn spawn_cmd(&mut self, mut cmd: Command) {
        let n = std::fs::read_dir(&self.t.root).unwrap().count();
        let log = std::fs::File::create(self.t.root.join(format!("app-{n:03}.log"))).unwrap();
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        self.app = Some(child);
    }

    /// Waits until a `status` is answered with anything but `recovering`.
    pub fn wait_ready(&self) {
        wait_until("the app to take commands", Duration::from_secs(30), || {
            let Ok(mut c) = Client::try_connect(&self.fhome().join("mork.sock")) else { return false };
            let a = c.send(json!({"status": {}}));
            a.get("ok").is_some()
        });
    }

    pub fn client(&self) -> Client {
        Client::connect(&self.fhome().join("mork.sock"))
    }

    /// One command over `mork.sock`; the answer.
    pub fn send(&self, cmd: Value) -> Value {
        self.client().send(cmd)
    }

    // The pipeline's own jobs (a bundle `close` starts, a clone) end on their own: the app is let finish
    // them, and record what it owes, for up to 15 s, so none is left behind. A turn may hang on purpose.
    fn let_work_end(&mut self) {
        if self.app.is_none() || std::thread::panicking() {
            return;
        }
        let start = Instant::now();
        let mut quiet = 0;
        while start.elapsed() < Duration::from_secs(15) && quiet < 2 {
            let Ok(state) = self.store().load() else { return };
            let busy =
                state.jobs.values().any(|j| j.settled.is_none() && j.purpose.is_some()) || !state.owed_work.is_empty();
            quiet = if busy { 0 } else { quiet + 1 };
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// SIGTERM to the app (a clean shutdown), and its exit.
    pub fn term(&mut self) -> Option<i32> {
        let mut c = self.app.take()?;
        let _ = Command::new("kill").args(["-TERM", &c.id().to_string()]).status();
        let start = Instant::now();
        loop {
            if let Ok(Some(st)) = c.try_wait() {
                return st.code();
            }
            if start.elapsed() > Duration::from_secs(20) {
                let _ = c.kill();
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The state, when it can be read now (a run killed mid-write, or not yet made).
    pub fn try_state(&self) -> Option<mork_core::state::State> {
        let path = self.fhome().join("state.db");
        let store = mork_engine::store::Store::open_read_only(&path).ok()?;
        store.load().ok()
    }

    /// The state, opened read-only.
    pub fn store(&self) -> mork_engine::store::Store {
        mork_engine::store::Store::open_read_only(&self.fhome().join("state.db")).unwrap()
    }

    pub fn messages(&self, place: &str) -> Vec<mork_types::state::Message> {
        self.store().load().unwrap().messages.into_iter().filter(|m| m.place == place).collect()
    }

    /// The jobs of an agent, from the state, oldest first.
    pub fn jobs(&self, agent: &str) -> Vec<mork_types::state::Job> {
        let mut j: Vec<_> =
            self.store().load().unwrap().jobs.into_values().filter(|j| j.owner.as_str() == agent).collect();
        j.sort_by_key(|j| j.spec.created_ms);
        j
    }

    pub fn processes(&self) -> Vec<Value> {
        std::fs::read_to_string(self.fhome().join("logs/processes.log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn echo(&self, text: &str) -> Value {
        self.send(json!({"say": {"place": "main", "to": "echo", "text": text}}))
    }

    /// Waits until no job is left unsettled in the state.
    pub fn wait_settled(&self, limit: Duration) {
        wait_until("every job settled", limit, || {
            if !Path::new(&self.fhome().join("state.db")).exists() {
                return false;
            }
            let Ok(s) = self.store().load() else { return false };
            waits::no_wall(&s);
            !s.jobs.is_empty() && s.jobs.values().all(|j| j.settled.is_some())
        });
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.let_work_end();
        let _ = self.term();
    }
}

/// A socket client speaking 1.6's protocol.
pub struct Client {
    pub stream: UnixStream,
    pub reader: BufReader<UnixStream>,
    pub hello: Value,
    next_id: u64,
}

impl Client {
    pub fn try_connect(path: &Path) -> std::io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_secs(20)))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let hello: Value = serde_json::from_str(&line).map_err(std::io::Error::other)?;
        Ok(Self { stream, reader, hello, next_id: 1 })
    }

    pub fn connect(path: &Path) -> Self {
        Self::try_connect(path).unwrap()
    }

    pub fn send_line(&mut self, line: &str) {
        self.stream.write_all(line.as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
    }

    /// Sends a command with a fresh id; returns its id.
    pub fn request(&mut self, cmd: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send_line(&json!({"id": id, "cmd": cmd}).to_string());
        id
    }

    pub fn next(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or(Value::Null)
    }

    /// The next answer (skipping events).
    pub fn answer(&mut self) -> Value {
        loop {
            let v = self.next();
            if v.get("event").is_none() {
                return v;
            }
        }
    }

    pub fn send(&mut self, cmd: Value) -> Value {
        self.request(cmd);
        self.answer()
    }
}

// Straight into the state, past the engine (1.10.4).
impl Rig {
    /// Commits `changes` to the state, opening it as the app would. The app must be stopped.
    pub fn put(&self, changes: Vec<mork_core::state::Change>) {
        self.put_journaled(changes, &[], 0);
    }

    /// As [`Rig::put`], with `journal`'s rows at `now`.
    pub fn put_journaled(
        &self,
        changes: Vec<mork_core::state::Change>,
        journal: &[mork_core::decide::Journal],
        now: i64,
    ) {
        assert!(self.app.is_none(), "put needs the app stopped");
        let f = self.fhome();
        let mut s = mork_engine::store::Store::open(&f.join("state.db"), &"0".repeat(40)).unwrap();
        s.commit(&changes, journal, now).unwrap();
    }

    /// A card of the test project, `stage` build, its specification approved by the operator (fac-16),
    /// with its clone folder made.
    pub fn card(&self, id: &str, mode: mork_types::card::Mode) -> mork_types::card::Card {
        std::fs::create_dir_all(self.fhome().join("clones").join(id)).unwrap();
        let mut c =
            mork_types::card::Card::new(mork_types::CardId::new(id), "mork", &format!("The title of {id}"), mode, 1, 0);
        c.description = format!("The description of {id}.");
        c.acceptance = vec![format!("{id} is accepted")];
        c.stage = mork_types::card::Stage::Build;
        c.approval = Some(mork_types::card::Approval {
            by: mork_types::state::Sender::Operator,
            via: mork_types::card::ApprovedVia::Approve,
            at_ms: 0,
        });
        c
    }

    /// Puts cards, and their rooms, in the state.
    pub fn put_cards(&self, cards: Vec<mork_types::card::Card>) {
        let mut changes = Vec::new();
        for c in cards {
            changes.push(mork_core::state::Change::PutPlace(mork_types::state::Place {
                id: c.id.as_str().into(),
                next_number: 1,
            }));
            changes.push(mork_core::state::Change::PutCard(Box::new(c)));
        }
        self.put(changes);
    }

    /// The whole state, read-only.
    pub fn state(&self) -> mork_core::state::State {
        self.store().load().unwrap()
    }

    /// The prompts an agent's turns were given, oldest first.
    pub fn prompts(&self, agent: &str) -> Vec<String> {
        self.jobs(agent).into_iter().map(|j| j.stdin).collect()
    }

    /// Waits until an agent has had `n` jobs, all settled.
    pub fn wait_turns(&self, agent: &str, n: usize) {
        wait_until(&format!("{n} settled turns of {agent}"), Duration::from_secs(60), || {
            let s = self.state();
            waits::no_wall(&s);
            let j: Vec<_> = s.jobs.values().filter(|j| j.owner.as_str() == agent).collect();
            j.len() >= n && j.iter().all(|j| j.settled.is_some())
        });
    }

    /// Says something in a place, over `mork.sock`, as the operator.
    pub fn say(&self, place: &str, text: &str) -> Value {
        self.send(json!({"say": {"place": place, "text": text}}))
    }

    pub fn job_dir(&self, job: &mork_types::state::Job) -> PathBuf {
        self.fhome().join("jobs").join(job.id.as_str())
    }

    pub fn out_lines(&self, job: &mork_types::state::Job) -> Vec<Value> {
        std::fs::read_to_string(self.job_dir(job).join("out.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

/// A whole reply (3.5), valid for a role with `outcome`, its hand's headline `headline`.
pub fn reply(outcome: &str, headline: &str, messages: Value) -> Value {
    json!({
        "hand": {"color": "grey", "headline": headline, "message": "A message.", "ask": false},
        "messages": messages, "findings": [], "risks": [],
        "outcome": outcome, "schema": 7
    })
}

/// The foreman seat on Codex, as the scenarios run it (the default is Claude).
pub const CODEX_FOREMAN: &str = "[seats.foreman]\ncli = \"codex\"\nmodel = \"gpt-6.2-sol\"\neffort = \"high\"\n";

/// Whether this run has no walls: `MORK_UNWALLED=1` (1.10.9, 1.10.10).
pub fn unwalled() -> bool {
    std::env::var_os("MORK_UNWALLED").is_some_and(|v| v == "1")
}

/// Marks assertions that need a real wall; `MORK_UNWALLED=1` means they cannot run.
/// With walls enabled, a wall that cannot start must fail the assertion (1.10.9, fac-65).
pub fn wall_test(what: &str) -> bool {
    let skip = unwalled();
    if skip {
        say_skipped(&format!(
            "{what}: its assertion is about the wall, and MORK_UNWALLED=1 runs no wall (1.10.9, 1.10.10)"
        ));
    }
    skip
}

/// The train's phase of the suite, which `cargo xtask test` is told (fac-65).
pub const TRAIN_PHASE: &str = "MORK_TRAIN_PHASE";
/// The file a binary of the train's walled phase records its skipped wall tests in (fac-65).
pub const WALL_TESTS: &str = "MORK_WALL_TESTS";

/// Walled skips need a record so the unwalled phase can prove their assertions (fac-65).
/// Skips stay non-fatal until fac-123 so the older engine can land this support.
pub fn say_skipped(why: &str) {
    let _ = writeln!(std::io::stderr(), "skipped: {why}");
    if std::env::var_os(TRAIN_PHASE).is_none_or(|p| p != "walled") {
        return;
    }
    let Some(record) = std::env::var_os(WALL_TESTS).filter(|f| !f.is_empty()) else { return };
    // libtest names each test's thread after it: the name the unwalled phase runs, `--exact`.
    let current = std::thread::current();
    let Some(name) = current.name().filter(|n| *n != "main") else {
        panic!("{why}: skipped in the merge train on a thread with no test's name to run again (fac-65)");
    };
    let line = format!("{name}\n");
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&record)
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = written {
        panic!("{why}: its skip couldn't be recorded in {}: {e} (fac-65)", Path::new(&record).display());
    }
}
