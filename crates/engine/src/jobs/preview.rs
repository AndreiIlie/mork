//! A card's preview worker (fac-196), or its mock host (fac-258): one long-lived walled process,
//! the engine's own and no turn's, its output appended to its clone's log. Its guard ends it with the app.

use std::io;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::Duration;

use factory_types::job::{JobSpec, WallMode};

use super::procs::libc_sig;
use super::{launch, proclog, wall};
use crate::clock;

/// A running worker; dropping it ends it.
pub struct Worker {
    child: Child,
    group: i32,
}

/// Starts `command`, a `kind` of worker, in `spec`'s wall under a guard leading its own group, its output appended
/// to the clone's `log`: opened by the walled shell, or beneath the clone with no link followed when unwalled.
pub fn start(home_root: &Path, spec: &JobSpec, kind: &str, log: &str, command: &str) -> io::Result<Worker> {
    let walled = spec.wall != WallMode::None;
    let started = format!("echo 'factory: {kind} started at {}'", clock::now_ms());
    let script =
        if walled { format!("exec >>{log} 2>&1; {started}; {command}") } else { format!("{started}; {command}") };
    let mut spec = spec.clone();
    spec.argv = vec!["sh".into(), "-c".into(), script];
    let argv = if walled { wall::argv_short(&spec) } else { spec.argv.clone() };
    proclog::record(home_root, kind, &argv, &spec.cwd, walled);
    let (out, err) = if walled {
        (Stdio::null(), Stdio::null())
    } else {
        let f = factory_sys::append_beneath(Path::new(&spec.cwd), Path::new(log))?;
        (Stdio::from(f.try_clone()?), Stdio::from(f))
    };
    let mut cmd = launch::guard(&argv);
    launch::without_git_env(&mut cmd);
    if !walled {
        cmd.env_clear().envs(&spec.env).current_dir(&spec.cwd);
    }
    let child = cmd.stdin(Stdio::null()).stdout(out).stderr(err).spawn()?;
    // Never 0: a signal to group 0 would reach the app's own group.
    let group = i32::try_from(child.id()).unwrap_or(i32::MAX);
    Ok(Worker { child, group })
}

impl Worker {
    /// How it ended, once it has.
    pub fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(s)) => Some(s.to_string()),
            Ok(None) => None,
            Err(e) => Some(format!("it couldn't be checked: {e}")),
        }
    }
}

impl Drop for Worker {
    // Its guard kills what it started on SIGTERM; what is left of its group goes after a grace.
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = factory_sys::kill(-self.group, libc_sig::TERM);
            let ends = clock::instant();
            while matches!(self.child.try_wait(), Ok(None)) && clock::elapsed(ends) < Duration::from_secs(3) {
                clock::sleep(Duration::from_millis(10));
            }
            let _ = factory_sys::kill(-self.group, libc_sig::KILL);
        }
        let _ = self.child.wait();
    }
}
