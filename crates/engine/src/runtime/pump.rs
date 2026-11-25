// Launching (1.4.3): an owed turn becomes a job's intent, then its folder, then its supervisor.
// Every step is in the state before the next, so a crash anywhere is recovered.

use std::collections::BTreeMap;
use std::mem::take;
use std::path::{Path, PathBuf};

use factory_core::agents::closing;
use factory_core::agents::turns::{self, TurnPlan};
use factory_core::decide::Event;
use factory_types::config::{Cli, Seat};
use factory_types::job::{JobSpec, WallMode};
use factory_types::state::{Job, JobStatus};
use factory_types::{AgentKey, JobId};

use super::{Engine, Live, Phase};
use crate::jobs::context::wall_unavailable;
use crate::jobs::{folder, launch};
use crate::{cli, clock, faults, random};

// The turn's prompt; a retry's list of images names them where this turn has them.
fn turn_prompt(plan: &TurnPlan, carried: Option<PathBuf>, shown: &[String], io: &str) -> String {
    let mut prompt = plan.prompt.clone();
    if let Some(old_io) = carried {
        prompt = prompt.replace(&old_io.display().to_string(), io);
    } else if !shown.is_empty() {
        prompt.push_str("\n\nAttached images; view each with Read:\n");
        prompt.push_str(&shown.join("\n"));
        prompt.push('\n');
    }
    prompt
}

// A turn's two folders (fac-654): its seat's, its `$FACTORY_IO` for the seat's lifetime, named by its first turn; and
// the job's own `io/`, holding the schema and last message its CLI reads and writes, out of the seat's reach.
struct TurnIo {
    seat: PathBuf,
    job: PathBuf,
}

#[derive(Debug, Default)]
struct TurnPlace {
    cwd: String,
    read_only: Vec<String>,
    read_write: Vec<String>,
    env: BTreeMap<String, String>,
    toolchains: Vec<factory_types::config::Toolchain>,
    // An empty tmpfs at the clone's path: a closing turn resumes where its session ran, the clone unseen (fac-147).
    empty: Option<String>,
    // Its project's data folder, read-only at `/data` (fac-173).
    data: Option<String>,
}

/// A job's record from its spec: `launching`.
pub fn job_of(spec: JobSpec, stdin: String, io_files: BTreeMap<String, String>) -> Job {
    Job {
        id: spec.id.clone(),
        kind: spec.kind,
        owner: spec.owner.clone(),
        spec,
        stdin,
        io_files,
        status: JobStatus::Launching,
        settled: None,
        settled_ms: None,
        stop_requested: false,
        detached: false,
        interrupted: false,
        corrects: None,
        retry_of: None,
        continues: None,
        opening: false,
        io_copies: BTreeMap::new(),
        purpose: None,
        limits_at: 0,
        head: None,
        checks_at: None,
        seen: None,
        delivered: Vec::new(),
        read: None,
        staged: BTreeMap::new(),
    }
}

impl Engine {
    /// Owed turns become jobs; nothing new starts while a reload is under way.
    pub(super) fn pump(&mut self) {
        if matches!(self.phase, Phase::Reloading { .. }) {
            return;
        }
        self.sweep_closing_dirs();
        for key in turns::due(&self.state, &self.ctx.config, clock::now_ms()) {
            // An unwalled seat's turn needs no wall (fac-25).
            let seat = self.seat_of(&key);
            let mode = seat.as_ref().map_or(self.ctx.mode, |s| self.ctx.wall_of(s));
            if let Some(reason) = wall_unavailable(mode, &self.ctx.bwrap) {
                self.apply(&Event::WallMissing { agent: key, reason });
                continue;
            }
            if mode == WallMode::None
                && let Err(e) = self.closing_dir(&key)
            {
                // Its closing turn ends unasked rather than wait on a folder (fac-147).
                self.log(&format!("{key}: no closing folder: {e}"));
                self.apply(&Event::TurnUnmade { agent: key, reason: format!("no closing folder: {e}") });
                continue;
            }
            let ctx =
                factory_core::decide::Ctx { now: clock::now_ms(), config: &self.ctx.config, faults: self.faults() };
            let codex_exec = seat.is_ok_and(|s| s.cli == Cli::Codex && mode == WallMode::None);
            let Some(plan) = turns::plan(&self.state, &key, ctx, codex_exec) else { continue };
            let (job, session) = match self.turn_job(&plan) {
                Ok(j) => j,
                Err(e) => {
                    // Held until something changes, so it is said once, not every tick (fac-146).
                    self.log(&format!("{key}: no turn: {e}"));
                    self.apply(&Event::TurnUnmade { agent: key, reason: format!("no turn: {e}") });
                    continue;
                }
            };
            let id = job.id.clone();
            faults::hit("job.before-intent");
            self.apply(&Event::TurnPrepared { job: Box::new(job), session });
            faults::hit("job.after-intent");
            self.launch(&id);
        }
    }

