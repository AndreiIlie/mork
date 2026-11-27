// Starting up, in 2.7's order: the lock, the sockets, the state, a reload's end, the jobs;
// then commands.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, atomic::Ordering};
use std::time::Instant;

use mork_core::decide::Event;
use mork_core::state::State;

use super::{Engine, Input, Options, Phase, StartError, upkeep};
use crate::jobs::context::Context;
use crate::paths::Home;
use crate::sockets::{self, Origin, Shared};
use crate::store::{Store, StoreError};
use crate::{builds, clock, config, jobs, settings};

// The fallback is said once, whatever the starts after it find (fac-143).
const FALLBACK: &str = "launch.fallback";

/// The descriptors a reload keeps open across its exec (1.8.6.5).
#[derive(Debug)]
pub struct Kept {
    pub lock: File,
    pub mork: RawFd,
    pub agent: RawFd,
}

/// The environment variable that hands the kept descriptors to the target of a reload.
pub const INHERIT: &str = "MORK_INHERIT";

fn lock(home: &Home) -> Result<File, StartError> {
    std::fs::create_dir_all(home.root()).map_err(|e| format!("{}: {e}", home.root().display()))?;
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.app_lock())
        .map_err(|e| format!("{}: {e}", home.app_lock().display()))?;
    if !mork_sys::try_lock_exclusive(&f).map_err(|e| e.to_string())? {
        let mut pid = String::new();
        let _ = f.read_to_string(&mut pid);
        return Err(StartError::AlreadyRunning(format!(
            "Mork already runs on {} (pid {})",
            home.root().display(),
            pid.trim()
        )));
    }
    f.set_len(0).map_err(|e| e.to_string())?;
    let _ = write!(f, "{}", std::process::id());
    Ok(f)
}

fn inherited(opts: &Options) -> Option<(File, RawFd, RawFd)> {
    opts.reloaded.as_ref()?;
    let spec = std::env::var(INHERIT).ok()?;
    let mut fds = spec.split(',').filter_map(|p| p.parse::<RawFd>().ok());
    let (l, f, a) = (fds.next()?, fds.next()?, fds.next()?);
    let lock = mork_sys::file_from_fd(l).ok()?;
    Some((lock, f, a))
}

