// Launching (1.4.3): an owed turn becomes a job's intent, then its folder, then its supervisor.
// Every step is in the state before the next, so a crash anywhere is recovered.

#[path = "turn_place.rs"]
mod place;

use std::collections::BTreeMap;
use std::mem::take;
use std::path::{Path, PathBuf};

use mork_core::agents::closing;
use mork_core::agents::turns::{self, TurnPlan};
use mork_core::decide::Event;
use mork_types::config::{Cli, Seat};
use mork_types::job::{JobSpec, WallMode};
use mork_types::state::{Job, JobStatus};
use mork_types::{AgentKey, JobId};

use super::{Engine, Live, Phase};
use crate::jobs::context::wall_unavailable;
use crate::jobs::{folder, launch};
use crate::{cli, clock, random};

// The turn's prompt; a retry's list of images names them where this turn has them.
fn turn_prompt(plan: &TurnPlan, carried: Option<PathBuf>, shown: &[String], io: &str) -> String {
    let mut prompt = plan.prompt.clone();
    if let Some(old_io) = carried {
        let request = prompt.split_off(plan.specification_bytes);
        prompt.push_str(&request.replace(&old_io.display().to_string(), io));
    } else if !shown.is_empty() {
        prompt.push_str("\n\nAttached images; view each with Read:\n");
        prompt.push_str(&shown.join("\n"));
        prompt.push('\n');
    }
    prompt
}

// A turn's two folders (fac-654): its seat's, its `$MORK_IO` for the seat's lifetime, named by its first turn; and
// the job's own `io/`, holding the schema and last message its CLI reads and writes, out of the seat's reach.
struct TurnIo {
    seat: PathBuf,
    home: PathBuf,
    job: PathBuf,
}

impl TurnIo {
    fn new(home: &crate::paths::Home, previous: Option<&str>, id: &JobId) -> Self {
        let io_id = previous.unwrap_or(id.as_str());
        Self { home: home.seat_home(io_id), seat: home.seat_io(io_id), job: home.job(id.as_str()).join("io") }
    }
}

