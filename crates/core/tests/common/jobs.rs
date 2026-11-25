use std::collections::BTreeMap;

use factory_core::decide::{Decision, Event, Intent, JobEnd, SessionSeen};
use factory_types::action::{Action, ActionStatus, ActionWhat};
use factory_types::job::{JobKind, JobSpec, Mount, WallMode};
use factory_types::state::{Job, JobStatus, Sender};
use factory_types::work::{ActionFailure, Purpose};
use factory_types::{ActionId, AgentKey, JobId};
use serde_json::{Value, json};

use super::{HOME, Sim, sha};

impl Sim {
    fn spec(&mut self, kind: JobKind, owner: &AgentKey, format: u32) -> JobSpec {
        self.next += 1;
        JobSpec {
            format,
            id: JobId::new(format!("job-{:04}", self.next)),
            kind,
            owner: owner.clone(),
            argv: vec!["x".into()],
            cwd: "/".into(),
            mounts: Vec::new(),
            env: BTreeMap::new(),
            ports: None,
            wall: WallMode::Full,
            factory_bin: "/f".into(),
            bwrap: "/b".into(),
            created_ms: self.now,
            launch: Default::default(),
            live: None,
        }
    }

    fn job(spec: JobSpec, stdin: String) -> Job {
        Job {
            id: spec.id.clone(),
            kind: spec.kind,
            owner: spec.owner.clone(),
            spec,
            stdin,
            io_files: BTreeMap::new(),
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

    /// Records the jobs `core` asked for, as the engine would, each under its `<card>/<what>` owner.
    pub fn record_jobs(&mut self) {
        let starts: Vec<Purpose> = self
            .intents
            .iter()
            .filter_map(|i| match i {
                Intent::StartJob { purpose } => Some(purpose.clone()),
                _ => None,
            })
            .collect();
        self.intents.retain(|i| !matches!(i, Intent::StartJob { .. }));
        for p in starts {
            let kind = match p {
                Purpose::Install { .. } => JobKind::Install,
                Purpose::Clone { .. } | Purpose::Bundle { .. } | Purpose::Candidate { .. } => JobKind::Bundle,
                _ => JobKind::Check,
            };
            let owner = AgentKey::new(format!("{}/work", p.card().map_or("factory", |c| c.as_str())));
            let mut job = Self::job(self.spec(kind, &owner, 1), String::new());
            if matches!(p, Purpose::Check { .. }) {
                job.checks_at = Some("c1".into());
            }
            job.purpose = Some(p);
            self.apply(Event::JobRecorded { job: Box::new(job) });
        }
    }

    /// The unsettled recorded jobs whose purpose `pick` accepts, the earliest first.
    pub fn open_jobs(&mut self, pick: impl Fn(&Purpose) -> bool) -> Vec<JobId> {
        self.record_jobs();
        let open = |j: &&Job| j.settled.is_none() && j.purpose.as_ref().is_some_and(&pick);
        self.state.jobs.values().filter(open).map(|j| j.id.clone()).collect()
    }

    /// A recorded job's purpose.
    pub fn purpose(&self, id: &JobId) -> Purpose {
        self.state.jobs.get(id).and_then(|j| j.purpose.clone()).unwrap_or_else(|| panic!("job {id:?} has no purpose"))
    }

    /// Ends a job with its result.
    pub fn end_job(&mut self, id: &JobId, result: Value) -> Decision {
        self.apply(Event::JobRunning { id: id.clone() });
        let end = JobEnd::Finished { reply: Ok(result), session: SessionSeen::default() };
        self.apply(Event::JobEnded { id: id.clone(), end })
    }

    /// Ends the one pending job whose purpose `pick` accepts, which must exist.
    pub fn answer_job(&mut self, what: &str, pick: impl Fn(&Purpose) -> bool, result: Value) -> Decision {
        let ids = self.open_jobs(pick);
        let id = ids.first().unwrap_or_else(|| panic!("no pending {what} job; jobs {:?}", self.pending_jobs()));
        let id = id.clone();
        self.end_job(&id, result)
    }

    /// The purposes of every recorded job not yet ended, for a failure's words.
    pub fn pending_jobs(&mut self) -> Vec<Purpose> {
        let ids = self.open_jobs(|_| true);
        ids.iter().map(|id| self.purpose(id)).collect()
    }

    /// The actions `core` asked to run, not yet returned.
    pub fn pending(&self) -> Vec<Action> {
        let asked = |a: &&Action| {
            self.intents.iter().any(|i| matches!(i, Intent::RunAction { id } if *id == a.id))
                && matches!(a.status, ActionStatus::Pending | ActionStatus::Started)
        };
        self.state.actions.values().filter(asked).cloned().collect()
    }

    /// Returns an action's result, as the engine would after the call: marked started first (2.5.1).
    pub fn returned(&mut self, id: &ActionId, result: Result<Value, ActionFailure>) -> Decision {
        self.intents.retain(|i| !matches!(i, Intent::RunAction { id: x } if x == id));
        self.apply(Event::ActionStarted { id: id.clone(), prepared: None });
        self.apply(Event::ActionReturned { id: id.clone(), result })
    }

    /// The pending action of a kind, which must exist.
    pub fn action_of(&self, pick: impl Fn(&ActionWhat) -> bool) -> Action {
        let pending = self.pending();
        let found = pending.into_iter().find(|a| pick(&a.what));
        found.unwrap_or_else(|| panic!("no such pending action; actions {:?}", self.actions()))
    }

    /// Answers every pending action that needs no data with its success: board writes, clone removals, pushes,
    /// fetches at the base's tip; a PR opens as number 7.
    pub fn settle_actions(&mut self) {
        loop {
            let easy: Vec<Action> = self.pending().into_iter().filter(|a| easy(&a.what)).collect();
            if easy.is_empty() {
                return;
            }
            let holds = self.holding();
            for a in easy {
                let result = match &a.what {
                    ActionWhat::Fetch { .. } => json!({"base_tip": sha(self.tip), "contains": holds.clone()}),
                    ActionWhat::OpenPr { .. } => json!({"pr": 7}),
                    _ => json!({}),
                };
                self.returned(&a.id, Ok(result));
            }
        }
    }

    /// Plays an agent's due turn: prepared as its plan says, running, ended with `reply`.
    pub fn turn(&mut self, key: &str, reply: Value) -> Decision {
        self.turn_sending(key, &[], reply)
    }

    /// Plays an agent's due turn that sends `commands` while it runs, then ends with `reply`.
    pub fn turn_sending(&mut self, key: &str, commands: &[Value], reply: Value) -> Decision {
        let k = AgentKey::new(key);
        let id = self.turn_started(key);
        for c in commands {
            let answer = self.cmd(Sender::Agent(k.clone()), c.clone());
            assert!(answer.is_ok(), "{key}'s {c} was refused: {answer:?}");
        }
        let session = SessionSeen { started: true, ..SessionSeen::default() };
        self.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Ok(reply), session } })
    }
}

