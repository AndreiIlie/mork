//! The door over ssh (fac-86): Andrei's own alias and the box's own tools, nothing of the Factory's there.
//! A command is one remote string of single-quoted arguments, so the login shell reads each back as one literal word.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::clock;

// Never interactive, never forwarding this side's agent or display, never running a local
// command; the alias's host key as pinned, its connection given up when it goes quiet.
const OPTIONS: &[&str] = &[
    "BatchMode=yes",
    "StrictHostKeyChecking=yes",
    "ConnectTimeout=10",
    "ServerAliveInterval=10",
    "ServerAliveCountMax=2",
    "ForwardAgent=no",
    "ForwardX11=no",
    "PermitLocalCommand=no",
    "RequestTTY=no",
    "LogLevel=ERROR",
];

/// ssh's exit when it, not the remote command, ends the run: a connection lost or never made.
pub const SSH_FAILED: i32 = 255;

/// SSH's shared connection policy, followed by the caller's operation-specific options.
pub(crate) fn base(extra: &[&str]) -> Vec<String> {
    let mut a = vec!["ssh".to_owned()];
    for o in OPTIONS.iter().chain(extra) {
        a.extend(["-o".to_owned(), (*o).to_owned()]);
    }
    a
}

/// One argument as a POSIX shell reads it back, literally.
pub fn quoted(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// ssh running `argv` over the alias: its remote string is the arguments, each quoted.
pub fn command_argv(alias: &str, argv: &[String]) -> Vec<String> {
    let mut a = base(&["ClearAllForwardings=yes"]);
    let remote: Vec<String> = argv.iter().map(|x| quoted(x)).collect();
    a.extend(["--".to_owned(), alias.to_owned(), remote.join(" ")]);
    a
}

/// ssh forwarding `socket` on this side to `host:port` as the alias's box sees it; no command.
pub fn tunnel_argv(alias: &str, socket: &Path, host: &str, port: u16) -> Vec<String> {
    let mut a = base(&["ExitOnForwardFailure=yes", "StreamLocalBindUnlink=yes"]);
    let forward = format!("{}:{host}:{port}", socket.display());
    a.extend(["-N".to_owned(), "-L".to_owned(), forward, "--".to_owned(), alias.to_owned()]);
    a
}

// No symlink at any part of the path, the root included; each folder is entered from the last and proved the
// very entry just seen (device, inode, type), so a swapped part is refused; `dd` opens the file, following no symlink.
const READ: &str = r#"set -f; unset CDPATH; tool=$1 n=$2 at=${4:-$3}; IFS=/; set -- $3 $4; unset IFS
no() { echo "$at: $1" >&2; exit 3; }
cd / || no "its root can't be reached"; f=
for c; do [ -n "$c" ] || continue
case $c in .|..) no "a . or .. is on its path";; esac
if [ -n "$f" ]; then d=$(stat -c %d:%i:%F -- "./$f") || no "its path can't be read"
case $d in *:directory) ;; *:"symbolic link") no "a symlink is on its path";; *) no "not a regular file";; esac
cd -P -- "./$f" && [ "$(stat -c %d:%i:%F .)" = "$d" ] || no "its path changed while it was read"; fi
f=$c; done
case $(stat -c %F -- "./$f") in regular*) ;; "symbolic link") no "a symlink is on its path";;
*) no "not a regular file";; esac
case $tool in head) s=0;; tail) z=$(stat -c %s -- "./$f") || exit 3; s=$((z > n ? z - n : 0));;
*) no "no such read";; esac
exec dd if="./$f" iflag=nofollow,nonblock,skip_bytes,count_bytes bs=65536 skip="$s" count="$n" status=none"#;

/// The remote argv that reads `n` bytes of `root/path` (`head`) or of a log's end (`tail`, the
/// log as `root`, no `path`), with the box's own `sh`, `stat` and `dd`.
pub fn read_argv(tool: &str, n: u64, root: &str, path: &str) -> Vec<String> {
    let parts = ["sh", "-c", READ, "door-read", tool, &n.to_string(), root, path];
    parts.iter().map(|s| (*s).to_owned()).collect()
}

/// ssh's environment: the dispatcher's, and the ssh agent's socket when the engine has one, so a
/// key of Andrei's that has a passphrase still opens; the agent is never forwarded.
pub fn env(mut e: BTreeMap<String, String>) -> BTreeMap<String, String> {
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        e.insert("SSH_AUTH_SOCK".to_owned(), sock);
    }
    e
}

/// A forward held open for one call; ended (its ssh's group killed) when dropped.
pub struct Tunnel {
    child: Child,
    group: i32,
}

