//! The window's SSH forwards (fac-35) own their connections: no daemon or shared SSH master
//! may keep a forward alive after its tracker or the app exits.

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use factory_types::web_tunnel::{Environment, Status};

use super::{
    door_ssh::{self, Scratch},
    launch, proclog,
};
use crate::{clock, paths::Home};

/// The window's explicit open or close request, never an engine command available to a seat.
pub type Request = (Environment, bool);

/// Starts the tracker off the window thread. Dropping its sender closes its owned forwards;
/// `changed` returning false closes them too. The process guard also ends them when the app dies.
pub fn start(home: Home, changed: impl Fn(Environment, Status) -> bool + Send + 'static) -> Sender<Request> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || run(&home, rx, changed));
    tx
}

fn run(home: &Home, requests: Receiver<Request>, changed: impl Fn(Environment, Status) -> bool) {
    let mut tunnels: [Option<Tunnel>; 2] = [None, None];
    let mut reopen = wanted(home).into_iter();
    loop {
        let request = match reopen.next() {
            Some(env) => Ok((env, true)),
            None => requests.recv_timeout(Duration::from_millis(100)),
        };
        match request {
            Ok((env, open)) => {
                remember(home, env, open);
                let at = &mut tunnels[env.index()];
                if !open {
                    *at = None;
                    if !changed(env, Status::Closed) {
                        return;
                    }
                } else if at.is_none() {
                    let status = match Tunnel::open(home, env) {
                        Ok(t) => {
                            *at = Some(t);
                            Status::Opening
                        }
                        Err(e) => Status::Failed(e),
                    };
                    if !changed(env, status) {
                        return;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        for env in Environment::ALL {
            let at = &mut tunnels[env.index()];
            if let Some(t) = at
                && let Some(status) = t.poll()
            {
                if status != Status::Open {
                    *at = None;
                }
                if !changed(env, status) {
                    return;
                }
            }
        }
    }
}

// The forwards Andrei left open, opened again when the app starts: a restart or a reload ends them.
fn wanted(home: &Home) -> Vec<Environment> {
    let text = std::fs::read_to_string(open_list(home)).unwrap_or_default();
    Environment::ALL.into_iter().filter(|e| text.lines().any(|l| l == e.name())).collect()
}

fn remember(home: &Home, env: Environment, open: bool) {
    let mut open_ones = wanted(home);
    open_ones.retain(|e| *e != env);
    if open {
        open_ones.push(env);
    }
    let text: String = open_ones.iter().map(|e| format!("{}\n", e.name())).collect();
    let _ = std::fs::write(open_list(home), text);
}

fn open_list(home: &Home) -> PathBuf {
    home.door().join("web-tunnels-open")
}

fn argv(env: Environment, socket: &Path) -> Vec<String> {
    let mut a = door_ssh::base(&[
        "ExitOnForwardFailure=yes",
        "ControlMaster=yes",
        "ControlPersist=no",
        "ForkAfterAuthentication=no",
    ]);
    a.extend(["-N".into(), "-S".into(), socket.display().to_string()]);
    a.extend(["-L".into(), format!("127.0.0.1:{}:127.0.0.1:9081", env.port()), "--".into(), env.alias().into()]);
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
    fn open(home: &Home, env: Environment) -> Result<Self, String> {
        Self::open_using(home, env, |socket| argv(env, socket))
    }

    fn open_using(home: &Home, env: Environment, args: impl FnOnce(&Path) -> Vec<String>) -> Result<Self, String> {
        let scratch = Scratch::make(&home.run_dir().join("web-tunnels"), env.name())?;
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
            let _ = factory_sys::kill(pid, super::procs::libc_sig::TERM);
        }
        let _ = self.child.wait();
    }
}
