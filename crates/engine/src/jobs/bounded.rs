//! A command whose output is bounded while it is read (fac-1), never collected whole and trimmed
//! after; its input is written on its own thread, so a command that doesn't read never stalls.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::clock;

#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    pub max_out: usize,
    pub max_err: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// An output went over its cap: the command was ended there.
    pub truncated: bool,
    pub timed_out: bool,
    /// Its exit code; 128 and the signal's number for a signal; none if it never ended.
    pub exit: Option<i32>,
    pub elapsed_ms: u64,
}

/// What a run is: its argv, its environment (`None` keeps the caller's), its folder, its input.
#[derive(Debug, Clone, Default)]
pub struct Run<'a> {
    pub argv: &'a [String],
    pub env: Option<&'a BTreeMap<String, String>>,
    pub cwd: Option<&'a Path>,
    pub stdin: &'a [u8],
}

/// Called on the stdout read so far when the command is cut, before its group is ended.
pub type Cut<'a> = &'a dyn Fn(&[u8]);

/// Asked, on the stdout read so far, whether a held input's rest may be written.
pub type First<'a> = &'a dyn Fn(&[u8]) -> Hold;

/// What a run's caller watches while it runs (fac-183).
#[derive(Default)]
pub struct Watch<'a> {
    pub cut: Option<Cut<'a>>,
    /// Set by the caller, it cuts the command.
    pub stop: Option<&'a AtomicBool>,
    /// The input's first bytes, the rest written only once `First` answers `Release`.
    pub hold: Option<(usize, First<'a>)>,
}

/// What a held input waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Wait,
    Release,
    Cut,
}

// Past `cap` the read stops and `over` is set: the caller ends the command. What was read so far
// can be looked at while it runs.
struct Capped {
    done: std::sync::mpsc::Receiver<()>,
    buf: Arc<Mutex<Vec<u8>>>,
    over: Arc<AtomicBool>,
}

