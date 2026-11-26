use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::mpsc;

use mork_core::decide::Decision;
use mork_core::messages::places::Who;
use mork_core::post::{Poster, new_agent};
use mork_core::state::Change;
use mork_types::job::{Claim, ExitRecord, JOB_FORMAT, JobKind, JobSpec, Live, RunInfo, WallMode};
use mork_types::reply::Role;
use mork_types::state::{AgentStatus, JobStatus, MessageKind, Owed, Sender, Settlement};
use serde_json::{Value, json};

use crate::cli::live::Delivery;
use crate::jobs::context::Context;
use crate::jobs::folder::{self, JobDir, NewFolder};
use crate::paths::Home;
use crate::runtime::{Engine, Options, Phase, pump, startup::Kept};
use crate::sockets::Shared;
use crate::store::Store;
use crate::{clock, files};

const JOB: &str = "20261014-163012-main-echo-0001";
const AGENT: &str = "main/echo";
const PROMPT: &str = "Read the saved request.";

struct Fixture {
    home: Home,
    dir: JobDir,
    who: Who,
}

impl Fixture {
    fn new() -> Self {
        let home = Home::new(std::env::temp_dir().join(format!("mork-replay-{}", crate::random::uuid())));
        fs::create_dir(home.root()).unwrap();
        let who = Who { key: AGENT.into(), seat: "echo".into(), role: Role::Echo, place: "main".into() };
        let spec = JobSpec {
            format: JOB_FORMAT,
            id: JOB.into(),
            kind: JobKind::Turn,
            owner: who.key.clone(),
            argv: vec!["codex".into(), "app-server".into()],
            cwd: home.root().display().to_string(),
            mounts: vec![],
            env: Default::default(),
            ports: None,
            net: None,
            wall: WallMode::Full,
            mork_bin: "mork".into(),
            bwrap: "/usr/bin/bwrap".into(),
            created_ms: 1,
            launch: Default::default(),
            live: Some(Live::Codex { thread: None, images: vec![], model: None, effort: None }),
        };
        let dir =
            folder::write(&home.jobs(), &NewFolder { spec: &spec, stdin: PROMPT.as_bytes(), io_files: &[] }).unwrap();
        let mut job = pump::job_of(spec, PROMPT.into(), Default::default());
        job.opening = true;
        let mut agent = new_agent(&who);
        agent.status = AgentStatus::Working { job: job.id.clone() };
        let mut seed = Decision::default();
        seed.journal("job.intent", None, json!({"job": JOB, "agent": AGENT}));
        seed.changes = vec![Change::PutAgent(agent), Change::PutJob(Box::new(job))];
        Store::open(&home.state_db(), "test").unwrap().commit(&seed.changes, &seed.journal, 1).unwrap();

        files::write_json(&dir.claim_path(), &Claim { pid: i32::MAX, start_time: 1, boot_id: "fixture".into() })
            .unwrap();
        // The synthetic wall has no live init, including when recovery checks a cut turn for leftovers.
        let run = RunInfo {
            bwrap_pid: i32::MAX,
            bwrap_start: 1,
            init_pid: i32::MAX,
            init_start: 1,
            pidns: "pid:[1]".into(),
            pgid: None,
        };
        files::write_json(&dir.run_path(), &run).unwrap();
        let fixture = Self { home, dir, who };
        fixture.output(json!({"id": 2, "result": {"thread": {"id": "saved-thread"}}}));
        fixture
    }

    fn open(&self) -> Engine {
        let opts = Options {
            home: self.home.clone(),
            config_path: self.home.root().join("mork.toml"),
            bin: self.home.root().join("mork"),
            commit: "test".into(),
            dirty: false,
            began: clock::instant(),
            reloaded: None,
            mode_args: vec![],
        };
        let config = crate::config::parse("[mork]\n[git]\nauthor = 'Test <test@example.invalid>'").unwrap();
        let ctx = Context::new(self.home.clone(), config, opts.bin.clone(), Default::default());
        let shared = Shared::new(opts.commit.clone(), false, self.home.root().display().to_string());
        let store = Store::open(&self.home.state_db(), &opts.commit).unwrap();
        let state = store.load().unwrap();
        // Recovery needs the store and job files. The app's listeners and event loop never start.
        Engine {
            reserved_ports: Default::default(),
            opts,
            ctx,
            store,
            state,
            shared,
            phase: Phase::Ready,
            live: Default::default(),
            watchers: vec![],
            tx: mpsc::channel().0,
            last_upkeep: None,
            keep: Kept { lock: File::create(self.home.app_lock()).unwrap(), mork: -1, agent: -1 },
            seen: Default::default(),
            limits_read: Default::default(),
            pipe: Default::default(),
            views_cache: Default::default(),
            door: Default::default(),
            hooks: Default::default(),
            wakes: Default::default(),
            closing_dirs: Default::default(),
            previews: Default::default(),
            mocks: Default::default(),
            suites: Default::default(),
        }
    }

