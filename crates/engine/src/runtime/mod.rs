//! The engine's life: start up (2.7), take commands, launch and watch jobs, keep up, and
//! shut down cleanly.

mod act_board;
mod act_forge;
mod act_found;
mod act_run;
mod actions;
mod bundles;
mod burn;
mod candidate_read;
mod chat;
mod commands;
mod deliver;
mod door;
pub mod handoff;
mod hooks;
mod journal;
mod limits_read;
pub(crate) mod locate;
mod lookback;
mod mock_kept;
mod mocks;
mod outside;
mod pipe;
mod previews;
mod pump;
mod queue_work;
mod records;
mod recover;
mod reloads;
mod retries;
mod review_proofs;
mod review_threads;
mod routing;
mod say_images;
mod search;
mod settings;
mod slots;
mod startup;
mod suite_logs;
mod suite_watch;
mod turn_images;
mod unloaded;
mod updates;
pub(crate) mod upkeep;
mod usage;
mod verdict;
mod views;
mod wakes;
mod wall_runs;
mod watch;
mod work;
mod work_end;
mod work_plan;
mod work_threads;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use mork_core::decide::{Ctx, Decision, Event, Intent};
use mork_core::state::State;
use mork_types::JobId;

use crate::cli::StreamState;
use crate::jobs::context::Context;
use crate::lock::Lock;
use crate::paths::Home;
use crate::sockets::{Conn, Incoming, Shared};
use crate::store::Store;
use crate::{clock, faults};

/// How the app starts the engine.
#[derive(Debug, Clone)]
pub struct Options {
    pub home: Home,
    pub config_path: PathBuf,
    /// The running binary.
    pub bin: PathBuf,
    pub commit: String,
    pub dirty: bool,
    /// When the process began to start, for the start's timing lines.
    pub began: Instant,
    /// Started by a reload: its id (`--reloaded <id>`).
    pub reloaded: Option<String>,
    /// Only an interactive start opens the dashboard.
    pub open_web: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Ready,
    /// A reload is under way: outward commands are refused (1.8.6.2); it execs at `at`.
    Reloading {
        target: String,
        id: String,
        at: Instant,
        who: String,
    },
}

/// What the engine knows of a job it watches, in memory only (1.4.7).
#[derive(Debug, Clone)]
pub struct Live {
    pub offset: u64,
    pub stream: StreamState,
    pub spawned: Instant,
    pub cli: Option<mork_types::config::Cli>,
    /// The limits last passed on, so a stream read again passes nothing new.
    pub reported: Vec<crate::cli::limits::Window>,
    /// Reattached after a restart: what the stream held up to here was passed on before it, and
    /// is no newer than the state (1.4.7.2).
    pub replayed_to: u64,
    /// When the stream's parts were read (fac-206).
    pub stamps: crate::jobs::stamps::Stamps,
    /// Its processes were asked to end once its report was read (fac-321).
    pub ended: bool,
}

/// What the views show of an agent's latest turn, kept in memory (1.4.7.2).
#[derive(Debug, Clone, Default)]
pub struct AgentSeen {
    pub steps: Vec<String>,
    pub doing: Option<String>,
    pub context: Option<crate::cli::ContextReport>,
    /// Its steps with their text and thinking, of the job that read them (fac-96).
    pub turn: Option<Arc<mork_types::seen::TurnView>>,
    /// Its own step list, of the job that read it (fac-128).
    pub list: Option<mork_types::seen::StepList>,
}

/// A CLI's windows as the state's limits.
pub fn limits_of(
    cli: mork_types::config::Cli,
    windows: &[crate::cli::limits::Window],
    now: i64,
) -> Vec<mork_types::usage::Limit> {
    windows
        .iter()
        .filter(|w| cli != mork_types::config::Cli::Claude || w.account.is_some())
        .map(|w| mork_types::usage::Limit {
            cli,
            account: w.account.clone(),
            bucket: w.bucket.clone(),
            window: w.window.clone(),
            percent: w.percent,
            resets_at: w.resets_at,
            reached: w.reached,
            seen_ms: w.seen_ms.unwrap_or(now),
        })
        .collect()
}

