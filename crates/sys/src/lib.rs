//! Every `unsafe` block of the workspace lives here (1.12.2), each with its `// SAFETY:` line.
//! Callers use the safe wrappers only.

mod interface_addresses;
pub use interface_addresses::interface_addresses;
mod landlock;
mod proc;
mod seccomp;
pub mod wall_bridge;

pub use landlock::{LandlockAbi, Restriction, abi as landlock_abi, restrict_self};
pub use proc::{
    ForkOutcome, close_fds_from, fork, install_term_flag, kill, prepare_bridge_spawn, prepare_guard_spawn, prepare_wall_spawn,
    redirect_stdio, set_child_subreaper, set_no_new_privs, setsid, term_requested, try_reap,
};
pub use seccomp::install_wall_filter;

use std::io;
use std::os::fd::AsRawFd;

/// Takes an exclusive `flock` on the file without waiting. `Ok(false)` means another
/// process holds it.
pub fn try_lock_exclusive(file: &std::fs::File) -> io::Result<bool> {
    // SAFETY: flock on a descriptor we own; no memory is shared with the kernel.
    let r = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if r == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) { Ok(false) } else { Err(err) }
}

/// Takes a shared `flock` without waiting: `Ok(true)` when nobody holds it exclusively.
/// Used to test whether a job's lock is held, then released at once.
pub fn lock_is_free(file: &std::fs::File) -> io::Result<bool> {
    // SAFETY: flock on a descriptor we own.
    let r = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
    if r == 0 {
        // SAFETY: releasing the lock we just took, on the same descriptor.
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) { Ok(false) } else { Err(err) }
}

pub fn getpid() -> i32 {
    std::process::id().cast_signed()
}

/// Peer credentials of a connected unix socket (`SO_PEERCRED`): the peer's pid.
pub fn peer_pid(sock: &impl AsRawFd) -> io::Result<i32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: getsockopt writes at most `len` bytes into `cred`, which is that size.
    let r = unsafe {
        libc::getsockopt(sock.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&raw mut cred).cast(), &raw mut len)
    };
    if r == 0 { Ok(cred.pid) } else { Err(io::Error::last_os_error()) }
}