    fn output(&self, notification: Value) {
        let mut out = OpenOptions::new().create(true).append(true).open(self.dir.out()).unwrap();
        writeln!(out, "{notification}").unwrap();
    }

    fn item(&self, item: Value) {
        self.output(json!({"method": "item/completed", "params": {"item": item, "threadId": "saved-thread"}}));
    }

    fn deliver(&self, engine: &mut Engine, text: &str) -> u64 {
        let mut posted = Poster::new(&engine.state, 2);
        let seq = posted.post("main", Sender::Operator, Some("echo"), MessageKind::Request, "", text);
        assert!(posted.deliver(&self.who, &Sender::Operator, seq));
        let mut decision = Decision::default();
        posted.finish(&mut decision);
        engine.commit_decided(decision, 2);
        seq
    }

    fn replay(&self) -> Engine {
        let mut engine = self.open();
        engine.recover_jobs();
        assert!(engine.pipe.recovered.is_empty(), "a claimed job queued another launch");
        assert_eq!(engine.state.jobs.len(), 1);
        engine
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.home.root());
    }
}

#[test]
fn running_turn_reattaches_with_its_saved_progress() {
    let fixture = Fixture::new();
    let lock = File::create(fixture.dir.lock()).unwrap();
    assert!(mork_sys::try_lock_exclusive(&lock).unwrap());
    fixture.item(json!({"type": "commandExecution", "id": "step-1", "command": "pwd"}));

    for _ in 0..2 {
        let mut engine = fixture.replay();
        engine.watch_jobs();
        let job = &engine.state.jobs[JOB];
        assert_eq!(job.status, JobStatus::Running);
        assert_eq!(job.settled, None);
        assert_eq!(engine.state.agents[AGENT].status, AgentStatus::Working { job: JOB.into() });
        assert_eq!(engine.seen[AGENT].steps, ["pwd"]);
        assert_eq!(engine.store.events_since("job.running", 0).unwrap().len(), 1);
    }
}

#[test]
fn completed_turn_applies_its_reply_and_input_receipts_once() {
    let fixture = Fixture::new();
    let mut before = fixture.open();
    let read = fixture.deliver(&mut before, "First message");
    let missed = fixture.deliver(&mut before, "Second message");
    drop(before);
    let input = |seq| files::read_json::<Delivery>(&fixture.dir.input().join(format!("{seq}.json"))).unwrap().unwrap();
    fixture.item(json!({"type": "userMessage", "id": "input-1", "clientId": input(read).uuid}));
    fixture.item(
        json!({"type": "agentMessage", "id": "reply", "text": json!({"message": "Recovered reply"}).to_string()}),
    );
    fixture.output(json!({"method": "turn/completed", "params": {"turn": {"status": "completed"}}}));
    files::write_json(
        &fixture.dir.exit_path(),
        &ExitRecord { code: Some(0), signal: None, ended_ms: 3, stopped: false, left: vec![], error: None },
    )
    .unwrap();
    assert!(input(missed).text.contains("Second message"));

    for _ in 0..2 {
        let engine = fixture.replay();
        let job = &engine.state.jobs[JOB];
        assert_eq!(job.settled, Some(Settlement::Applied));
        assert_eq!(job.read.as_deref(), Some([read].as_slice()));
        let agent = &engine.state.agents[AGENT];
        assert_eq!(agent.missed, [missed]);
        assert_eq!(agent.owed, Some(Owed::Wake));
        assert_eq!(
            engine
                .state
                .messages_in("main")
                .filter(|m| m.from == Sender::Agent(AGENT.into()) && m.text == "Recovered reply")
                .count(),
            1
        );
        assert_eq!(engine.store.events_since("job.settled", 0).unwrap().len(), 1);
    }
}

#[test]
fn cut_turn_keeps_one_retry_for_the_original_request() {
    let fixture = Fixture::new();
    // Even a complete JSON value is an unfinished line until the supervisor writes its newline.
    let mut out = OpenOptions::new().append(true).open(fixture.dir.out()).unwrap();
    write!(out, "{}", json!({"method": "turn/completed", "params": {"turn": {"status": "completed"}}})).unwrap();
    drop(out);

    for _ in 0..2 {
        let engine = fixture.replay();
        assert_eq!(engine.state.jobs[JOB].settled, Some(Settlement::Cut));
        let agent = &engine.state.agents[AGENT];
        assert_eq!(agent.cuts, 1);
        assert!(
            matches!(&agent.owed, Some(Owed::Retry { request: Some(id), prompt, resume: true, .. }) if id == JOB && prompt.ends_with(PROMPT))
        );
        assert_eq!(engine.store.events_since("job.settled", 0).unwrap().len(), 1);
    }
}
