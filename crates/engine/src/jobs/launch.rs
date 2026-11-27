//! Starting processes: a job's supervisor (1.4.3), short commands (1.2.3), an `exec` of
//! another build (reload, the launcher, `ctl`), and the interactive login wall (1.7.2).

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;

use mork_types::job::JobSpec;

use super::{proclog, wall};
use crate::clock;
use crate::lock::Lock;

/// Starts `mork supervise <dir>`: the supervisor detaches at once (fork, new session,
/// fork), so the wait is short.
pub fn start_supervisor(bin: &Path, dir: &Path, home_root: &Path) -> io::Result<()> {
    let mut cmd = Command::new(bin);
    without_git_env(&mut cmd);
    cmd.arg("supervise").arg(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let argv = vec![bin.display().to_string(), "supervise".into(), dir.display().to_string()];
    proclog::record(home_root, "supervisor", &argv, &dir.display().to_string(), false);
    let status = cmd.status()?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("the supervisor didn't start: {status}"))) }
}

/// A long-lived child its caller owns and ends: `mork live-check`'s scratch mork (1.11).
/// Its output goes to `log`.
pub fn child(home_root: &Path, argv: &[String], env: &[(String, String)], log: &Path) -> io::Result<Child> {
    let (program, args) = argv.split_first().ok_or_else(|| io::Error::other("empty argv"))?;
    proclog::record(home_root, "app", argv, "", false);
    let out = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    let err = out.try_clone()?;
    let mut cmd = Command::new(program);
    without_git_env(&mut cmd);
    cmd.args(args).envs(env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null()).stdout(out).stderr(err).spawn()
}

/// A short command (1.2.3): a plain child of the app, with a timeout. It never outlives the
/// call: on the timeout it is killed.
pub fn short(home_root: &Path, argv: &[&str], cwd: &Path, timeout: Duration) -> io::Result<Output> {
    short_env(home_root, argv, cwd, timeout, &[])
}

/// As [`short`], with variables added to its environment: a secret goes here, never in the
/// argv `processes.log` records.
pub fn short_env(
    home_root: &Path,
    argv: &[&str],
    cwd: &Path,
    timeout: Duration,
    env: &[(String, String)],
) -> io::Result<Output> {
    let (program, _) = argv.split_first().ok_or_else(|| io::Error::other("empty argv"))?;
    let owned: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
    proclog::record(home_root, "short", &owned, &cwd.display().to_string(), false);
    let mut child = guarded(argv, cwd, env).spawn()?;
    // Never 0: a signal to group 0 would reach the app's own group.
    let group = -i32::try_from(child.id()).unwrap_or(i32::MAX);
    // Read on threads, so a long output never fills its pipe and stalls the child.
    let out = Drain::start(child.stdout.take().map(|o| Box::new(o) as Box<dyn io::Read + Send>));
    let err = Drain::start(child.stderr.take().map(|e| Box::new(e) as Box<dyn io::Read + Send>));
    let start = clock::instant();
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if clock::elapsed(start) > timeout {
            // The guard ends what escaped its group first (M4.6), then the group goes.
            let _ = mork_sys::kill(-group, super::procs::libc_sig::TERM);
            let ends = clock::instant();
            while child.try_wait()?.is_none() && clock::elapsed(ends) < Duration::from_secs(1) {
                clock::sleep(Duration::from_millis(5));
            }
            let _ = mork_sys::kill(group, super::procs::libc_sig::KILL);
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, format!("{program} timed out")));
        }
        clock::sleep(Duration::from_millis(5));
    };
    // What it left running in its group ends with it.
    let _ = mork_sys::kill(group, super::procs::libc_sig::KILL);
    // A descendant in a session of its own may hold a pipe: the read ends at the timeout (M3.34).
    let left = timeout.saturating_sub(clock::elapsed(start));
    Ok(Output { status, stdout: out.finish(left), stderr: err.finish(left) })
}

