// The jobs of no agent's turn (section 4): a clone, `install`, a bundle, the checks gate, a joined tree; the merge
// queue's are `queue_work`'s (fac-10). Each is a walled job like a turn (1.4.3), read from its `io/` when it ends.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_types::card::Card;
use mork_types::config::ProjectConfig as Project;
use mork_types::job::{JobKind, WallMode};
use mork_types::state::Job;
use mork_types::work::Purpose;
use mork_types::{AgentKey, CardId, JobId};
use serde_json::{Value, json};

use super::Engine;
use crate::jobs::context::Context;
use crate::jobs::work::Order;
use crate::{clock, random};

/// What a work job is made of, before its spec.
pub(super) struct Plan {
    pub owner: AgentKey,
    pub kind: JobKind,
    pub argv: Vec<String>,
    pub cwd: String,
    pub read_only: Vec<String>,
    pub read_write: Vec<String>,
    pub files: BTreeMap<String, String>,
    pub copies: BTreeMap<String, String>,
    pub project: Option<String>,
    /// Run outside every wall: Mork's own wall tests, which no wall can start inside (fac-65).
    pub unwalled: bool,
}

impl Engine {
    pub(super) fn card(&self, id: &CardId) -> Result<Card, String> {
        self.state.cards.get(id).cloned().ok_or_else(|| format!("no card {id}"))
    }

    pub(super) fn project(&self, name: &str) -> Result<Project, String> {
        self.ctx.config.projects.get(name).cloned().ok_or_else(|| format!("no project {name}"))
    }

    pub(super) fn clone_dir(&self, name: &str) -> PathBuf {
        self.opts.home.clones().join(name)
    }

    // A check run with the checks folder dirty: its end, at once, untrusted (4.7.2).
    fn dirty_checks(&self, purpose: &Purpose) -> Option<JobEnd> {
        let (card, head) = match purpose {
            Purpose::Check { card, head, .. } => (card, head),
            Purpose::TrainRun { card, commit, checks: true, .. } => return self.dirty_final(card, commit),
            Purpose::Clone { .. }
            | Purpose::Install { .. }
            | Purpose::Bundle { .. }
            | Purpose::JoinTest { .. }
            | Purpose::Incorporate { .. }
            | Purpose::Final { .. }
            | Purpose::Candidate { .. }
            | Purpose::TrainRun { .. }
            | Purpose::WallTests { .. }
            | Purpose::Suite { .. }
            | Purpose::Build { .. } => return None,
        };
        let (folder_commit, dirty) = self.checks_folder(card);
        if !dirty {
            return None;
        }
        let checks = mork_types::work::CheckRun {
            head: head.clone(),
            exit: None,
            report: None,
            folder_commit,
            folder_dirty: true,
            located: Vec::new(),
            folder_moved: false,
        };
        Some(finished(serde_json::to_value(checks).unwrap_or_default()))
    }

    /// `StartJob`: the job recorded with its purpose, then launched (1.4.3).
    pub(super) fn start_work(&mut self, purpose: &mork_types::work::Purpose) {
        // Work is owed until its job is recorded (M3.9): a second intent for work already
        // recorded (owed at recovery, then issued again by `Started`) starts nothing (T6.3).
        if !self.state.owed_work.contains_key(&purpose.key()) {
            return;
        }
        if self.wait_for_work(purpose) {
            return;
        }
        let now = clock::now_ms();
        if let Purpose::Build { commit } = purpose {
            if let Err(e) = self.start_build(commit.as_str(), Some(purpose)) {
                self.work_refused(purpose, &e.message, now);
            }
            return;
        }
        // A checks folder with uncommitted changes is untrusted and never run (4.7.2, R2, M3.8).
        if let Some(end) = self.dirty_checks(purpose) {
            return self.work_ends_at_once(purpose, end, now);
        }
        if super::work_threads::prepares(purpose) {
            return self.prepare_work(purpose);
        }
        let plan = match self.plan_work(purpose) {
            Ok(p) => p,
            Err(e) => return self.work_refused(purpose, &e, now),
        };
        self.launch_work(purpose, plan, now);
    }

