//! `mork supervise <job folder>` (1.4.1, 1.4.3): owns one job outside its wall, recording its end alive or not.
//! Single-threaded for its whole life, so the wall's parent-death signal stays tied to it.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use mork_types::job::{Claim, ExitRecord, JobSpec, Launch, RunInfo, WallMode};

use super::folder::JobDir;
use super::{feed, proclog, procs, slice, wall};
use crate::{clock, faults, files};

/// The supervisor's entry: detach (fork, new session, fork), then own the job.
/// Returns the exit code of the process that called it.
pub fn main(dir: &Path) -> i32 {
    match mork_sys::fork() {
        Ok(mork_sys::ForkOutcome::Parent(_)) => return 0,
        Ok(mork_sys::ForkOutcome::Child) => {}
        Err(_) => return 1,
    }
    if mork_sys::setsid().is_err() {
        std::process::exit(1);
    }
    match mork_sys::fork() {
        Ok(mork_sys::ForkOutcome::Parent(_)) => std::process::exit(0),
        Ok(mork_sys::ForkOutcome::Child) => {}
        Err(_) => std::process::exit(1),
    }
    let job = JobDir::new(dir);
    if let (Ok(null), Ok(log)) =
        (File::open("/dev/null"), OpenOptions::new().create(true).append(true).open(job.sup_log()))
    {
        let _ = mork_sys::redirect_stdio(&null, &log);
    }
    match own(&job) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("supervisor: {e}");
            std::process::exit(1)
        }
    }
}

fn own(job: &JobDir) -> io::Result<()> {
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(job.lock())?;
    if !mork_sys::try_lock_exclusive(&lock)? {
        return Ok(()); // another supervisor owns the job
    }
    if !claim(job)? {
        return Ok(()); // claimed by a supervisor that died: recovery records it cut
    }
    mork_sys::install_term_flag()?;
    let spec = job.spec()?.ok_or_else(|| io::Error::other("no job.json"))?;
    let _leak = leak_for_test();
    let started = feed::Feed::new(job, &spec).and_then(|f| Ok((f, start(job, &spec)?)));
    let (mut feed, started) = match started {
        Ok(s) => s,
        Err(e) => return record_exit(job, None, false, Vec::new(), Some(e.to_string())),
    };
    let Started { mut child, run, _status } = started;
    if let Some(f) = feed.as_mut() {
        f.attach(child.stdin.take());
    }
    let mut stopped = false;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if let Some(f) = feed.as_mut() {
            f.step(job);
        }
        if mork_sys::term_requested() && !stopped {
            stopped = true;
            end(&spec, &run);
        }
        clock::sleep(Duration::from_millis(25));
    };
    let left = finish(&spec, &run);
    use std::os::unix::process::ExitStatusExt;
    let exit = ExitRecord {
        code: status.code(),
        signal: status.signal(),
        ended_ms: clock::now_ms(),
        stopped,
        left,
        error: None,
    };
    files::write_json(&job.exit_path(), &exit)
}

fn claim(job: &JobDir) -> io::Result<bool> {
    let f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(job.claim_path());
    let mut f = match f {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    };
    let pid = mork_sys::getpid();
    let c = Claim { pid, start_time: procs::start_time(pid).unwrap_or(0), boot_id: procs::boot_id() };
    f.write_all(&serde_json::to_vec(&c).map_err(io::Error::other)?)?;
    f.sync_all()?;
    Ok(true)
}

struct Started {
    child: Child,
    run: RunInfo,
    // bwrap's status pipe, kept open until it exits: it writes the exit code there last.
    _status: Option<BufReader<io::PipeReader>>,
}

fn start(job: &JobDir, spec: &JobSpec) -> io::Result<Started> {
    if spec.wall == WallMode::Partial && !faults::TEST_SETTINGS {
        // Fail closed (1.4.11.6): only a build with `faults` honours a partial wall. No wall is a
        // seat's own (fac-25), written by the engine as the rest of job.json is.
        return Err(io::Error::other("no wall: this build runs no job partly walled"));
    }
    let walled = spec.wall != WallMode::None;
    // A production build runs the system's bwrap, whatever job.json names (1.4.11.3).
    let mut spec = spec.clone();
    if !faults::TEST_SETTINGS {
        spec.bwrap = wall::BWRAP.into();
    }
    let spec = &spec;
    let argv = if walled { wall::argv(spec, false) } else { spec.argv.clone() };
    // bwrap clears its environment itself; unwalled, the bus `systemd-run` needs is left out of the job's.
    let bus: Vec<(&str, String)> = slice::BUS_VARS
        .into_iter()
        .filter(|v| !walled && spec.launch == Launch::Slice && !spec.env.contains_key(*v))
        .filter_map(|v| Some((v, std::env::var(v).ok()?)))
        .collect();
    let argv = slice::wrap(spec.launch, argv, &bus.iter().map(|b| b.0).collect::<Vec<_>>());
    let (program, args) = argv.split_first().ok_or_else(|| io::Error::other("empty argv"))?;
    let mut cmd = Command::new(program);
    // A live turn's prompt and messages are spoken to it as it runs (fac-389).
    let stdin = if spec.live.is_some() { Stdio::piped() } else { Stdio::from(File::open(job.stdin())?) };
    cmd.args(args).stdin(stdin).stdout(Stdio::from(append(&job.out())?)).stderr(Stdio::from(append(&job.err())?));
    if !walled {
        cmd.env_clear().envs(&spec.env).envs(bus).current_dir(&spec.cwd);
    }
    if let Some(home) = job.dir.parent().and_then(Path::parent) {
        proclog::record(home, spec.kind.as_str(), &argv, &spec.cwd, walled);
    }
    if walled { start_walled(job, cmd) } else { start_unwalled(job, cmd) }
}