/// What a connection watches: its places, each view with only its latest `tail` messages when
/// one is given (fac-42), and the whole view only when `full`, else the conversation alone (fac-280).
#[derive(Debug, Clone)]
pub struct Watched {
    pub places: Vec<String>,
    pub tail: Option<u64>,
    pub full: bool,
    // The hash excludes the revision so unrelated state changes send nothing (fac-321).
    sent: Arc<Lock<BTreeMap<String, u64>>>,
}

impl Watched {
    pub fn new(places: Vec<String>, tail: Option<u64>, full: bool) -> Self {
        Self { places, tail, full, sent: Arc::default() }
    }

    /// Whether this place's view says what the last one sent did, noting `view` as the last.
    pub(super) fn unchanged(&self, place: &str, view: &mut serde_json::Value) -> bool {
        use std::hash::{Hash, Hasher};
        let revision = view.as_object_mut().and_then(|o| o.remove("revision"));
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        view.to_string().hash(&mut hasher);
        let hash = hasher.finish();
        if let (Some(r), Some(o)) = (revision, view.as_object_mut()) {
            o.insert("revision".into(), r);
        }
        self.sent.lock().insert(place.to_owned(), hash) == Some(hash)
    }
}

pub struct Engine {
    routing: routing::Cache,
    loaded_config: String,
    bound_web_port: Option<u16>,
    reserved_ports: std::collections::BTreeSet<u16>,
    pub opts: Options,
    pub ctx: Context,
    pub store: Store,
    pub state: State,
    pub shared: Arc<Shared>,
    pub phase: Phase,
    pub live: BTreeMap<JobId, Live>,
    pub watchers: Vec<(Arc<Conn>, Watched)>,
    pub tx: Sender<Input>,
    pub last_upkeep: Option<Instant>,
    /// Kept open for a reload's exec: the lock and the two listeners' descriptors.
    pub keep: startup::Kept,
    /// Each agent's latest turn, as its stream showed it.
    pub seen: BTreeMap<mork_types::AgentKey, AgentSeen>,
    /// Codex's limits reader (1.4.8, Codex 7).
    pub limits_read: limits_read::Reader,
    /// The pipeline's intents, slots and poller (section 4).
    pub pipe: pipe::Pipe,
    /// The views' reads of the world, kept between ticks (section 5).
    pub views_cache: outside::Cache,
    /// The door's requests waiting for their calls (fac-1).
    pub door: door::DoorRt,
    /// The orchestrators' open hooks, by project (fac-119, fac-244).
    pub hooks: hooks::Hooks,
    /// The haiku runs that wake the orchestrators (fac-265).
    pub wakes: wakes::Wakes,
    /// The empty folders unwalled closing turns run in where a clone was (fac-147), removed once
    /// no turn is in them.
    pub closing_dirs: std::collections::BTreeSet<std::path::PathBuf>,
    /// The full cards' preview workers (fac-196).
    pub previews: previews::Previews,
    /// The visual cards' mock hosts (fac-258).
    pub mocks: mocks::Mocks,
    /// The live cards' suite runs, read as they run (fac-697).
    pub suites: BTreeMap<JobId, suite_watch::SuiteRead>,
}