    // The seat its next turn launches as: a pending pick's model and effort over it (fac-96). A
    // closing turn asks the session as it ran, keeping its effort (fac-147, fac-670).
    fn seat_of(&self, key: &AgentKey) -> Result<Seat, String> {
        let a = self.state.agents.get(key).ok_or("no such agent")?;
        let seat = if closing::ending_turn(a) {
            factory_core::agents::models::seat(&self.ctx.config, a)
        } else {
            factory_core::agents::models::launching(&self.state, &self.ctx.config, a)
        };
        seat.ok_or_else(|| {
            format!("`{key}` has no allowed model and effort: select both with `pick` and check its seat")
        })
    }

    fn turn_job(&self, plan: &TurnPlan) -> Result<(Job, Option<String>), String> {
        let a = self.state.agents.get(&plan.agent).ok_or("no such agent")?;
        let seat = self.seat_of(&plan.agent)?;
        cli::agy::walled(&seat, self.ctx.wall_of(&seat))?;
        let now = clock::now_ms();
        let id = factory_core::state::names::job_id(now, &plan.agent, random::salt());
        let io = TurnIo {
            seat: self.opts.home.seat_io(a.io.as_deref().unwrap_or(id.as_str())),
            job: self.opts.home.job(id.as_str()).join("io"),
        };
        let (io_s, job_io) = (io.seat.display().to_string(), io.job.display().to_string());
        let schema = factory_core::agents::reply::schema_of(&self.state, a, seat.cli);
        let mut place = if closing::ending_turn(a) { self.closing_place(a)? } else { self.turn_place(a)? };
        let (copies, shown, carried) = self.turn_images(plan, &id, &io.seat);
        let image_dir = (!copies.is_empty()).then(|| io.seat.join("images").display().to_string());
        let skills = self.ctx.skills.as_ref().map(|d| d.display().to_string());
        let (argv, live, session) = match seat.cli {
            Cli::Claude => {
                let session = plan.session.as_ref().map_or_else(random::uuid, ToString::to_string);
                let dirs = cli::claude::Dirs { images: image_dir.as_deref(), skills: skills.as_deref() };
                let cap = self.ctx.config.factory.context_cap();
                let argv =
                    cli::claude::argv(&seat, &session, plan.resume, &schema, dirs, self.style(a).as_deref(), cap);
                let live = factory_types::job::Live::Claude { session: session.clone() };
                (argv, Some(live), Some(session))
            }
            Cli::Codex => {
                let (argv, live) = self.codex_turn(plan, &seat, &job_io, &place.cwd, &shown);
                (argv, live, None)
            }
            Cli::Agy => (cli::agy::argv(&seat, plan.session.as_deref().filter(|_| plan.resume), &job_io), None, None),
        };
        let mut env = self.ctx.env(&id, &plan.agent, Some(&seat), &place.toolchains, &io.seat);
        env.extend(std::mem::take(&mut place.env));
        let mut places = self.turn_places(&io, &mut place, &seat)?;
        places.blanked = self.blanked(&id, live.as_ref(), &env);
        // A retry in its request's seat folder finds its images where that request's text names them (fac-654).
        let moved = if carried.as_deref() == Some(io.seat.as_path()) { &[][..] } else { shown.as_slice() };
        let short = self.short_retry(plan, seat.cli, &env, &place.cwd, moved);
        let mut spec = self.ctx.spec(
            &id,
            factory_types::job::JobKind::Turn,
            &plan.agent,
            argv,
            &place.cwd,
            &places,
            env,
            // Any port: a test the agent runs serves on loopback ports of its own (fac-93).
            None,
            now,
        );
        spec.wall = self.ctx.wall_of(&seat);
        spec.live = live;
        let files = BTreeMap::from([("schema.json".to_owned(), schema.to_string())]);
        let (stdin, staged) = short.unwrap_or_else(|| (turn_prompt(plan, carried, &shown, &io_s), plan.staged.clone()));
        let mut job = job_of(spec, stdin, files);
        // Each try of an opening request in the session it began, short or whole, names its first and runs one.
        job.continues = plan.short.as_ref().and(plan.request.clone());
        job.opening = plan.corrects.is_none() && (!plan.resume || job.continues.is_some());
        job.corrects = plan.corrects.clone();
        job.io_copies = copies;
        job.staged = staged;
        Ok((job, session))
    }