fn start_walled(job: &JobDir, mut cmd: Command) -> io::Result<Started> {
    let (reader, writer) = io::pipe()?;
    use std::os::fd::AsRawFd;
    mork_sys::prepare_wall_spawn(&mut cmd, Some(writer.as_raw_fd()), false);
    let mut child = cmd.spawn()?;
    drop(writer);
    let mut status = BufReader::new(reader);
    let mut line = String::new();
    status.read_line(&mut line)?;
    let init = serde_json::from_str::<serde_json::Value>(&line)
        .ok()
        .and_then(|v| v.get("child-pid")?.as_i64())
        .and_then(|p| i32::try_from(p).ok());
    let Some(init) = init else {
        let _ = child.kill();
        let _ = child.wait();
        let err = std::fs::read_to_string(job.err()).unwrap_or_default();
        return Err(io::Error::other(format!("the wall didn't start: {}", err.trim())));
    };
    // The wall must have a pid namespace of its own: else the "job's processes" would be
    // everyone's in the supervisor's (M0.1). Kill what was started, and signal nothing else.
    let own = procs::pidns(mork_sys::getpid());
    match procs::pidns(init) {
        Some(ns) if Some(&ns) != own.as_ref() => {}
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("the wall didn't form: its init has no pid namespace of its own"));
        }
    }
    let bwrap = child.id().cast_signed();
    let run = RunInfo {
        bwrap_pid: bwrap,
        bwrap_start: procs::start_time(bwrap).unwrap_or(0),
        init_pid: init,
        init_start: procs::start_time(init).unwrap_or(0),
        pidns: procs::pidns(init).unwrap_or_default(),
        pgid: None,
    };
    files::write_json(&job.run_path(), &run)?;
    Ok(Started { child, run, _status: Some(status) })
}

fn start_unwalled(job: &JobDir, mut cmd: Command) -> io::Result<Started> {
    // A process the job detaches (`( cmd & )`, `setsid`) is adopted here, so it stays the job's to
    // know (`identity`) and to end (fac-25).
    mork_sys::set_child_subreaper()?;
    mork_sys::prepare_wall_spawn(&mut cmd, None, true);
    let child = cmd.spawn()?;
    let pid = child.id().cast_signed();
    let start = procs::start_time(pid).unwrap_or(0);
    let run = RunInfo {
        bwrap_pid: pid,
        bwrap_start: start,
        init_pid: pid,
        init_start: start,
        pidns: String::new(),
        pgid: Some(pid),
    };
    files::write_json(&job.run_path(), &run)?;
    Ok(Started { child, run, _status: None })
}

fn append(p: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(p)
}

fn end(spec: &JobSpec, run: &RunInfo) {
    if spec.wall == WallMode::None {
        let _ = procs::end_unwalled(run.pgid, spec.id.as_str(), Some(mork_sys::getpid()));
    } else {
        let _ = procs::end_walled(&run.pidns, run.init_pid);
    }
}

// Nothing of the job outlives it (1.4.4.3).
fn finish(spec: &JobSpec, run: &RunInfo) -> Vec<i32> {
    if spec.wall == WallMode::None {
        return procs::end_unwalled(run.pgid, spec.id.as_str(), Some(mork_sys::getpid()));
    }
    if run.pidns.is_empty() || procs::pidns(mork_sys::getpid()).as_deref() == Some(run.pidns.as_str()) {
        return Vec::new();
    }
    let alive =
        || -> Vec<i32> { procs::in_namespace(&run.pidns).into_iter().filter(|&p| !procs::is_zombie(p)).collect() };
    let left = procs::wait_until_gone(Duration::from_secs(2), alive);
    if left.is_empty() {
        return left;
    }
    procs::end_walled(&run.pidns, run.init_pid)
}

fn record_exit(
    job: &JobDir,
    code: Option<i32>,
    stopped: bool,
    left: Vec<i32>,
    error: Option<String>,
) -> io::Result<()> {
    let exit = ExitRecord { code, signal: None, ended_ms: clock::now_ms(), stopped, left, error };
    files::write_json(&job.exit_path(), &exit)
}

// T1.35's careless leak, honoured only with `faults`: a descriptor the supervisor holds open,
// made inheritable on purpose. Only the descriptor closing before bwrap keeps it out.
fn leak_for_test() -> Option<File> {
    let path = faults::test_env("MORK_TEST_LEAK_FD")?;
    let f = File::open(path).ok()?;
    use std::os::fd::AsRawFd;
    mork_sys::keep_across_exec(f.as_raw_fd()).ok()?;
    Some(f)
}