    pub(super) fn launch_work(&mut self, purpose: &Purpose, plan: Plan, now: i64) -> bool {
        let id = mork_core::state::names::job_id(now, &plan.owner, random::salt());
        let io = self.opts.home.job(id.as_str()).join("io");
        let mut job = match job_of_plan(&self.ctx, plan, &id, &io, now) {
            Ok(job) => job,
            Err(e) => {
                self.work_refused(purpose, &e, now);
                return false;
            }
        };
        job.purpose = Some(purpose.clone());
        // The checks folder's commit as the run starts: the run counts for it alone (4.7.7, M3.12).
        if let Purpose::Check { card, .. } | Purpose::TrainRun { card, checks: true, .. } = purpose {
            job.checks_at = Some(self.checks_folder(card).0);
        }
        self.apply(&Event::JobRecorded { job: Box::new(job) });
        if self.state.jobs.contains_key(&id) {
            self.launch(&id);
            return true;
        }
        false
    }

    pub(super) fn work_refused(&mut self, purpose: &Purpose, why: &str, now: i64) {
        self.work_ends_at_once(purpose, JobEnd::Failed { words: why.to_owned() }, now);
    }

    pub(super) fn work_ends_at_once(&mut self, purpose: &Purpose, end: JobEnd, now: i64) {
        let owner = owner_of(purpose);
        let id = mork_core::state::names::job_id(now, &owner, random::salt());
        let io = self.opts.home.job(id.as_str()).join("io");
        let places = self.ctx.unwalled_places(&io, Vec::new(), Vec::new(), None, &[]);
        let spec = self.ctx.spec(
            &id,
            JobKind::Bundle,
            &owner,
            Vec::new(),
            "/",
            &places,
            BTreeMap::new(),
            Some(Vec::new()),
            now,
        );
        let mut job = super::pump::job_of(spec, String::new(), BTreeMap::new());
        job.purpose = Some(purpose.clone());
        self.apply(&Event::JobRecorded { job: Box::new(job) });
        self.apply(&Event::JobEnded { id, end });
    }

    pub(super) fn plan_work(&self, purpose: &Purpose) -> Result<Plan, String> {
        match purpose {
            Purpose::WallTests { .. } => self.plan_wall_run(purpose),
            Purpose::Clone { .. } | Purpose::Candidate { .. } | Purpose::Bundle { .. } => {
                self.worktree(purpose)?.prepare(purpose, &mut Vec::new())
            }
            Purpose::Install { card, .. } => {
                let c = self.card(card)?;
                let install = self.project(&c.project)?.install.ok_or("the project has no install")?;
                let dir = self.clone_dir(card.as_str()).display().to_string();
                Ok(plan(purpose, JobKind::Install, vec!["sh".into(), "-c".into(), install], &dir, &c.project).rw(dir))
            }
            Purpose::Check { card, fork, .. } => self.plan_check(card, fork.as_str()),
            Purpose::Suite { .. } | Purpose::JoinTest { .. } | Purpose::Incorporate { .. } | Purpose::Final { .. } => {
                Err("an older build's run: the merge train runs none now (fac-68)".into())
            }
            Purpose::TrainRun { .. } => self.plan_train_run(purpose),
            Purpose::Build { .. } => Err("a build is made by its own path (1.8)".into()),
        }
    }

    pub(super) fn work_argv(&self) -> Vec<String> {
        vec![self.ctx.bin.display().to_string(), "work".into(), "{io}/work.json".into()]
    }

    fn plan_check(&self, card: &CardId, fork: &str) -> Result<Plan, String> {
        let c = self.card(card)?;
        let p = self.project(&c.project)?;
        let checks = p.checks.clone().ok_or("the project has no checks")?;
        let dir = self.clone_dir(card.as_str()).display().to_string();
        let argv = vec![
            "sh".into(),
            "-c".into(),
            format!("{} \"$@\"", checks.command),
            "sh".into(),
            "--base".into(),
            fork.to_owned(),
            "--json".into(),
            "{io}/checks.json".into(),
        ];
        let purpose = Purpose::Check {
            card: card.clone(),
            head: c.head.clone().unwrap_or_else(|| mork_types::Sha::new(fork)),
            fork: mork_types::Sha::new(fork),
        };
        // The clone read-write (checks write caches and build output in it), the checks' checkout
        // read-only (1.4.11.1, M3.28, fac-165).
        let mount = self.checks_mount(&checks.folder);
        Ok(plan(&purpose, JobKind::Check, argv, &dir, &c.project).rw(dir).ro(mount))
    }
}