    // The seat's folder read-write; agy reads its schema by path, so that one file of the job's is read-only (fac-654).
    fn turn_places(
        &self,
        io: &TurnIo,
        place: &mut TurnPlace,
        seat: &Seat,
    ) -> Result<crate::jobs::wall::WallPlaces, String> {
        let (ro, rw, credentials) = (take(&mut place.read_only), take(&mut place.read_write), seat.credentials.clone());
        let walled = self.ctx.wall_of(seat) != WallMode::None;
        let mut places = if walled {
            self.ctx.places(&io.seat, ro, rw, credentials, &place.toolchains)?
        } else {
            self.ctx.unwalled_places(&io.seat, ro, rw, credentials, &place.toolchains)
        };
        if seat.cli == Cli::Agy {
            places.read_only.push(io.job.join("schema.json").display().to_string());
        }
        if let Some(skills) = &self.ctx.skills {
            crate::jobs::wall::skills_at(&self.opts.home.agent_home())?;
            places.skills = Some(skills.display().to_string());
        }
        places.empty = place.empty.take().filter(|_| walled);
        places.data = place.data.take();
        Ok(places)
    }

    // Walled, Codex runs its app-server, which messages steer; unwalled, `exec`, which they wait for (fac-389).
    fn codex_turn(
        &self,
        plan: &TurnPlan,
        seat: &Seat,
        job_io: &str,
        cwd: &str,
        shown: &[String],
    ) -> (Vec<String>, Option<factory_types::job::Live>) {
        let thread = if plan.resume { plan.session.as_deref() } else { None };
        let style = self.state.agents.get(&plan.agent).and_then(|a| self.style(a));
        let agents = self.opts.home.agent_home().join(".codex");
        let users = [agents.as_path(), self.ctx.app_codex_home.as_path()];
        let mcp = cli::codex::project_mcp_servers(Path::new(cwd), &users);
        if self.ctx.wall_of(seat) == WallMode::None {
            return (cli::codex::argv(seat, thread, job_io, &mcp, shown, style.as_deref()), None);
        }
        let (thread, images, model) = (thread.map(str::to_owned), shown.to_vec(), Some(seat.model.clone()));
        let live = factory_types::job::Live::Codex { thread, images, model, effort: Some(seat.effort) };
        (cli::codex::app_server_argv(seat, &mcp, style.as_deref()), Some(live))
    }