impl Sim {
    /// Starts an agent's due turn, prepared as its plan says, in its seat's `io/` folder as the engine binds it
    /// (fac-654): the one its first turn named.
    pub fn turn_started(&mut self, key: &str) -> JobId {
        self.turn_started_with_argv(key, &["x"]).0
    }

    /// Starts an agent's due turn with `argv`, returning its job id and planned prompt.
    pub fn turn_started_with_argv(&mut self, key: &str, argv: &[&str]) -> (JobId, String) {
        let k = AgentKey::new(key);
        let due = factory_core::agents::turns::due(&self.state, &self.config, self.now);
        assert!(self.owed(key), "{key} isn't owed a turn; due: {due:?}");
        let plan = factory_core::agents::turns::plan(&self.state, &k, self.ctx(), false).unwrap();
        let format = factory_types::job::JOB_FORMAT;
        let mut spec = self.spec(JobKind::Turn, &k, format);
        spec.argv = argv.iter().map(|s| (*s).to_owned()).collect();
        let mut job = Self::job(spec, plan.prompt.clone());
        job.corrects = plan.corrects.clone();
        job.opening = plan.corrects.is_none() && !plan.resume;
        job.staged = plan.staged.clone();
        let agent = &self.state.agents[&k];
        let schema = factory_core::agents::reply::schema_of(&self.state, agent, factory_types::config::Cli::Codex);
        job.io_files.insert("schema.json".into(), schema.to_string());
        let named = self.state.agents.get(&k).and_then(|a| a.io.clone());
        let io = format!("{HOME}/io/{}", named.unwrap_or_else(|| job.id.as_str().to_owned()));
        job.spec.env.insert("FACTORY_IO".into(), io.clone());
        job.spec.mounts.push(Mount::Rw { path: io });
        let id = job.id.clone();
        let session = (argv.first() != Some(&"agy")).then(|| format!("session-{key}"));
        self.apply(Event::TurnPrepared { job: Box::new(job), session });
        self.apply(Event::JobRunning { id: id.clone() });
        (id, plan.prompt)
    }

    // The cards whose head holds the base's tip, as a fetch reads them: those whose take read it.
    fn holding(&self) -> serde_json::Map<String, Value> {
        let tip = sha(self.tip);
        let holds = |c: &&factory_types::card::Card| c.fork.as_ref() == Some(&tip);
        self.state.cards.values().filter(holds).map(|c| (c.id.to_string(), json!(true))).collect()
    }
}

fn easy(what: &ActionWhat) -> bool {
    matches!(
        what,
        ActionWhat::Fetch { .. }
            | ActionWhat::ChangeBoardCard { .. }
            | ActionWhat::CreateBoardCard { .. }
            | ActionWhat::RemoveClone { .. }
            | ActionWhat::RemoveTrain { .. }
            | ActionWhat::Push { .. }
            | ActionWhat::OpenPr { .. }
    )
}
