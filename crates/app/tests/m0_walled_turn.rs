// M0: the production build never runs a seat's job in a partial wall, nor with any bwrap but the system's (6.2).
#![cfg(not(feature = "faults"))]
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mork_engine::jobs::{folder, wall};
use mork_fakes::TestHome;
use mork_types::job::{JOB_FORMAT, JobKind, JobSpec, WallMode};
use mork_types::{AgentKey, JobId};

fn mork_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mork"))
}

fn wait_for(path: &Path, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

// What the fake reports seeing: the state, its own job.json and the test's home must be
// hidden by the wall (the home is an empty tmpfs); the agent home must not.
fn checked_paths(t: &TestHome, id: &str) -> Vec<String> {
    let f = t.mork_home();
    vec![
        f.join("state.db").display().to_string(),
        f.join("jobs").join(id).join("job.json").display().to_string(),
        t.home().join("dummy-secret").display().to_string(),
        f.join("agent-home").display().to_string(),
    ]
}

fn echo_spec(t: &TestHome, id: &str) -> (JobSpec, PathBuf) {
    let fhome = t.mork_home();
    let agent_home = fhome.join("agent-home");
    let run_dir = fhome.join("run");
    std::fs::create_dir_all(&agent_home).unwrap();
    std::fs::create_dir_all(&run_dir).unwrap();
    let io = fhome.join("jobs").join(id).join("io");
    let bin_dir = mork_exe().parent().unwrap().display().to_string();
    let places = wall::WallPlaces {
        system: wall::system().unwrap(),
        sealed: Some(fhome.display().to_string()),
        hidden: [t.home(), t.tmp(), t.xdg()].into_iter().map(|p| p.display().to_string()).collect(),
        masked: Vec::new(),
        empty_file: String::new(),
        agent_home: agent_home.display().to_string(),
        caches: Vec::new(),
        browsers: None,
        run_dir: run_dir.display().to_string(),
        read_only: vec![bin_dir.clone(), t.bin().display().to_string()],
        peer_clones: Vec::new(),
        read_write: vec![io.display().to_string()],
        credentials: None,
        skills: None,
        empty: None,
        data: None,
        blanked: Vec::new(),
        sessions: None,
        sockets: None,
    };
    let log = agent_home.join("fake.log");
    let env = BTreeMap::from([
        ("HOME".to_owned(), agent_home.display().to_string()),
        ("PATH".to_owned(), format!("{bin_dir}:{}:/usr/local/bin:/usr/bin:/bin", t.bin().display())),
        ("MORK_JOB".to_owned(), id.to_owned()),
        ("MORK_AGENT".to_owned(), "main/echo".to_owned()),
        ("FAKE_LOG".to_owned(), log.display().to_string()),
        ("FAKE_CHECK_PATHS".to_owned(), checked_paths(t, id).join(":")),
    ]);
    let spec = JobSpec {
        format: JOB_FORMAT,
        id: JobId::new(id),
        kind: JobKind::Turn,
        owner: AgentKey::new("main/echo"),
        argv: vec!["claude".into(), "-p".into()],
        cwd: agent_home.display().to_string(),
        mounts: wall::mounts(&places),
        env,
        ports: None,
        net: None,
        wall: WallMode::Full,
        mork_bin: mork_exe().display().to_string(),
        bwrap: "/usr/bin/bwrap".into(),
        created_ms: 0,
        launch: Default::default(),
        live: None,
    };
    (spec, log)
}

// Production-only (1.10.8): a build without `faults` refuses a job that asks for a partial wall,
// and runs nothing (1.4.11.6).
#[test]
fn m0_production_build_refuses_a_partial_wall() {
    if common::wall_test("M0, fail closed") {
        return;
    }
    let t = TestHome::new();
    t.link_fakes(&mork_exe());
    let id = "20261014-163012-main-echo-0002";
    let (mut spec, log) = echo_spec(&t, id);
    spec.wall = WallMode::Partial;
    let fhome = t.mork_home();
    let dir =
        folder::write(&fhome.join("jobs"), &folder::NewFolder { spec: &spec, stdin: b"", io_files: &[] }).unwrap();
    let status = std::process::Command::new(mork_exe())
        .args(["supervise", &dir.dir.display().to_string()])
        .envs(t.env())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(wait_for(&dir.exit_path(), Duration::from_secs(20)));
    let exit = dir.exit().unwrap().unwrap();
    assert!(exit.error.as_deref().is_some_and(|e| e.contains("no wall")), "{exit:?}");
    assert!(mork_fakes::runlog::read(&log).is_empty());
}

// Production-only (fac-25): a seat with no wall is the engine's own word in job.json, so a build
// without `faults` runs its job unwalled, as its process group, and records it so.
#[test]
fn m0_production_build_runs_an_unwalled_seats_job() {
    let t = TestHome::new();
    t.link_fakes(&mork_exe());
    let id = "20261014-163012-main-echo-0004";
    let (mut spec, log) = echo_spec(&t, id);
    spec.wall = WallMode::None;
    let fhome = t.mork_home();
    let dir =
        folder::write(&fhome.join("jobs"), &folder::NewFolder { spec: &spec, stdin: b"hello", io_files: &[] }).unwrap();
    let status = std::process::Command::new(mork_exe())
        .args(["supervise", &dir.dir.display().to_string()])
        .envs(t.env())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(wait_for(&dir.exit_path(), Duration::from_secs(20)));
    let exit = dir.exit().unwrap().unwrap();
    assert_eq!((exit.code, exit.error.as_deref()), (Some(0), None), "{exit:?}");
    let runs = mork_fakes::runlog::read(&log);
    assert_eq!((runs.len(), runs[0]["job"].as_str()), (1, Some(id)));
    let run = dir.run().unwrap().unwrap();
    assert!(run.pidns.is_empty() && run.pgid.is_some(), "{run:?}");
    let procs = std::fs::read_to_string(fhome.join("logs/processes.log")).unwrap();
    assert!(procs.contains(r#""walled":false"#), "{procs}");
}

// Production-only (M0.1): a build without `faults` runs `/usr/bin/bwrap`, whatever
// `job.json` names.
#[test]
fn m0_production_build_runs_only_the_systems_bwrap() {
    if common::wall_test("M0, the system's bwrap") {
        return;
    }
    let t = TestHome::new();
    t.link_fakes(&mork_exe());
    let id = "20261014-163012-main-echo-0003";
    let (mut spec, log) = echo_spec(&t, id);
    spec.bwrap = t.root.join("not-bwrap").display().to_string();
    let fhome = t.mork_home();
    let dir =
        folder::write(&fhome.join("jobs"), &folder::NewFolder { spec: &spec, stdin: b"", io_files: &[] }).unwrap();
    let status = std::process::Command::new(mork_exe())
        .args(["supervise", &dir.dir.display().to_string()])
        .envs(t.env())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(wait_for(&dir.exit_path(), Duration::from_secs(20)));
    let exit = dir.exit().unwrap().unwrap();
    assert_eq!(exit.code, Some(0), "{exit:?}");
    assert_eq!(mork_fakes::runlog::read(&log).len(), 1);
}