    // A planner's or foreman's output style, read at each launch (fac-596); one that can't be read is logged and left out.
    fn style(&self, a: &factory_types::state::Agent) -> Option<String> {
        let asks = matches!(a.role, factory_types::reply::Role::Planner | factory_types::reply::Role::Foreman);
        let home = std::env::var_os("HOME").filter(|h| asks && !h.is_empty())?;
        let style = crate::settings::read_style(Path::new(&home));
        style.map_err(|e| self.log(&format!("{}: launched without Andrei's output style: {e}", a.key))).ok()
    }

    // Its app-server reads the user config `exec` was told to ignore, so the wall shows that config empty.
    fn blanked(
        &self,
        id: &JobId,
        live: Option<&factory_types::job::Live>,
        env: &BTreeMap<String, String>,
    ) -> Vec<(String, String)> {
        if !matches!(live, Some(factory_types::job::Live::Codex { .. })) {
            return Vec::new();
        }
        let empty = folder::JobDir::new(self.opts.home.job(id.as_str())).empty().display().to_string();
        let home = env.get("HOME").map(|h| format!("{h}/.codex"));
        let Some(codex) = env.get("CODEX_HOME").cloned().or(home) else { return Vec::new() };
        ["config.toml", "environments.toml"].map(|f| (empty.clone(), format!("{codex}/{f}"))).into()
    }

    // The handoff stays available to seats whose work must leave the clone read-only.
    fn turn_place(&self, a: &factory_types::state::Agent) -> Result<TurnPlace, String> {
        let card =
            factory_core::messages::places::card_of(&self.state, &a.place).and_then(|c| self.state.cards.get(&c));
        let Some(card) = card else {
            let work = self.opts.home.agent_home().join("work").join(a.key.as_str());
            std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
            return Ok(TurnPlace { cwd: work.display().to_string(), ..TurnPlace::default() });
        };
        let clone = self.opts.home.root().join("clones").join(card.id.as_str());
        if !clone.is_dir() {
            return Err(format!("{} has no clone", card.id));
        }
        let clone_s = clone.display().to_string();
        // A foreman's researcher reads the clone from a folder of its own (fac-94).
        let researches = a.parent.is_some() && a.role == factory_types::reply::Role::Researcher;
        let own = self.opts.home.agent_home().join("work").join(a.key.as_str());
        let (mut read_only, read_write) = if researches {
            std::fs::create_dir_all(&own).map_err(|e| e.to_string())?;
            (vec![clone_s.clone()], vec![own.display().to_string()])
        } else if a.role == factory_types::reply::Role::Planner && factory_core::lifecycle::mock::writes(card) {
            (vec![clone_s.clone()], vec![handoff_dir(&clone)?, mock_dir(&clone)?])
        } else if a.role == factory_types::reply::Role::Baseline {
            // The baseline seat writes the mock's folder alone, never the handoff (fac-636).
            (vec![clone_s.clone()], vec![mock_dir(&clone)?])
        } else if a.role == factory_types::reply::Role::Planner
            || (card.mode == factory_types::card::Mode::Report && a.role == factory_types::reply::Role::Foreman)
        {
            (vec![clone_s.clone()], vec![handoff_dir(&clone)?])
        } else if factory_core::lifecycle::facts::clone_writable(card) {
            (Vec::new(), vec![clone_s.clone()])
        } else {
            (vec![clone_s.clone()], Vec::new())
        };
        let cwd = if researches { own.display().to_string() } else { clone_s.clone() };
        let mut env = BTreeMap::from([
            ("FACTORY_CARD".to_owned(), card.id.as_str().to_owned()),
            ("FACTORY_PROJECT".to_owned(), card.project.to_string()),
        ]);
        if researches {
            env.insert("FACTORY_CLONE".into(), clone_s);
        }
        let project = self.ctx.config.projects.get(&card.project);
        // The checks folder's checkout, read-only, for `factory test --checks` (4.9.1, fac-165).
        read_only.extend(project.and_then(|p| p.checks.as_ref()).map(|k| self.checks_mount(&k.folder)));
        if let Some(port) = card.port {
            env.insert("FACTORY_PORT".into(), port.to_string());
        }
        let toolchains = project.map(|p| p.toolchains.clone()).unwrap_or_default();
        let data = project.and_then(|p| p.data.clone());
        Ok(TurnPlace { cwd, read_only, read_write, env, toolchains, empty: None, data })
    }