#[derive(Debug, Default)]
struct TurnPlace {
    cwd: String,
    project: Option<mork_types::ProjectId>,
    read_only: Vec<String>,
    read_write: Vec<String>,
    env: BTreeMap<String, String>,
    toolchains: Vec<mork_types::config::Toolchain>,
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
        specification_bytes: 0,
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
            let ctx = mork_core::decide::Ctx { now: clock::now_ms(), config: &self.ctx.config, faults: self.faults() };
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
            self.apply(&Event::TurnPrepared { job: Box::new(job), session });
            self.launch(&id);
        }
    }

    // The seat its next turn launches as: a pending pick's model and effort over it (fac-96). A
    // closing turn asks the session as it ran, keeping its effort (fac-147, fac-670).
    fn seat_of(&self, key: &AgentKey) -> Result<Seat, String> {
        let a = self.state.agents.get(key).ok_or("no such agent")?;
        let seat = if closing::ending_turn(a) {
            mork_core::agents::models::seat(&self.ctx.config, a)
        } else {
            mork_core::agents::models::launching(&self.state, &self.ctx.config, a)
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
        let id = mork_core::state::names::job_id(now, &plan.agent, random::salt());
        let io = TurnIo::new(&self.opts.home, a.io.as_deref(), &id);
        let (io_s, job_io) = (io.seat.display().to_string(), io.job.display().to_string());
        let schema =
            mork_core::agents::reply::schema_of(&self.state, a, seat.cli, self.ctx.config.mork.project.is_some());
        let mut place = self.turn_place(a)?;
        let (copies, shown, carried) = self.turn_images(plan, &id, &io.seat);
        let image_dir = (!copies.is_empty()).then(|| io.seat.join("images").display().to_string());
        let skills = self.ctx.skills.as_ref().map(|d| d.display().to_string());
        let (argv, live, session) = match seat.cli {
            Cli::Claude => {
                let session = plan.session.as_ref().map_or_else(random::uuid, ToString::to_string);
                let dirs = cli::claude::Dirs { images: image_dir.as_deref(), skills: skills.as_deref() };
                let cap = self.ctx.config.mork.context_cap();
                let argv =
                    cli::claude::argv(&seat, &session, plan.resume, &schema, dirs, self.style(a).as_deref(), cap);
                let live = mork_types::job::Live::Claude { session: session.clone() };
                (argv, Some(live), Some(session))
            }
            Cli::Codex => {
                let (argv, live) = self.codex_turn(plan, &seat, &job_io, &place.cwd, &io.home, &shown);
                (argv, live, None)
            }
            Cli::Agy => (cli::agy::argv(&seat, plan.session.as_deref().filter(|_| plan.resume), &job_io), None, None),
        };
        let mut env = self.ctx.env(&id, &plan.agent, Some(&seat), &place.toolchains, &io.seat);
        crate::seat_homes::env(&self.opts.home, &io.home, &mut env);
        env.extend(std::mem::take(&mut place.env));
        let mut places = self.turn_places(&io, &mut place, &seat)?;
        places.blanked = self.blanked(&id, live.as_ref(), &env);
        // A retry in its request's seat folder finds its images where that request's text names them (fac-654).
        let moved = if carried.as_deref() == Some(io.seat.as_path()) { &[][..] } else { shown.as_slice() };
        let short = self.short_retry(plan, seat.cli, &env, &place.cwd, moved);
        let mut spec =
            self.ctx.spec(&id, mork_types::job::JobKind::Turn, &plan.agent, argv, &place.cwd, &places, env, None, now);
        spec.wall = self.ctx.wall_of(&seat);
        spec.net = (spec.wall != WallMode::None).then(|| self.turn_net(a));
        spec.live = live;
        let files = BTreeMap::from([("schema.json".to_owned(), schema.to_string())]);
        let (stdin, staged) = short.unwrap_or_else(|| (turn_prompt(plan, carried, &shown, &io_s), plan.staged.clone()));
        let mut job = job_of(spec, stdin, files);
        // Each try of an opening request in the session it began, short or whole, names its first and runs one.
        job.continues = plan.short.as_ref().and(plan.request.clone());
        job.opening = plan.corrects.is_none() && (!plan.resume || job.continues.is_some());
        job.specification_bytes = plan.specification_bytes;
        job.corrects = plan.corrects.clone();
        job.io_copies = copies;
        job.staged = staged;
        Ok((job, session))
    }

    fn turn_net(&self, agent: &mork_types::state::Agent) -> mork_types::job::Net {
        let config = &self.ctx.config;
        let card =
            mork_core::messages::places::card_of(&self.state, &agent.place).and_then(|id| self.state.cards.get(&id));
        let mut domains = config.mork.domains.clone();
        let mut ports = std::collections::BTreeSet::new();
        if let Some(card) = card {
            if let Some(project) = config.projects.get(&card.project) {
                domains.extend(project.domains.iter().cloned());
                ports.extend(project.host_ports.iter().flat_map(|range| range.from..=range.to));
            }
            ports.extend(card.port);
            ports.extend(card.mock.and_then(|m| m.port));
        }
        for port in config.tunnels.values().map(|t| t.port).chain(self.bound_web_port) {
            ports.remove(&port);
        }
        mork_types::job::Net { domains, host_ports: ports.into_iter().collect() }
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
        folder::make_seat_io(&io.seat).map_err(|e| e.to_string())?;
        let author = self.ctx.config.git_author(place.project.as_deref());
        crate::seat_homes::prepare(&self.opts.home, &io.home, seat.cli, author, &mut places)
            .map_err(|e| format!("seat home: {e}"))?;
        if walled {
            crate::seat_homes::caches(&self.opts.home.caches(), &io.home, &self.ctx.config.mork, &mut places)
                .map_err(|e| format!("shared caches: {e}"))?;
        }
        if seat.cli == Cli::Agy {
            places.read_only.push(io.job.join("schema.json").display().to_string());
        }
        if let Some(skills) = &self.ctx.skills {
            crate::jobs::wall::skills_at(&io.home)?;
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
        home: &Path,
        shown: &[String],
    ) -> (Vec<String>, Option<mork_types::job::Live>) {
        let thread = if plan.resume { plan.session.as_deref() } else { None };
        let style = self.state.agents.get(&plan.agent).and_then(|a| self.style(a));
        let agents = home.join(".codex");
        let users = [agents.as_path(), self.ctx.app_codex_home.as_path()];
        let mcp = cli::codex::project_mcp_servers(Path::new(cwd), &users);
        if self.ctx.wall_of(seat) == WallMode::None {
            return (cli::codex::argv(seat, thread, job_io, &mcp, shown, style.as_deref()), None);
        }
        let (thread, images, model) = (thread.map(str::to_owned), shown.to_vec(), Some(seat.model.clone()));
        let live = mork_types::job::Live::Codex { thread, images, model, effort: Some(seat.effort) };
        (cli::codex::app_server_argv(seat, &mcp, style.as_deref()), Some(live))
    }

    // A planner's or foreman's output style, read at each launch (fac-596); one that can't be read is logged and left out.
    fn style(&self, a: &mork_types::state::Agent) -> Option<String> {
        let asks = matches!(a.role, mork_types::reply::Role::Planner | mork_types::reply::Role::Foreman);
        let home = std::env::var_os("HOME").filter(|h| asks && !h.is_empty())?;
        let style = crate::settings::read_style(Path::new(&home));
        style.map_err(|e| self.log(&format!("{}: launched without the operator's output style: {e}", a.key))).ok()
    }

    // Its app-server reads the user config `exec` was told to ignore, so the wall shows that config empty.
    fn blanked(
        &self,
        id: &JobId,
        live: Option<&mork_types::job::Live>,
        env: &BTreeMap<String, String>,
    ) -> Vec<(String, String)> {
        if !matches!(live, Some(mork_types::job::Live::Codex { .. })) {
            return Vec::new();
        }
        let empty = folder::JobDir::new(self.opts.home.job(id.as_str())).empty().display().to_string();
        let home = env.get("HOME").map(|h| format!("{h}/.codex"));
        let Some(codex) = env.get("CODEX_HOME").cloned().or(home) else { return Vec::new() };
        ["config.toml", "environments.toml"].map(|f| (empty.clone(), format!("{codex}/{f}"))).into()
    }

    /// Writes a recorded job's folder (when it isn't there), its images into its `$MORK_IO`, and starts its
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
        let io = dir.mork_io(&job.spec);
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
        self.start_supervisor(id);
    }

    /// Starts the supervisor for a job whose folder is ready.
    pub(super) fn start_supervisor(&mut self, id: &JobId) {
        let dir = self.opts.home.job(id.as_str());
        if let Err(e) = launch::start_supervisor(&self.opts.bin, &dir, self.opts.home.root()) {
            self.log(&format!("{id}: {e}"));
        }
        let cli = super::watch::cli_of(&self.state, id);
        let mut stream = self.stream_for(id);
        if let Some(job) = self.state.jobs.get(id) {
            // A resumed session's planner keeps its list: its open steps carry into the turn (fac-128).
            if job.spec.argv.iter().any(|a| a == "--resume")
                && let Some(list) = self.seen.get(&job.owner).and_then(|s| s.list.as_ref())
            {
                let open = list.steps.iter().filter(|s| s.status != mork_types::seen::StepStatus::Completed);
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
            Some(j) => (j.spec.cwd.clone(), j.spec.env.get("HOME").cloned().unwrap_or_default()),
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