// A short command under a guard leading its group (M3.34): when the app dies the guard is
// sent SIGTERM and kills the whole group, what the command started included (1.2.3).
fn guarded(argv: &[&str], cwd: &Path, env: &[(String, String)]) -> Command {
    let mut cmd = guard(argv);
    without_git_env(&mut cmd);
    cmd.envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

// A caller's git variables (`git rev-parse --local-env-vars`, every `GIT_CONFIG*`; a git hook's shell, say) never
// point Mork's git, or a Mork process it starts, at another repository (M5.12). What a call adds itself comes after.
const GIT_LOCAL: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// Leaves git's repository and config variables out of a child's environment (M5.12).
pub fn without_git_env(cmd: &mut Command) {
    for (k, _) in std::env::vars_os() {
        let name = k.to_string_lossy();
        if name.starts_with("GIT_CONFIG") || GIT_LOCAL.contains(&name.as_ref()) {
            cmd.env_remove(&k);
        }
    }
}

/// `argv` under a guard leading its own group, which kills the group when the process that starts it dies
/// (M3.34, M3.25); the command reads the guard's stdin, and the guard exits as it does, 128 plus a signal's number.
pub fn guard(argv: &[impl AsRef<std::ffi::OsStr>]) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", GUARD, "mork-guard"]).args(argv);
    mork_sys::prepare_guard_spawn(&mut cmd);
    cmd
}

// M3.34: at the command's end or on SIGTERM, kills each live (non-zombie) child, pass after pass, till none is left; a
// killed child's `setsid` descendants are adopted (M4.6) however deep (M5.22). 200 passes 20 ms apart, then its group.
const GUARD: &str = "k() { n=0; while [ $n -lt 200 ]; do c=; { read -r c < /proc/$$/task/$$/children; } 2>/dev/null || :; \
    live=; for p in $c; do st=; { read -r st < /proc/$p/stat; } 2>/dev/null || continue; \
    case \"$st\" in *') Z '*) ;; *) live=\"$live $p\";; esac; done; \
    [ -z \"$live\" ] && return; kill -KILL $live 2>/dev/null; sleep 0.02; n=$((n+1)); done; }; \
    trap 'k; kill -KILL 0' TERM; exec 3<&0; \"$@\" <&3 3<&- & wait $!; s=$?; k; exit $s";

struct Drain {
    buf: Arc<Lock<Vec<u8>>>,
    done: std::sync::mpsc::Receiver<()>,
}

impl Drain {
    fn start(r: Option<Box<dyn io::Read + Send>>) -> Self {
        use std::io::Read;
        let buf = Arc::new(Lock::new(Vec::new()));
        let (tx, done) = std::sync::mpsc::channel();
        let shared = Arc::clone(&buf);
        std::thread::spawn(move || {
            if let Some(mut r) = r {
                let mut chunk = [0u8; 65_536];
                while let Ok(n @ 1..) = r.read(&mut chunk) {
                    shared.lock().extend_from_slice(&chunk[..n]);
                }
            }
            let _ = tx.send(());
        });
        Self { buf, done }
    }

    fn finish(self, left: Duration) -> Vec<u8> {
        let _ = self.done.recv_timeout(left);
        std::mem::take(&mut *self.buf.lock())
    }
}

/// The system's browser on a web page: `xdg-open`, reaped on its own thread.
pub fn browser(home_root: &Path, url: &str) -> io::Result<()> {
    let argv = ["xdg-open".to_owned(), url.to_owned()];
    proclog::record(home_root, "browser", &argv, "", false);
    let mut child = Command::new(&argv[0]).arg(url).stdin(Stdio::null()).spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Replaces this process with `bin` and `args`; returns only on failure.
pub fn exec(bin: &Path, args: &[String], env: &[(String, String)]) -> io::Error {
    Command::new(bin).args(args).envs(env.iter().map(|(a, b)| (a, b))).exec()
}

/// Runs the login wall: bwrap with the terminal attached and without `--new-session`, so the
/// CLI's URL can be followed (1.7.2). Returns the CLI's exit code.
pub fn interactive_wall(spec: &JobSpec, home_root: &Path) -> io::Result<i32> {
    let argv = wall::argv(spec, true);
    proclog::record(home_root, "login", &argv, &spec.cwd, true);
    let (program, args) = argv.split_first().ok_or_else(|| io::Error::other("empty argv"))?;
    let status = Command::new(program).args(args).status()?;
    Ok(status.code().unwrap_or(1))
}

/// How a limits read answers: Codex's server answers id 2 while `lines` hold its stdin open; agy's command prints one
/// JSON object and exits, read whole only at a successful exit.
#[derive(Debug, Clone, Copy)]
pub enum Answer<'a> {
    Rpc(&'a [&'a str]),
    AtExit,
}