impl Capped {
    fn start(r: Option<impl Read + Send + 'static>, cap: usize) -> Self {
        let (tx, done) = std::sync::mpsc::channel();
        let over = Arc::new(AtomicBool::new(false));
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (flag, kept) = (Arc::clone(&over), Arc::clone(&buf));
        std::thread::spawn(move || {
            if let Some(mut r) = r {
                let mut chunk = [0u8; 16_384];
                while let Ok(n @ 1..) = r.read(&mut chunk) {
                    let mut b = kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let room = cap.saturating_sub(b.len());
                    b.extend_from_slice(&chunk[..n.min(room)]);
                    if n > room {
                        flag.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
            let _ = tx.send(());
        });
        Self { done, buf, over }
    }

    fn over(&self) -> bool {
        self.over.load(Ordering::SeqCst)
    }

    fn seen(&self) -> Vec<u8> {
        self.buf.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn finish(self, left: Duration) -> Vec<u8> {
        let _ = self.done.recv_timeout(left);
        std::mem::take(&mut *self.buf.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

/// Runs `run` within `bounds`; `kind` names it in `processes.log`, when `home_root` is given.
pub fn run(home_root: Option<&Path>, kind: &str, run: &Run<'_>, bounds: Bounds) -> io::Result<Captured> {
    run_watched(home_root, kind, run, bounds, &Watch::default())
}

/// As `run`, watched: cut at its deadline, a cap, its caller's stop or its held input's answer.
pub fn run_watched(
    home_root: Option<&Path>,
    kind: &str,
    run: &Run<'_>,
    bounds: Bounds,
    watch: &Watch<'_>,
) -> io::Result<Captured> {
    let (program, args) = run.argv.split_first().ok_or_else(|| io::Error::other("empty argv"))?;
    if let Some(root) = home_root {
        let cwd = run.cwd.map(|c| c.display().to_string()).unwrap_or_default();
        super::proclog::record(root, kind, run.argv, &cwd, program.ends_with("bwrap"));
    }
    let mut cmd = Command::new(program);
    super::launch::without_git_env(&mut cmd);
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A group of its own, ended with it; killed when its caller dies, so a crash leaves none.
    factory_sys::prepare_wall_spawn(&mut cmd, None, true);
    if let Some(env) = run.env {
        cmd.env_clear().envs(env);
    }
    if let Some(cwd) = run.cwd {
        cmd.current_dir(cwd);
    }
    let start = clock::instant();
    let mut child = cmd.spawn()?;
    let group = -i32::try_from(child.id()).unwrap_or(i32::MAX);
    let mut release = child.stdin.take().map(|input| feed(input, run.stdin, watch));
    let out = Capped::start(child.stdout.take(), bounds.max_out);
    let err = Capped::start(child.stderr.take(), bounds.max_err);
    let (mut truncated, mut timed_out) = (false, false);
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        truncated = out.over() || err.over();
        timed_out = clock::elapsed(start) > bounds.timeout;
        if truncated || timed_out || stopped(watch, &out, &mut release) {
            if let Some(cut) = watch.cut {
                cut(&out.seen());
            }
            let _ = factory_sys::kill(group, super::procs::libc_sig::KILL);
            let _ = child.kill();
            break child.wait().ok();
        }
        clock::sleep(Duration::from_millis(5));
    };
    // What it left in its group ends with it.
    let _ = factory_sys::kill(group, super::procs::libc_sig::KILL);
    let left = bounds.timeout.saturating_sub(clock::elapsed(start)).max(Duration::from_millis(200));
    let (out_over, err_over) = (Arc::clone(&out.over), Arc::clone(&err.over));
    let (stdout, stderr) = (out.finish(left), err.finish(Duration::from_millis(200)));
    // A reader that stopped at its cap may have ended the command itself (`SIGPIPE`).
    truncated = truncated || out_over.load(Ordering::SeqCst) || err_over.load(Ordering::SeqCst);
    let exit = status.and_then(|s| {
        use std::os::unix::process::ExitStatusExt;
        s.code().or_else(|| s.signal().map(|n| 128 + n))
    });
    let elapsed_ms = u64::try_from(clock::elapsed(start).as_millis()).unwrap_or(u64::MAX);
    Ok(Captured { stdout, stderr, truncated, timed_out, exit, elapsed_ms })
}

// Its input written on a thread of its own, held after its first bytes until it is released.
fn feed(mut input: std::process::ChildStdin, stdin: &[u8], watch: &Watch<'_>) -> std::sync::mpsc::Sender<()> {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let data = stdin.to_vec();
    let held = watch.hold.map_or(data.len(), |(n, _)| n.min(data.len()));
    std::thread::spawn(move || {
        if input.write_all(&data[..held]).and_then(|()| input.flush()).is_ok()
            && (held == data.len() || released.recv().is_ok())
        {
            let _ = input.write_all(&data[held..]);
        }
    });
    release
}

// Its caller's stop, or its held input's answer: release it, or cut the command.
fn stopped(watch: &Watch<'_>, out: &Capped, release: &mut Option<std::sync::mpsc::Sender<()>>) -> bool {
    if let Some((_, first)) = watch.hold
        && release.is_some()
    {
        match first(&out.seen()) {
            Hold::Wait => {}
            Hold::Release => {
                let _ = release.take().map(|r| r.send(()));
            }
            Hold::Cut => return true,
        }
    }
    watch.stop.is_some_and(|s| s.load(Ordering::SeqCst))
}

/// Runs `spec` with bounded input/output in its wall, or unwalled under the test setting.
pub fn walled(
    home_root: &Path,
    kind: &str,
    spec: &factory_types::job::JobSpec,
    stdin: &[u8],
    bounds: Bounds,
) -> io::Result<Captured> {
    use factory_types::job::WallMode;
    if spec.wall == WallMode::None {
        let cwd = Path::new(&spec.cwd);
        let r = Run { argv: &spec.argv, env: Some(&spec.env), cwd: Some(cwd), stdin };
        return run(Some(home_root), kind, &r, bounds);
    }
    let argv = super::wall::argv_short(spec);
    super::proclog::record(home_root, kind, &argv, &spec.cwd, true);
    run(None, kind, &Run { argv: &argv, env: None, cwd: None, stdin }, bounds)
}

/// The environment a door process starts with: the engine's `PATH` and `HOME` and nothing else of
/// its own (no ssh agent, no git settings), `C.UTF-8`; under test settings, the fakes' variables too.
pub fn door_env() -> BTreeMap<String, String> {
    let mut e = BTreeMap::new();
    for k in ["PATH", "HOME"] {
        if let Ok(v) = std::env::var(k) {
            e.insert(k.to_owned(), v);
        }
    }
    e.entry("PATH".to_owned()).or_insert_with(|| "/usr/local/bin:/usr/bin:/bin".into());
    e.insert("LANG".into(), "C.UTF-8".into());
    if crate::faults::TEST_SETTINGS {
        e.extend(std::env::vars().filter(|(k, _)| k.starts_with("FAKE_")));
    }
    e
}
