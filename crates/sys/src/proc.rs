use std::io;
use std::os::unix::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkOutcome {
    Parent(i32),
    Child,
}

/// Forks. Only ever called by a single-threaded process (the supervisor before it spawns
/// anything), where a fork is sound.
pub fn fork() -> io::Result<ForkOutcome> {
    // SAFETY: the supervisor is single-threaded when it forks, so the child holds no lock
    // another thread could own.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(ForkOutcome::Child),
        p => Ok(ForkOutcome::Parent(p)),
    }
}

/// Starts a session of its own.
pub fn setsid() -> io::Result<()> {
    // SAFETY: setsid takes no pointers.
    let r = unsafe { libc::setsid() };
    if r == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Makes the calling process a child subreaper (fac-25): an orphan of its descendants is adopted
/// by it, never by init, so it stays in its tree.
pub fn set_child_subreaper() -> io::Result<()> {
    // SAFETY: prctl with integers.
    let r = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
    if r == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Sends a signal. `Ok(false)` when the process is gone.
pub fn kill(pid: i32, sig: i32) -> io::Result<bool> {
    // SAFETY: kill takes no pointers.
    let r = unsafe { libc::kill(pid, sig) };
    if r == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) { Ok(false) } else { Err(err) }
}

/// Reaps a child by pid without waiting (M3.24): `Ok(None)` while it runs, else its exit code,
/// 128 plus the signal's number for a signal.
pub fn try_reap(pid: i32) -> io::Result<Option<i32>> {
    let mut status = 0;
    // SAFETY: waitpid writes one int we own.
    let r = unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) };
    if r == -1 {
        return Err(io::Error::last_os_error());
    }
    if r == 0 {
        return Ok(None);
    }
    Ok(Some(if libc::WIFSIGNALED(status) { 128 + libc::WTERMSIG(status) } else { libc::WEXITSTATUS(status) }))
}

/// Sets no-new-privileges for this process and what it execs.
pub fn set_no_new_privs() -> io::Result<()> {
    // SAFETY: prctl with integer arguments only.
    let r = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Closes every descriptor from `min` up.
pub fn close_fds_from(min: i32) -> io::Result<()> {
    // SAFETY: close_range takes integers; closing descriptors we don't use is the point.
    let r = unsafe { libc::syscall(libc::SYS_close_range, min.cast_unsigned(), u32::MAX, 0u32) };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

static TERM: AtomicBool = AtomicBool::new(false);

extern "C" fn on_term(_sig: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

/// Installs a SIGTERM and SIGINT handler that only sets a flag, read by [`term_requested`]:
/// either is a clean shutdown (2.7).
pub fn install_term_flag() -> io::Result<()> {
    let handler: extern "C" fn(libc::c_int) = on_term;
    for sig in [libc::SIGTERM, libc::SIGINT] {
        // SAFETY: the handler only stores into an atomic, which is async-signal-safe.
        let r = unsafe { libc::signal(sig, handler as libc::sighandler_t) };
        if r == libc::SIG_ERR {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Whether SIGTERM or SIGINT arrived since [`install_term_flag`].
pub fn term_requested() -> bool {
    TERM.load(Ordering::SeqCst)
}

/// The child never outlives the caller (SIGKILL on parent death); every descriptor above the
/// kept ones is closed; `own_group` is the unwalled path's group (1.4.4.1).
pub fn prepare_wall_spawn(cmd: &mut std::process::Command, keep: Option<i32>, own_group: bool) {
    prepare_spawn(cmd, keep, None, own_group, libc::SIGKILL);
}

/// Guard for a short command (M3.34): SIGTERM on caller death kills its whole group; a subreaper
/// (M4.6), so a descendant that left the group by `setsid` is still ended.
pub fn prepare_guard_spawn(cmd: &mut std::process::Command) {
    prepare_spawn(cmd, None, None, true, libc::SIGTERM);
    // SAFETY: prctl with integers, in the child between fork and exec: async-signal-safe. The
    // attribute is kept across exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Preserves both the bwrap status pipe and the wall's bootstrap socket until exec.
pub fn prepare_bridge_spawn(cmd: &mut std::process::Command, status: i32, bridge: i32) {
    prepare_spawn(cmd, Some(status), Some(bridge), false, libc::SIGKILL);
}

fn prepare_spawn(cmd: &mut std::process::Command, keep: Option<i32>, bridge: Option<i32>, own_group: bool, death: i32) {
    let parent = std::process::id().cast_signed();
    let hook = move || -> io::Result<()> {
        // SAFETY: prctl, getppid, _exit and dup2 with integers, in the child between fork and
        // exec; all are async-signal-safe.
        unsafe {
            if libc::prctl(libc::PR_SET_PDEATHSIG, death, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            // The parent died before the signal was set: it would never come. An error std can't
            // report aborts, a core dump holding the app's lock and sockets: it ends (fac-184).
            if libc::getppid() != parent {
                libc::_exit(1);
            }
            // Preserve the bridge source before descriptor 3 can be overwritten.
            let bridge_copy = bridge.map(|fd| libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 5));
            if bridge_copy == Some(-1) {
                return Err(io::Error::last_os_error());
            }
            if let Some(fd) = keep {
                let r = if fd == 3 { libc::fcntl(3, libc::F_SETFD, 0) } else { libc::dup2(fd, 3) };
                if r == -1 {
                    return Err(io::Error::last_os_error());
                }
            }
            if let Some(fd) = bridge_copy && libc::dup2(fd, crate::wall_bridge::BRIDGE_FD) < 0 {
                return Err(io::Error::last_os_error());
            }
            if own_group && libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let first = 3 + i32::from(keep.is_some()) + i32::from(bridge.is_some());
        close_fds_from(first)
    };
    // SAFETY: the hook calls only async-signal-safe functions (prctl, getppid, _exit, dup2,
    // setpgid, close_range), and allocates nothing.
    unsafe {
        cmd.pre_exec(hook);
    }
}

/// Points stdin at `input` and stdout and stderr at `output` (the supervisor's own log).
pub fn redirect_stdio(input: &std::fs::File, output: &std::fs::File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    for (from, to) in [(input.as_raw_fd(), 0), (output.as_raw_fd(), 1), (output.as_raw_fd(), 2)] {
        // SAFETY: dup2 on descriptors we own.
        if unsafe { libc::dup2(from, to) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