    // Where a closing turn runs: where its session ran, as Claude finds a session by its folder
    // (fac-147). A clone's path, the clone there or not; whether it is one.
    fn closing_cwd(&self, a: &factory_types::state::Agent) -> (PathBuf, bool) {
        let researcher = a.role == factory_types::reply::Role::Researcher;
        match factory_core::messages::places::card_of(&self.state, &a.place) {
            Some(c) if !(researcher && a.parent.is_some()) => (self.clone_dir(c.as_str()), true),
            _ => (self.opts.home.agent_home().join("work").join(a.key.as_str()), false),
        }
    }

    // Nothing of a clone: walled, an empty tmpfs at its path; else the folder `closing_dir` made
    // where it is gone, or the clone as it is.
    fn closing_place(&self, a: &factory_types::state::Agent) -> Result<TurnPlace, String> {
        let (dir, clone) = self.closing_cwd(a);
        let cwd = dir.display().to_string();
        if clone {
            return Ok(TurnPlace { cwd: cwd.clone(), empty: Some(cwd), ..TurnPlace::default() });
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(TurnPlace { cwd: cwd.clone(), read_write: vec![cwd], ..TurnPlace::default() })
    }

    // An unwalled closing turn needs its gone clone's path on the host: an empty folder, kept
    // until no turn is in it.
    fn closing_dir(&mut self, key: &AgentKey) -> std::io::Result<()> {
        let Some(a) = self.state.agents.get(key).filter(|a| a.retiring.is_some()) else { return Ok(()) };
        let (dir, clone) = self.closing_cwd(a);
        if clone && !dir.exists() {
            std::fs::create_dir_all(&dir)?;
            self.closing_dirs.insert(dir);
        }
        Ok(())
    }

    // Removed once no job runs in it, so nothing takes it for a clone; one a clone was made in
    // since, its `.git` there, is left alone.
    fn sweep_closing_dirs(&mut self) {
        let busy: Vec<PathBuf> =
            self.state.jobs.values().filter(|j| j.settled.is_none()).map(|j| PathBuf::from(&j.spec.cwd)).collect();
        self.closing_dirs.retain(|d| {
            if busy.contains(d) {
                return true;
            }
            if !d.join(".git").exists() {
                let _ = std::fs::remove_dir_all(d);
            }
            false
        });
    }

    /// Writes a recorded job's folder (when it isn't there), its images into its `$FACTORY_IO`, and starts its
    /// supervisor.
    pub(super) fn launch(&mut self, id: &JobId) {
        let Some(job) = self.state.jobs.get(id).cloned() else { return };
        let mut files: Vec<(String, Vec<u8>)> =
            job.io_files.iter().map(|(k, v)| (k.clone(), v.clone().into_bytes())).collect();
        let mut copies = Vec::new();
        for (rel, src) in &job.io_copies {
            match super::turn_images::read_image(&self.opts.home, src) {
                Ok(bytes) => copies.push((rel.clone(), bytes)),
                Err(e) => self.log(&format!("{id}: the image {src} couldn't be read: {e}")),
            }
        }
        let dir = folder::JobDir::new(self.opts.home.job(id.as_str()));
        let io = dir.factory_io(&job.spec);
        // A seat's folder is made before its wall binds it, its images written through no link it planted (fac-654).
        if io == dir.io() {
            files.append(&mut copies);
        } else if let Err(e) = folder::make_seat_io(&io) {
            self.log(&format!("{id}: its io/ folder couldn't be made: {e}"));
            return;
        }
        // An image the seat's own links keep out is left out, as one that can't be read is.
        for (rel, bytes) in &copies {
            if let Err(e) = folder::write_seat_file(&io, rel, bytes) {
                self.log(&format!("{id}: the image {rel} couldn't be written to its io/ folder: {e}"));
            }
        }
        let stdin = cli::agy::stdin(&job.spec.argv, &job.stdin);
        let new = folder::NewFolder { spec: &job.spec, stdin: stdin.as_bytes(), io_files: &files };
        if let Err(e) = folder::write(&self.opts.home.jobs(), &new) {
            self.log(&format!("{id}: the folder couldn't be written: {e}"));
            return;
        }
        self.drop_sources(&job);
        faults::hit("job.after-folder");
        self.start_supervisor(id);
        faults::hit("job.after-spawn");
    }

    /// Starts a job's supervisor; only the first this process starts gets a `sup.*` fault.
    pub(super) fn start_supervisor(&mut self, id: &JobId) {
        let dir = self.opts.home.job(id.as_str());
        let pass = self.supervisors == 0;
        self.supervisors += 1;
        if let Err(e) = launch::start_supervisor(&self.opts.bin, &dir, self.opts.home.root(), pass) {
            self.log(&format!("{id}: {e}"));
        }
        let cli = super::watch::cli_of(&self.state, id);
        let mut stream = self.stream_for(id);
        if let Some(job) = self.state.jobs.get(id) {
            // A resumed session's planner keeps its list: its open steps carry into the turn (fac-128).
            if job.spec.argv.iter().any(|a| a == "--resume")
                && let Some(list) = self.seen.get(&job.owner).and_then(|s| s.list.as_ref())
            {
                let open = list.steps.iter().filter(|s| s.status != factory_types::seen::StepStatus::Completed);
                stream.list = open.cloned().collect();
            }
            self.seen.insert(job.owner.clone(), super::AgentSeen::default());
        }
        let stamps = crate::jobs::stamps::Stamps::load(self.job_dir(id).stamps());
        let live = Live {
            offset: 0,
            stream,
            spawned: clock::instant(),
            cli,
            reported: Vec::new(),
            replayed_to: 0,
            stamps,
            ended: false,
        };
        self.live.insert(id.clone(), live);
    }

    /// A stream's reader, with the folder its paths are shortened against.
    pub(super) fn stream_for(&self, id: &JobId) -> cli::StreamState {
        let (cwd, home) = match self.state.jobs.get(id) {
            Some(j) => (j.spec.cwd.clone(), self.opts.home.agent_home().display().to_string()),
            None => (String::new(), String::new()),
        };
        let mut st = cli::StreamState::for_folder(&cwd, &home);
        st.seat = self.state.jobs.get(id).map(|j| j.owner.as_str().to_owned());
        st.account = self
            .job_dir(id)
            .usage_account()
            .filter(|account| self.views_cache.claude_accounts.live.as_ref().is_some_and(|live| live.id == account.id));
        // The window its model was last reported with holds until a result says otherwise (M2.9).
        if let Some((model, window)) =
            self.state.jobs.get(id).and_then(|j| self.state.agents.get(&j.owner)?.window.clone())
        {
            st.windows.insert(model, window);
        }
        st
    }
}

// Made beneath the clone through no link, so the wall mounts a real folder and `.git` is never made.
fn handoff_dir(clone: &Path) -> Result<String, String> {
    let handoff = Path::new(factory_core::memory::HANDOFF);
    for rel in [handoff.parent().unwrap_or(handoff), handoff] {
        factory_sys::mkdir_beneath(clone, rel).map_err(|e| format!("{}: {e}", clone.join(rel).display()))?;
    }
    Ok(clone.join(handoff).display().to_string())
}

// The open mock's writable mount must be a real folder, never a link planted by another seat.
fn mock_dir(clone: &Path) -> Result<String, String> {
    let rel = Path::new(factory_core::lifecycle::mock::FOLDER);
    factory_sys::mkdir_beneath(clone, rel).map_err(|e| format!("{}: {e}", clone.join(rel).display()))?;
    Ok(clone.join(rel).display().to_string())
}