pub enum Input {
    Routing(Result<Box<mork_types::routing::Statistics>, String>),
    Socket(Incoming),
    /// A delayed answer (`debug.sleep`).
    Answer {
        conn: Arc<Conn>,
        id: u64,
        ok: serde_json::Value,
    },
    /// What a thread found (an action's answer, a poll), for `core`.
    Event(Box<Event>),
    /// A clone's completed thread read, kept off the engine's loop (fac-448).
    ReviewThreads {
        job: JobId,
        end: Box<mork_core::decide::JobEnd>,
    },
    WorkPrepared(Box<work_threads::Prepared>),
    CandidateRead {
        job: JobId,
        end: Box<mork_core::decide::JobEnd>,
    },
    /// The poller asked Bitbucket who the token is, whatever it answered (3.7.2).
    TokenAsked(mork_types::ProjectId),
    /// The views' reads of the world, made on their thread.
    Outside(Box<outside::Reads>),
    /// The machine's load, sampled on its own thread.
    Resources(mork_types::view::Resources),
    /// The kill of a read's query tried again, as the same query was asked again (fac-184).
    DoorKilled(Box<door::Killed>),
}

#[derive(Debug)]
pub enum StartError {
    AlreadyRunning(String),
    Failed(String),
}

impl From<String> for StartError {
    fn from(reason: String) -> Self {
        Self::Failed(reason)
    }
}

/// Runs the engine until shutdown, returning a distinct error when the home is in use.
pub fn run(opts: Options) -> Result<i32, StartError> {
    let (mut engine, rx) = startup::start(opts)?;
    if engine.opts.open_web && !mork_sys::term_requested() {
        engine.open_dashboard();
    }
    let code = engine.serve(&rx);
    engine.shutdown();
    Ok(code)
}