/// A limits read (1.4.8, Codex 7; fac-657): `spec`'s argv in a wall; returns what it printed up to its answer, or why
/// not. Cut at `cut`; `group` gets its process group so a shutdown can end it.
pub fn walled_read(
    spec: &JobSpec,
    home_root: &Path,
    answer: Answer<'_>,
    cut: Duration,
    group: &std::sync::atomic::AtomicI32,
) -> io::Result<String> {
    use std::io::{BufRead, BufReader, Write};
    let argv = if spec.wall == mork_types::job::WallMode::None { spec.argv.clone() } else { wall::argv_short(spec) };
    proclog::record(home_root, "limits", &argv, &spec.cwd, spec.wall != mork_types::job::WallMode::None);
    let (program, args) = argv.split_first().ok_or_else(|| io::Error::other("empty argv"))?;
    let mut cmd = Command::new(program);
    let input = if matches!(answer, Answer::Rpc(_)) { Stdio::piped() } else { Stdio::null() };
    cmd.args(args).stdin(input).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0);
    if spec.wall == mork_types::job::WallMode::None {
        cmd.env_clear().envs(&spec.env).current_dir(&spec.cwd);
    }
    let mut child = cmd.spawn()?;
    let pgid = i32::try_from(child.id()).unwrap_or(0);
    group.store(pgid, std::sync::atomic::Ordering::SeqCst);
    // Kept open until the read ends: the server answers while its stdin is open.
    let mut stdin = child.stdin.take();
    if let (Some(input), Answer::Rpc(lines)) = (stdin.as_mut(), answer) {
        for l in lines {
            let _ = writeln!(input, "{l}");
        }
    }
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    let result = read_answer(&rx, &mut child, answer, cut);
    if pgid > 0 {
        let _ = mork_sys::kill(-pgid, super::procs::libc_sig::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
    group.store(0, std::sync::atomic::Ordering::SeqCst);
    drop(stdin);
    result
}

fn read_answer(
    rx: &std::sync::mpsc::Receiver<String>,
    child: &mut Child,
    answer: Answer<'_>,
    cut: Duration,
) -> io::Result<String> {
    use std::sync::mpsc::RecvTimeoutError;
    let start = clock::instant();
    let cut_off = || io::Error::new(io::ErrorKind::TimedOut, format!("the limits read was cut after {cut:?}"));
    let mut text = String::new();
    loop {
        // Lines already buffered never outrun the cut.
        let left = cut.saturating_sub(clock::elapsed(start));
        if left.is_zero() {
            return Err(cut_off());
        }
        match rx.recv_timeout(left.min(Duration::from_millis(200))) {
            Ok(line) => {
                let id = serde_json::from_str::<serde_json::Value>(&line).ok().and_then(|v| v.get("id")?.as_u64());
                text.push_str(&line);
                text.push('\n');
                if matches!(answer, Answer::Rpc(_)) && id == Some(2) {
                    return Ok(text);
                }
            }
            Err(RecvTimeoutError::Disconnected) if matches!(answer, Answer::AtExit) => {
                return exited(child, start, cut).ok_or_else(cut_off)?.map(|()| text);
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the limits read ended with no answer"));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

// Its output closed: its exit, within what is left of the cut; none if it outlasts it.
fn exited(child: &mut Child, start: std::time::Instant, cut: Duration) -> Option<io::Result<()>> {
    while clock::elapsed(start) < cut {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Some(Ok(())),
            Ok(Some(status)) => return Some(Err(io::Error::other(format!("the limits read exited {status}")))),
            Ok(None) => clock::sleep(Duration::from_millis(20)),
            Err(e) => return Some(Err(e)),
        }
    }
    None
}

/// Ends a short command's process group, if it still runs.
pub fn end_group(group: &std::sync::atomic::AtomicI32) {
    let pgid = group.swap(0, std::sync::atomic::Ordering::SeqCst);
    if pgid > 0 {
        let _ = mork_sys::kill(-pgid, super::procs::libc_sig::KILL);
    }
}