// Jobs without a project override inherit the shared home's Git identity.
fn git_identity(home: &Home, config: &mork_types::config::Config) -> Result<(), String> {
    let text = crate::seat_homes::git_identity(&config.git.author);
    let path = home.agent_home().join(".gitconfig");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        crate::files::write_whole(&path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

fn make_dirs(home: &Home) -> Result<(), String> {
    for d in [
        home.root().to_path_buf(),
        home.run_dir(),
        home.logs(),
        home.jobs(),
        home.agent_home(),
        home.work(),
        home.homes(),
        home.caches(),
        home.login_home(),
        home.builds(),
    ] {
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    Ok(())
}

fn open_sockets(
    opts: &Options,
    shared: &Arc<Shared>,
    tx: &std::sync::mpsc::Sender<Input>,
    inh: Option<(RawFd, RawFd)>,
) -> Result<(RawFd, RawFd), String> {
    let (fl, al) = match inh {
        Some((f, a)) => (
            mork_sys::listener_from_fd(f).map_err(|e| e.to_string())?,
            mork_sys::listener_from_fd(a).map_err(|e| e.to_string())?,
        ),
        None => (
            sockets::bind(&opts.home.mork_sock()).map_err(|e| format!("mork.sock: {e}"))?,
            sockets::bind(&opts.home.agent_sock()).map_err(|e| format!("agent.sock: {e}"))?,
        ),
    };
    let fds = (fl.as_raw_fd(), al.as_raw_fd());
    let (ftx, atx) = (tx.clone(), tx.clone());
    let (fsx, asx) = (std::sync::mpsc::channel(), std::sync::mpsc::channel());
    forward(fsx.1, ftx);
    forward(asx.1, atx);
    sockets::serve(fl, Origin::Operator, Arc::clone(shared), fsx.0);
    sockets::serve(al, Origin::Agent, Arc::clone(shared), asx.0);
    Ok(fds)
}

fn forward(rx: Receiver<sockets::Incoming>, tx: std::sync::mpsc::Sender<Input>) {
    std::thread::spawn(move || {
        for inc in rx {
            if tx.send(Input::Socket(inc)).is_err() {
                break;
            }
        }
    });
}

fn open_state(home: &Home, build: &str) -> Result<Store, String> {
    match Store::open(&home.state_db(), build) {
        Ok(s) => Ok(s),
        Err(StoreError::Version { found, oldest, newest }) => {
            let readers = builds::readers_of(home, found);
            let names = if readers.is_empty() { "none here".to_owned() } else { readers.join(", ") };
            Err(format!(
                "the state is at version {found}; this build reads {oldest} to {newest}. Builds that read it: {names}"
            ))
        }
        Err(e) => {
            Err(format!("{e}; the latest backup: {}", builds::latest_backup(home).unwrap_or_else(|| "none".into())))
        }
    }
}

fn telemetry() -> std::collections::BTreeMap<String, String> {
    std::env::var_os("HOME").map(|h| settings::read_telemetry(std::path::Path::new(&h))).unwrap_or_default()
}

fn materialize_skills(home: &Home, config: &mork_types::config::Config) -> Result<Option<std::path::PathBuf>, String> {
    let extra = config.mork.skills.as_deref().map(std::path::Path::new);
    crate::builds::skills::materialize(home, extra).map_err(|e| format!("skills: {e}"))
}

// The jobs' launch is chosen before any job is written, recovery's included (fac-143); with
// why it is the fallback, if it is.
fn context(
    opts: &Options,
    config: mork_types::config::Config,
    choosing: jobs::slice::Choosing,
    skills: Option<std::path::PathBuf>,
) -> (Context, Option<String>) {
    let mut ctx = Context::new(opts.home.clone(), config, opts.bin.clone(), telemetry());
    ctx.skills = skills;
    let (launch, fallback) = choosing.chosen();
    ctx.launch = launch;
    (ctx, fallback)
}

// The start's timing: a line in `logs/app.log` for each step, with its duration, so a slow start
// shows where its time went (fac-145). Lines held until `logs/` exists.
struct Timer {
    home: Home,
    began: Instant,
    last: Instant,
    held: Vec<String>,
}

impl Timer {
    fn new(opts: &Options) -> Self {
        let now = clock::instant();
        let mut t = Timer { home: opts.home.clone(), began: opts.began, last: now, held: Vec::new() };
        t.say(format!("start launch: {} ms", clock::elapsed(opts.began).as_millis()));
        t
    }

    fn say(&mut self, line: String) {
        self.held.push(line);
        if self.home.logs().is_dir() {
            for l in self.held.drain(..) {
                upkeep::log(&self.home, &l);
            }
        }
    }

    // Timed from the previous step's end, so the lines add up to the total, nothing unaccounted.
    fn step(&mut self, name: &str) {
        let ms = clock::elapsed(self.last).as_millis();
        self.last = clock::instant();
        self.say(format!("start {name}: {ms} ms"));
    }

    fn since_process_start(&self) -> u128 {
        clock::elapsed(self.began).as_millis()
    }
}

// The engine, its inputs, and what its start says once: the reload's note, the launch's fallback.
type Opened = (Engine, Receiver<Input>, Option<builds::ReloadNote>, Option<String>);

// Split from `start` only to keep each within the 60-line limit (fac-145).
fn open(opts: Options, t: &mut Timer, lock_file: File, sock_fds: Option<(RawFd, RawFd)>) -> Result<Opened, String> {
    let (loaded_config, mut config) = config::read_text(&opts.config_path)?;
    t.step("config");
    make_dirs(&opts.home)?;
    git_identity(&opts.home, &config)?;
    mork_sys::install_term_flag().map_err(|e| e.to_string())?;
    let shared = Shared::new(opts.commit.clone(), opts.dirty, opts.home.root().display().to_string());
    let (tx, rx) = channel();
    let (mork, agent) = open_sockets(&opts, &shared, &tx, sock_fds)?;
    let (reserved_ports, bound_web_port) =
        jobs::wall_ports::start(&opts.home, &config).map_err(|e| format!("wall service ports: {e}"))?;
    // Core allocates preview and mock ports from this config, including when the OS chose the web port.
    config.mork.web_port = bound_web_port.or(config.mork.web_port);
    let sampled = tx.clone();
    std::thread::spawn(move || crate::resources::watch(|r| sampled.send(Input::Resources(r)).is_ok()));
    builds::check_reload_target(&opts)?;
    // Materialize configured skills before open_state migrates the DB or finish_reload moves the pin (fac-721).
    let skills = materialize_skills(&opts.home, &config)?;
    t.step("sockets");
    // Its probe runs while the state opens and loads (fac-336), its lock held.
    let choosing = jobs::slice::choosing(opts.home.root(), &config.slice);
    let store = open_state(&opts.home, &opts.commit)?;
    *shared.state_version.lock() = Some(store.version);
    t.step("open_state");
    let reload_note = builds::finish_reload(&opts)?;
    t.step("finish_reload");
    let state = store.load().map_err(|e| e.to_string())?;
    t.step("store.load");
    let (ctx, fallback) = context(&opts, config, choosing, skills);
    let wakes = super::wakes::Wakes::open(&opts.home);
    let routing = super::routing::Cache::open(&ctx.config, &store)?;
    let engine = Engine {
        routing,
        loaded_config,
        reserved_ports,
        bound_web_port,
        opts,
        ctx,
        store,
        state,
        shared,
        phase: Phase::Ready,
        live: Default::default(),
        watchers: Vec::new(),
        hooks: Default::default(),
        wakes,
        tx,
        last_upkeep: None,
        keep: Kept { lock: lock_file, mork, agent },
        seen: Default::default(),
        limits_read: Default::default(),
        pipe: Default::default(),
        views_cache: Default::default(),
        door: Default::default(),
        closing_dirs: Default::default(),
        previews: Default::default(),
        mocks: Default::default(),
        suites: Default::default(),
    };
    t.step("context");
    Ok((engine, rx, reload_note, fallback))
}

/// Starts the engine from its configured home and the running build's resources.
pub fn start(opts: Options) -> Result<(Engine, Receiver<Input>), StartError> {
    let (lock_file, sock_fds) = match inherited(&opts) {
        Some((l, f, a)) => (l, Some((f, a))),
        None => (lock(&opts.home)?, None),
    };
    let mut t = Timer::new(&opts);
    let (mut engine, rx, reload_note, fallback) = open(opts, &mut t, lock_file, sock_fds)?;
    engine.views_cache.claude_accounts = crate::claude_accounts::Accounts::read(
        &engine.opts.home.login_home(),
        engine.ctx.config.claude_accounts.clone(),
    );
    let shared = Arc::clone(&engine.shared);
    let from = reload_note.as_ref().and_then(|n| n.builds.as_ref()).map(|(from, _)| from.clone());
    // The boards first: they are never stored, and a job's end applied in recovery decides on
    // them (a card's started status, T6.3).
    engine.read_boards_now();
    t.step("read_boards_now");
    engine.recover_jobs();
    t.step("recover_jobs");
    engine.apply(&Event::Started);
    t.step("Started");
    engine.sweep_trains();
    t.step("sweep_trains");
    engine.sweep_bundles();
    t.step("sweep_bundles");
    engine.say_at_start(reload_note, fallback);
    t.step("say_at_start");
    engine.material_read(from.as_deref());
    t.step("material_read");
    // The started actions' checks come back and are applied while commands are still answered
    // `recovering` (2.7, step 7; M5.16).
    engine.check_started();
    t.step("check_started");
    // Pending door reads drop and owed door outcomes reach their requesters (fac-632).
    engine.door_recover();
    t.step("door_recover");
    // A SIGTERM while the checks were out: the loop ends at once, and nothing more starts.
    if mork_sys::term_requested() {
        t.say(format!("start ended by SIGTERM after {} ms", t.since_process_start()));
        return Ok((engine, rx));
    }
    shared.ready.store(true, Ordering::SeqCst);
    let ready = t.since_process_start();
    // Recovery has ended: what it decided starts now (2.7, step 9).
    engine.launch_recovered();
    t.step("launch_recovered");
    engine.run_pending();
    t.step("run_pending");
    let came = from.map_or_else(String::new, |from| format!(", reloaded from {from}"));
    let (commit, total) = (engine.opts.commit.clone(), t.since_process_start());
    t.say(format!("start total: {ready} ms to ready, {total} ms to the end of run_pending; runs {commit}{came}"));
    Ok((engine, rx))
}

impl Engine {
    // A train clone no card's candidate and no job holds is a stop's leftover: it goes (fac-68).
    fn sweep_trains(&mut self) {
        let kept = mork_core::queue::candidates::clones(&self.state);
        let Ok(rd) = std::fs::read_dir(self.opts.home.trains()) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !kept.contains(&name) && std::fs::remove_dir_all(e.path()).is_err() {
                self.log(&format!("the train clone {name} couldn't be removed"));
            }
        }
    }

    // The reload's note, and the fallback's when the jobs can't run in the slice (fac-143).
    fn say_at_start(&mut self, reload: Option<builds::ReloadNote>, fallback: Option<String>) {
        if let Some(note) = reload {
            self.say_once(note);
        }
        if let Some(why) = fallback.filter(|_| !self.store.said(FALLBACK).unwrap_or(true)) {
            let text = format!(
                "Suites, checks, builds and turns run under `nice -n 19 ionice -c3`, not in {}: {why}.",
                jobs::slice::SLICE
            );
            self.apply(&Event::EngineSays { place: "main".into(), text, key: Some(FALLBACK.into()) });
        }
    }

    // Said once across crashes: `pin.next` goes after (M1.15). What the reload brought is read only for a
    // line still unsaid, and a read that fails is said, never in the reload's way (fac-75).
    fn say_once(&mut self, note: builds::ReloadNote) {
        let mut said = None;
        if !self.store.said(&note.key).unwrap_or(false) {
            let text = match &note.builds {
                Some((from, to)) => mork_core::maintenance::changelog::note(&note.text, self.brought(from, to)),
                None => note.text,
            };
            said = note.builds.is_some().then(|| text.clone());
            self.apply(&Event::EngineSays { place: "main".into(), text, key: Some(note.key.clone()) });
        }
        if self.store.said(&note.key).unwrap_or(false) {
            // Queued only once committed in main: a crash between loses the wake rather than sending it twice.
            if let Some(text) = said {
                self.wakes_reloaded(text);
            }
            builds::reload_said(&self.opts.home);
        }
    }

    fn brought(&self, from: &str, to: &str) -> Result<mork_core::maintenance::changelog::Brought, String> {
        let (range, at) = builds::changelog::range(&self.opts.home, &self.ctx.config, from, to)
            .map_err(|e| e.to_string().lines().next().unwrap_or_default().to_owned())?;
        // A card in the range landed after its head was committed: from the range's earliest, a day given to clocks.
        let since = at.map_or(i64::MIN, |t| t - 86_400_000);
        // An empty range, a reload to the same build, has no landing to read.
        let rows = if range.is_empty() {
            Vec::new()
        } else {
            self.store.events_since("card.landed", since).map_err(|e| e.to_string())?
        };
        let new: std::collections::BTreeSet<&str> = range.iter().map(|c| c.sha.as_str()).collect();
        let landed: Vec<_> = rows
            .into_iter()
            .filter_map(|(time, card, _, data)| {
                let id = mork_types::CardId::new(card?);
                let head = data.get("head").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
                // An ended card's landing in the range is read from the store; one outside it isn't listed.
                let after = data.get("after").and_then(serde_json::Value::as_str);
                let unloaded = !self.state.cards.contains_key(&id)
                    && (new.contains(head.as_str()) || after.is_some_and(|a| new.contains(a)));
                let (stored, landing) =
                    unloaded.then(|| self.store.landing_of(id.as_str()).ok()).flatten().unwrap_or_default();
                let title = self.state.cards.get(&id).map(|c| c.title.clone()).or(stored);
                let title = title.or_else(|| self.state.board.get(&id).map(|b| b.title.clone())).unwrap_or_default();
                let actions = self.state.actions.values().chain(&landing);
                let (before, after) = mork_core::maintenance::changelog::bounds(&data, actions, id.as_str(), time);
                Some(mork_core::maintenance::changelog::Landed {
                    card: id.as_str().to_owned(),
                    head,
                    title,
                    before,
                    after,
                })
            })
            .collect();
        Ok(mork_core::maintenance::changelog::brought(&range, &landed))
    }

    // The material the last start journaled is the build before this one's (fac-14) when it is the reload's source;
    // with no reload, unless the pin moved since to another build, which may have run unjournaled: then unknown.
    fn material_read(&mut self, source: Option<&str>) {
        let last = self.store.last_journaled("material").ok().flatten();
        let previous = last.and_then(|(time, v)| {
            let recorded_build = v.get("build").and_then(|b| b.as_str());
            let elsewhere = match source {
                Some(src) => recorded_build != Some(src),
                None => builds::moved_to_since(&self.opts.home, time).iter().any(|to| *to != self.opts.commit),
            };
            let digests = serde_json::from_value(v.get("digests")?.clone()).ok()?;
            let build = v.get("build").and_then(|b| b.as_str()).map(str::to_owned);
            let texts = v.get("texts").and_then(|t| serde_json::from_value(t.clone()).ok()).unwrap_or_default();
            let mut recorded = mork_core::maintenance::material::Recorded { digests, build, texts };
            if elsewhere {
                return None;
            }
            builds::material::recover(&self.opts.home, &mut recorded);
            Some(recorded)
        });
        self.apply(&Event::MaterialRead { previous, build: self.opts.commit.clone() });
    }

    /// The state as loaded, for tests of the engine's own loop.
    pub fn state(&self) -> &State {
        &self.state
    }
}