impl Engine {
    fn dashboard_port(&self) -> Result<u16, &'static str> {
        self.ctx.config.mork.web_port.ok_or("[mork] web_port is unset")?;
        let port = self.bound_web_port.ok_or("the web bridge failed to start")?;
        crate::web::dashboard(self.ctx.config.mork.web_assets.as_deref())?;
        Ok(port)
    }

    fn open_dashboard(&self) {
        // A configured bridge already reported the specific startup error on stderr.
        if self.ctx.config.mork.web_port.is_some() && self.bound_web_port.is_none() {
            return;
        }
        match self.dashboard_port() {
            Ok(port) => {
                let address = crate::web::key::address(port);
                if let Err(e) = crate::jobs::launch::browser(self.opts.home.root(), &address) {
                    eprintln!("mork: couldn't open the dashboard: {e}");
                }
            }
            Err(why) => eprintln!("mork: couldn't open the dashboard: {why}"),
        }
    }

    pub fn faults(&self) -> bool {
        faults::TEST_SETTINGS
    }

    pub(super) fn decision_ctx(&self, now: i64) -> Ctx<'_> {
        Ctx {
            now,
            config: &self.ctx.config,
            routing: self.routing.stats.as_deref(),
            faults: self.faults(),
            claude_account: self.views_cache.claude_accounts.live.as_ref().map(|a| a.id.as_str()),
        }
    }

    /// Decides and commits one event; carries out its intents after the commit.
    pub fn apply(&mut self, event: &Event) -> Decision {
        self.load_wanted(event);
        let now = clock::now_ms();
        let d = self.decide(event, now);
        self.commit_decided(d, now)
    }

    /// Decides one event on the state as it stands, committing nothing.
    pub(super) fn decide(&self, event: &Event, now: i64) -> Decision {
        let ctx = self.decision_ctx(now);
        mork_core::decide::decide(&self.state, event, ctx)
    }

    /// Commits what [`Engine::decide`] decided, nothing committed since; carries out its intents after.
    pub(super) fn commit_decided(&mut self, d: Decision, now: i64) -> Decision {
        if let Err(e) = self.store.commit(&d.changes, &d.journal, now) {
            self.log(&format!("commit failed: {e}"));
            return Decision::refused(format!("the state couldn't be written: {e}"));
        }
        let used = self.used_bundles(&d.changes);
        let ended = self.cards_ending(&d.changes);
        self.work_changed(&d.changes);
        self.state.commit(&d.changes);
        self.drop_bundles(&used);
        self.wakes_note(&d.changes, &ended);
        if d.changes.iter().any(|c| matches!(c, mork_core::state::Change::AddOrchestratorEvent(_))) {
            self.hooks_send();
        }
        for (project, cfg) in &self.ctx.config.projects {
            if cfg.forge.hosted() {
                self.pipe.verdict.of(project).follow(self.state.world.token_refused.get(project));
            }
        }
        for intent in &d.intents {
            self.carry_out(intent);
        }
        // A door call that moved: its waiting requests answered, or its requester told (fac-1).
        for id in mork_core::door::touched(&d.changes) {
            self.door_settle(&id);
        }
        if !d.changes.is_empty() {
            self.publish();
        }
        d
    }

    fn carry_out(&mut self, intent: &Intent) {
        match intent {
            Intent::StopJob { id } => self.stop_job(id),
            // At once: the turn it is for may end any moment.
            Intent::Deliver { job, seq, text, images } => self.deliver(job, *seq, text, images),
            Intent::StartJob { .. }
            | Intent::RunAction { .. }
            | Intent::Notify { .. }
            | Intent::WriteRecord { .. }
            | Intent::ReadPr { .. }
            | Intent::SlotFreed
            | Intent::CopyProofs { .. }
            | Intent::RemoveHandoff { .. }
            | Intent::DropMock { .. } => self.pipe.later.push(intent.clone()),
        }
    }

    /// Appends to `logs/app.log`.
    /// A line in the app's log, its secrets masked (3.8.3, R15).
    pub fn log(&self, line: &str) {
        let mut cache = self.pipe.masks.lock();
        if cache.as_ref().is_none_or(|(at, _)| crate::clock::elapsed(*at) > std::time::Duration::from_secs(60)) {
            let s = crate::secrets::Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry);
            *cache = Some((crate::clock::instant(), s));
        }
        let masked = cache.as_ref().map_or_else(|| line.to_owned(), |(_, s)| s.mask(line));
        upkeep::log(&self.opts.home, &masked);
    }

    fn serve(&mut self, rx: &Receiver<Input>) -> i32 {
        loop {
            if mork_sys::term_requested() {
                return 0;
            }
            match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(Input::Socket(inc)) => self.incoming(inc),
                Ok(Input::Answer { conn, id, ok }) => conn.answer(id, Ok(ok)),
                Ok(Input::Event(e)) => self.event_in(&e),
                Ok(Input::ReviewThreads { job, end }) => self.review_threads_read(&job, *end),
                Ok(Input::WorkPrepared(p)) => self.work_prepared(*p),
                Ok(Input::CandidateRead { job, end }) => self.candidate_read(&job, *end),
                Ok(Input::TokenAsked(project)) => self.token_asked(project),
                Ok(Input::Routing(result)) => self.routing_read(result),
                Ok(Input::Outside(r)) => self.outside_read(*r),
                Ok(Input::Resources(r)) => self.resources_read(r),
                Ok(Input::DoorKilled(k)) => self.door_killed(*k),
                Err(_) => {}
            }
            self.tick();
        }
    }

    fn tick(&mut self) {
        self.pump();
        self.watch_jobs();
        self.watch_suites();
        self.pipe_tick();
        self.work_tick();
        self.hooks_tick();
        self.wakes_tick();
        self.read_limits();
        self.previews_tick();
        self.mocks_tick();
        self.restore_turns();
        self.refresh_outside();
        self.publish_steps();
        self.upkeep();
        self.reload_due();
    }

    // Jobs carry on: a shutdown takes nothing more (2.7).
    fn shutdown(&mut self) {
        self.shared.ready.store(false, std::sync::atomic::Ordering::SeqCst);
        self.end_limits_read();
        self.end_wakes();
        self.hooks_settle();
        self.previews_end();
        self.mocks_end();
        // Batches written during preview or mock teardown settle before exit (fac-665).
        self.settle_waiting_wakes();
    }
}