/// Sets the mode bits of a path (sockets are made 0600).
pub fn chmod(path: &std::path::Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Makes a descriptor survive `exec` (clears close-on-exec): `app.lock` and the sockets'
/// listeners are kept across a reload (1.8.6.5).
pub fn keep_across_exec(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor the caller owns, with integer arguments.
    let r = unsafe { libc::fcntl(fd, libc::F_SETFD, 0) };
    if r == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Makes a descriptor close on `exec` again: an adopted one, or one a failed exec kept, must
/// not reach a supervisor the app starts later (M1.3).
pub fn close_on_exec(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor the caller owns, with integer arguments.
    let r = unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    if r == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Makes writes to a descriptor return at once when they can't go on: a live turn's stdin is written by its
/// supervisor, which must never wait on it (fac-389).
pub fn set_nonblocking(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor the caller owns, with integer arguments.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above.
    let r = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if r == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Takes ownership of a listening unix socket inherited across `exec`; it closes on the next
/// `exec` again.
pub fn listener_from_fd(fd: std::os::fd::RawFd) -> io::Result<std::os::unix::net::UnixListener> {
    close_on_exec(fd)?;
    use std::os::fd::FromRawFd;
    // SAFETY: the descriptor was passed by the process that exec'd us, which kept it open for
    // exactly this, and nothing else in this process owns it.
    Ok(unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) })
}

/// Takes ownership of a file inherited across `exec` (`app.lock`).
pub fn file_from_fd(fd: std::os::fd::RawFd) -> io::Result<std::fs::File> {
    close_on_exec(fd)?;
    use std::os::fd::FromRawFd;
    // SAFETY: as for `listener_from_fd`: an inherited descriptor nothing else owns.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

/// Opens a file for reading without following a symlink at its last component (`O_NOFOLLOW`):
/// a file a wall could have replaced is read as itself or not at all (M1.1).
pub fn open_no_follow(path: &std::path::Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path)
}

/// Creates (or truncates) a file for writing without following a symlink at its last
/// component: a link planted in a clone is never written through (R9).
pub fn create_no_follow(path: &std::path::Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

/// Opens `rel` beneath `dir` for reading, following no symlink and never leaving `dir`: a file
/// an agent put in its `io/` is read as itself, never as a link's target (M2.1). A FIFO doesn't block.
pub fn open_beneath(dir: &std::path::Path, rel: &std::path::Path) -> io::Result<std::fs::File> {
    open_file_beneath(&dir_of(dir)?, rel)
}

/// Reads beneath the caller's open directory without following links or waiting on a FIFO.
pub fn open_file_beneath(dir: &std::fs::File, rel: &std::path::Path) -> io::Result<std::fs::File> {
    openat2(dir, rel, libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC, 0, BENEATH)
}

/// Opens the absolute `path` for reading, following no symlink at any component: a configured
/// path is read as written, or not at all. A FIFO doesn't block.
pub fn open_no_symlinks(path: &std::path::Path) -> io::Result<std::fs::File> {
    from_root(path, libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
}

/// As `open_beneath`, with the absolute `dir` itself reached through no symlink either, and a
/// FIFO's open not blocking.
pub fn open_beneath_no_symlinks(dir: &std::path::Path, rel: &std::path::Path) -> io::Result<std::fs::File> {
    let base = from_root(dir, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)?;
    open_file_beneath(&base, rel)
}

/// Opens `rel` beneath `dir` for appending, `0600` when created, following no symlink: a log a
/// wall replaced with a link is never written through (R9).
pub fn append_beneath(dir: &std::path::Path, rel: &std::path::Path) -> io::Result<std::fs::File> {
    let base = dir_of(dir)?;
    let flags = libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    openat2(&base, rel, flags, 0o600, BENEATH)
}

/// Opens the folder `rel` beneath the open folder `dir`, following no symlink: a walk down an
/// agent's `io/` stays in the folders it opened, whatever an agent swaps in since.
pub fn open_dir_beneath(dir: &std::fs::File, rel: &std::path::Path) -> io::Result<std::fs::File> {
    openat2(dir, rel, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0, BENEATH)
}

/// Makes the folder `rel` beneath `dir`, `0700`, through no symlink; one already there is kept
/// when it is a folder, not a link: nothing is made where a planted link points (R9).
pub fn mkdir_beneath(dir: &std::path::Path, rel: &std::path::Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "no folder name");
    let base = dir_of(dir)?;
    let name = rel.file_name().ok_or_else(invalid)?;
    let parent = match rel.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(p) => open_dir_beneath(&base, p)?,
        None => base,
    };
    let cname = std::ffi::CString::new(name.as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // SAFETY: mkdirat reads `cname`, a NUL-terminated string we own, beside a live descriptor.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), cname.as_ptr(), 0o700) } != 0 {
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::AlreadyExists {
            return Err(e);
        }
    }
    open_dir_beneath(&parent, std::path::Path::new(name)).map(drop)
}

/// Moves a folder between relative paths under two roots, through no symlink below either root and
/// without replacing an existing destination: agent-written parents cannot redirect the move (R9).
pub fn move_dir_beneath(
    from: &std::path::Path,
    source: &std::path::Path,
    to: &std::path::Path,
    dest: &std::path::Path,
) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let parent = |root: &std::path::Path, rel: &std::path::Path| {
        let rel = rel.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        open_beneath(root, rel)
    };
    let name = |rel: &std::path::Path| {
        let name = rel.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder name"))?;
        std::ffi::CString::new(name.as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
    };
    let (source_parent, dest_parent) = (parent(from, source)?, parent(to, dest)?);
    let (source_name, dest_name) = (name(source)?, name(dest)?);
    open_dir_beneath(&source_parent, std::path::Path::new(source.file_name().unwrap_or_default()))?;
    // SAFETY: renameat2 reads two owned NUL-terminated names beside live parent descriptors.
    let r = unsafe {
        libc::renameat2(
            source_parent.as_raw_fd(),
            source_name.as_ptr(),
            dest_parent.as_raw_fd(),
            dest_name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Writes `bytes` to `rel` beneath `dir`, `0600` when created, following no symlink (R9).
/// A FIFO planted there fails the open rather than blocking it.
pub fn write_beneath(dir: &std::path::Path, rel: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    openat2(&dir_of(dir)?, rel, flags, 0o600, BENEATH)?.write_all(bytes)
}

fn dir_of(dir: &std::path::Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dir)
}

/// The entries of the open folder `dir`, read through its descriptor, never a path an agent
/// could swap; an entry's metadata is a symlink's own.
pub fn read_dir_of(dir: &std::fs::File) -> io::Result<std::fs::ReadDir> {
    std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))
}

/// Whether a beneath open failed on a symlink it refused to follow (`ELOOP`), or on a component
/// that is no folder (`ENOTDIR`): what was there, not a failure to read it.
pub fn not_followed(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
}

const BENEATH: u64 = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_MAGICLINKS;

fn from_root(path: &std::path::Path, flags: libc::c_int) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let rel =
        path.strip_prefix("/").map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not an absolute path"))?;
    let root = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC).open("/")?;
    openat2(&root, rel, flags, 0, libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_MAGICLINKS)
}

fn openat2(
    base: &std::fs::File,
    rel: &std::path::Path,
    flags: libc::c_int,
    mode: u64,
    resolve: u64,
) -> io::Result<std::fs::File> {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(rel.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let how = OpenHow { flags: u64::try_from(flags).unwrap_or_default(), mode, resolve };
    // SAFETY: openat2 reads `path` (a NUL-terminated string we own) and `how` (a live struct of
    // the size we pass); it returns a new descriptor or -1, and keeps no pointer.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            base.as_raw_fd(),
            path.as_ptr(),
            &raw const how,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = i32::try_from(fd).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // SAFETY: a descriptor openat2 just returned, owned by nothing else.
    Ok(std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

// The kernel's `struct open_how`: libc's is non-exhaustive.
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

/// Installs a link in an opened parent beneath the root. Existing links to the target are kept.
pub fn link_beneath(root: &std::path::Path, rel: &std::path::Path, target: &std::path::Path) -> io::Result<()> {
    use std::path::Path;
    let parent = rel.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let dir = open_dir_beneath(&dir_of(root)?, parent)?;
    let name = rel.file_name().ok_or_else(|| io::Error::other("missing link name"))?;
    let path = Path::new("/proc/self/fd").join(dir.as_raw_fd().to_string()).join(name);
    match std::os::unix::fs::symlink(target, &path) {
        Err(e)
            if e.kind() == io::ErrorKind::AlreadyExists
                && std::fs::read_link(&path).ok().as_deref() == Some(target) =>
        {
            Ok(())
        }
        result => result,
    }
}