impl Tunnel {
    /// Opens it and waits for its socket until `deadline`; why not, in ssh's words, for the
    /// caller to mask. Its socket there means its connection is made and logged in.
    pub fn open(
        argv: &[String],
        socket: &Path,
        env: &BTreeMap<String, String>,
        deadline: Instant,
    ) -> Result<Self, String> {
        let (program, args) = argv.split_first().ok_or("an empty argv")?;
        let mut cmd = Command::new(program);
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).env_clear().envs(env);
        factory_sys::prepare_wall_spawn(&mut cmd, None, true);
        let child = cmd.spawn().map_err(|e| format!("its tunnel didn't start: {e}"))?;
        let group = -i32::try_from(child.id()).unwrap_or(i32::MAX);
        let mut t = Self { child, group };
        loop {
            if is_socket(socket) {
                return Ok(t);
            }
            if let Ok(Some(st)) = t.child.try_wait() {
                t.end();
                let mut err = String::new();
                if let Some(e) = t.child.stderr.take() {
                    let _ = e.take(4096).read_to_string(&mut err);
                }
                return Err(format!("its tunnel didn't open (exit {:?}): {}", st.code(), err.trim()));
            }
            if clock::instant() >= deadline {
                return Err("its tunnel didn't open before its deadline".into());
            }
            clock::sleep(Duration::from_millis(10));
        }
    }

    /// Whether its connection has ended.
    pub fn lost(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    fn end(&mut self) {
        let _ = factory_sys::kill(self.group, super::procs::libc_sig::KILL);
        let _ = self.child.kill();
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.end();
        let _ = self.child.wait();
    }
}

fn is_socket(p: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_socket())
}

// Kept under the kernel's 108-byte socket path limit.
const SOCKET_MAX: usize = 100;

const STALE: Duration = Duration::from_secs(60);

/// A call's own folder (0700) under `--tunnels`: its forward's socket, its client's option file. Gone when
/// dropped; one a killed dispatcher left (lock free, a minute old) is swept by the next call's.
pub struct Scratch {
    dir: PathBuf,
    lock: PathBuf,
    _held: std::fs::File,
}

impl Scratch {
    pub fn make(tunnels: &Path, call: &str) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        let private = |p: &Path| {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)
        };
        private(tunnels).map_err(|e| format!("its tunnels folder can't be made: {e}"))?;
        sweep(tunnels);
        let name: String = format!("{:x}", Sha256::digest(call.as_bytes())).chars().take(12).collect();
        let lock = tunnels.join(format!("{name}.lock"));
        let held = std::fs::File::create(&lock).map_err(|e| format!("its folder's lock: {e}"))?;
        if !factory_sys::try_lock_exclusive(&held).unwrap_or(false) {
            return Err("its folder is held by another dispatch".into());
        }
        let dir = tunnels.join(&name);
        let _ = std::fs::remove_dir_all(&dir);
        private(&dir).map_err(|e| format!("its folder can't be made: {e}"))?;
        let s = Self { dir, lock, _held: held };
        if s.socket().as_os_str().len() > SOCKET_MAX || s.socket().to_string_lossy().contains(':') {
            return Err("its tunnel's socket path is too long, or holds a `:`".into());
        }
        Ok(s)
    }

    /// The forward's socket: a `.sock`, as a MongoDB URI names one.
    pub fn socket(&self) -> PathBuf {
        self.dir.join("s.sock")
    }

    /// Writes a file only its owner reads, never over another.
    pub fn secret(&self, name: &str, text: &str) -> Result<PathBuf, String> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path = self.dir.join(name);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("its client's options can't be written: {e}"))?;
        f.write_all(text.as_bytes()).map_err(|e| format!("its client's options can't be written: {e}"))?;
        Ok(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_file(&self.lock);
    }
}

fn sweep(tunnels: &Path) {
    let Ok(entries) = std::fs::read_dir(tunnels) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".lock")) else {
            continue;
        };
        let modified = e.metadata().ok().and_then(|m| m.modified().ok());
        let since = modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        let age = since.map(|m| clock::real_now_ms() - i64::try_from(m.as_millis()).unwrap_or(i64::MAX));
        if age.is_none_or(|ms| ms < i64::try_from(STALE.as_millis()).unwrap_or(i64::MAX)) {
            continue;
        }
        if let Ok(f) = std::fs::File::open(&path)
            && factory_sys::try_lock_exclusive(&f).unwrap_or(false)
        {
            let _ = std::fs::remove_dir_all(tunnels.join(name));
            let _ = std::fs::remove_file(&path);
        }
    }
}