/// The owner a work job is recorded under: `<card>/<what>`, never an agent.
pub(super) fn owner_of(purpose: &Purpose) -> AgentKey {
    let (who, what) = match purpose {
        Purpose::Clone { card, .. } => (card.as_str().to_owned(), "clone"),
        Purpose::Install { card, .. } => (card.as_str().to_owned(), "install"),
        Purpose::Bundle { card, .. } => (card.as_str().to_owned(), "bundle"),
        Purpose::Check { card, .. } => (card.as_str().to_owned(), "check"),
        Purpose::JoinTest { card, .. } => (card.as_str().to_owned(), "join"),
        Purpose::Incorporate { card, .. } => (card.as_str().to_owned(), "incorporate"),
        Purpose::Final { card, .. } => (card.as_str().to_owned(), "final"),
        Purpose::Candidate { card, .. } => (card.as_str().to_owned(), "candidate"),
        Purpose::TrainRun { card, .. } => (card.as_str().to_owned(), "train"),
        Purpose::Suite { card, .. } => (card.as_str().to_owned(), "suite"),
        Purpose::Build { .. } => ("mork".to_owned(), "build"),
        Purpose::WallTests { card, .. } => (card.as_str().to_owned(), "walls"),
    };
    AgentKey::new(format!("{who}/{what}"))
}

// Shared job construction keeps I/O substitution and wall selection consistent across plans.
// The host phase needs an isolated temporary folder because its /tmp is shared (fac-65).
pub(super) fn job_of_plan(ctx: &Context, plan: Plan, id: &JobId, io: &Path, now: i64) -> Result<Job, String> {
    let io_s = io.display().to_string();
    let argv: Vec<String> = plan.argv.iter().map(|a| a.replace("{io}", &io_s)).collect();
    let files: BTreeMap<String, String> = plan.files.into_iter().map(|(k, v)| (k, v.replace("{io}", &io_s))).collect();
    let cwd = plan.cwd.replace("{io}", &io_s);
    let toolchains = ctx.project_toolchains(plan.project.as_deref());
    let mut env = ctx.env(id, &plan.owner, None, &toolchains, io);
    if plan.unwalled {
        env.insert("TMPDIR".into(), io.join("tmp").display().to_string());
    }
    let places = if plan.unwalled {
        ctx.unwalled_places(io, plan.read_only, plan.read_write, None, &toolchains)
    } else {
        ctx.places(io, plan.read_only, plan.read_write, None, &toolchains)?
    };
    let ports = ctx.work_ports(plan.kind, plan.project.as_deref());
    let mut spec = ctx.spec(id, plan.kind, &plan.owner, argv, &cwd, &places, env, ports, now);
    if plan.unwalled {
        spec.wall = WallMode::None;
    }
    let mut job = super::pump::job_of(spec, String::new(), files);
    job.io_copies = plan.copies;
    Ok(job)
}

pub(super) fn plan(purpose: &Purpose, kind: JobKind, argv: Vec<String>, cwd: &str, project: &str) -> Plan {
    Plan {
        owner: owner_of(purpose),
        kind,
        argv,
        cwd: cwd.to_owned(),
        read_only: Vec::new(),
        read_write: Vec::new(),
        files: BTreeMap::new(),
        copies: BTreeMap::new(),
        project: Some(project.to_owned()),
        unwalled: false,
    }
}

impl Plan {
    pub(super) fn rw(mut self, p: String) -> Self {
        self.read_write.push(p);
        self
    }
    pub(super) fn ro(mut self, p: String) -> Self {
        self.read_only.push(p);
        self
    }
}

pub(super) fn json_of(o: &Order) -> Result<String, String> {
    serde_json::to_string(o).map_err(|e| e.to_string())
}

/// A clone's folder, made empty: one left over from a crash is removed first.
pub(super) fn fresh_dir(dir: &std::path::Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// A work job's end, as `core` takes it.
pub(super) fn finished(v: Value) -> JobEnd {
    JobEnd::Finished { reply: Ok(v), session: SessionSeen::default() }
}

pub(super) fn failed(words: String) -> JobEnd {
    JobEnd::Failed { words }
}

pub(super) fn done() -> JobEnd {
    finished(json!({}))
}
