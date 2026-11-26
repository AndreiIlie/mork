//! The window's SSH forwards (fac-35) own their connections: no daemon or shared SSH master
//! may keep a forward alive after its tracker or the app exits.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use mork_types::config::TunnelConfig;
use mork_types::web_tunnel::Status;

use super::{
    door_ssh::{self, Scratch},
    launch, proclog,
};
use crate::{clock, paths::Home};

/// The window's explicit open or close request, never an engine command available to a seat.
pub type Request = (String, bool);

/// Starts the tracker off the window thread. Dropping its sender closes its owned forwards;
/// `changed` returning false closes them too. The process guard also ends them when the app dies.
pub fn start(
    home: Home,
    configured: BTreeMap<String, TunnelConfig>,
    changed: impl Fn(String, Status) -> bool + Send + 'static,
) -> Sender<Request> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || run(&home, &configured, rx, changed));
    tx
}

fn run(
    home: &Home,
    configured: &BTreeMap<String, TunnelConfig>,
    requests: Receiver<Request>,
    changed: impl Fn(String, Status) -> bool,
) {
    let mut tunnels: BTreeMap<String, Tunnel> = BTreeMap::new();
    let mut reopen = wanted(home, configured).into_iter();
    loop {
        let request = match reopen.next() {
            Some(name) => Ok((name, true)),
            None => requests.recv_timeout(Duration::from_millis(100)),
        };
        match request {
            Ok((name, open)) => {
                if !handle_request(home, configured, &mut tunnels, name, open, &changed) {
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if !poll_tunnels(&mut tunnels, &changed) {
            return;
        }
    }
}

fn handle_request(
    home: &Home,
    configured: &BTreeMap<String, TunnelConfig>,
    tunnels: &mut BTreeMap<String, Tunnel>,
    name: String,
    open: bool,
    changed: &impl Fn(String, Status) -> bool,
) -> bool {
    let Some(cfg) = configured.get(&name) else { return true };
    remember(home, configured, &name, open);
    if !open {
        tunnels.remove(&name);
        return changed(name, Status::Closed);
    }
    if tunnels.contains_key(&name) {
        return true;
    }
    let status = match Tunnel::open(home, &name, cfg) {
        Ok(t) => {
            tunnels.insert(name.clone(), t);
            Status::Opening
        }
        Err(e) => Status::Failed(e),
    };
    changed(name, status)
}

fn poll_tunnels(tunnels: &mut BTreeMap<String, Tunnel>, changed: &impl Fn(String, Status) -> bool) -> bool {
    let updates: Vec<_> = tunnels.iter_mut().filter_map(|(n, t)| t.poll().map(|s| (n.clone(), s))).collect();
    for (name, status) in updates {
        if status != Status::Open {
            tunnels.remove(&name);
        }
        if !changed(name, status) {
            return false;
        }
    }
    true
}

// The forwards the operator left open, opened again when the app starts: a restart or a reload ends them.
fn wanted(home: &Home, configured: &BTreeMap<String, TunnelConfig>) -> Vec<String> {
    let text = std::fs::read_to_string(open_list(home)).unwrap_or_default();
    configured.keys().filter(|name| text.lines().any(|l| l == name.as_str())).cloned().collect()
}

fn remember(home: &Home, configured: &BTreeMap<String, TunnelConfig>, name: &str, open: bool) {
    let mut open_ones = wanted(home, configured);
    open_ones.retain(|n| n != name);
    if open {
        open_ones.push(name.to_owned());
    }
    let text: String = open_ones.iter().map(|n| format!("{n}\n")).collect();
    let _ = std::fs::write(open_list(home), text);
}

fn open_list(home: &Home) -> PathBuf {
    home.door().join("web-tunnels-open")
}

fn argv(cfg: &TunnelConfig, socket: &Path) -> Vec<String> {
    let mut a = door_ssh::base(&[
        "ExitOnForwardFailure=yes",
        "ControlMaster=yes",
        "ControlPersist=no",
        "ForkAfterAuthentication=no",
    ]);
    a.extend(["-N".into(), "-S".into(), socket.display().to_string()]);
    a.extend([
        "-L".into(),
        format!("127.0.0.1:{}:127.0.0.1:{}", cfg.port, cfg.remote_port),
        "--".into(),
        cfg.alias.clone(),
    ]);
    a
}

struct Tunnel {
    child: Child,
    scratch: Scratch,
    errors: PathBuf,
    started: Instant,
    ready: bool,
}

impl Tunnel {
    fn open(home: &Home, name: &str, cfg: &TunnelConfig) -> Result<Self, String> {
        Self::open_using(home, name, |socket| argv(cfg, socket))
    }

    fn open_using(home: &Home, name: &str, args: impl FnOnce(&Path) -> Vec<String>) -> Result<Self, String> {
        let scratch = Scratch::make(&home.run_dir().join("web-tunnels"), name)?;
        let errors = scratch.secret("ssh.err", "")?;
        let args = args(&scratch.socket());
        proclog::record(home.root(), "web-tunnel", &args, "", false);
        let mut cmd = launch::guard(&args);
        launch::without_git_env(&mut cmd);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(File::create(&errors).map_err(|e| e.to_string())?);
        let child = cmd.spawn().map_err(|e| format!("SSH didn't start: {e}"))?;
        Ok(Self { child, scratch, errors, started: clock::instant(), ready: false })
    }

    fn poll(&mut self) -> Option<Status> {
        let failure = match self.child.try_wait() {
            Ok(Some(s)) => Some(format!("SSH exited ({s})")),
            Err(e) => Some(format!("SSH couldn't be checked: {e}")),
            Ok(None) if !self.ready && clock::elapsed(self.started) >= Duration::from_secs(20) => {
                Some("SSH didn't open before its deadline".into())
            }
            Ok(None) => None,
        };
        if let Some(why) = failure {
            let mut errors = String::new();
            if let Ok(file) = File::open(&self.errors) {
                let _ = file.take(4096).read_to_string(&mut errors);
            }
            let why = if errors.trim().is_empty() { why } else { format!("{why}: {}", errors.trim()) };
            return Some(if self.ready { Status::Dropped(why) } else { Status::Failed(why) });
        }
        // A TCP listener may belong to someone else; this private master socket is made only
        // after SSH authenticated and installed its forwards with ExitOnForwardFailure.
        if !self.ready && std::fs::symlink_metadata(self.scratch.socket()).is_ok_and(|m| m.file_type().is_socket()) {
            self.ready = true;
            return Some(Status::Open);
        }
        None
    }
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        // An exited child may already be reaped by poll; its PID must never be signalled again.
        if matches!(self.child.try_wait(), Ok(None))
            && let Ok(pid) = i32::try_from(self.child.id())
        {
            let _ = mork_sys::kill(pid, super::procs::libc_sig::TERM);
        }
        let _ = self.child.wait();
    }
}
