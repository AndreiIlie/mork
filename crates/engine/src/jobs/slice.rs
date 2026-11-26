//! `mork_work.slice` (fac-143): every job the supervisor starts runs in it, with a low CPU and
//! IO weight and a memory cap of the slice's own, so the desktop wins whenever it wants the machine.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

use mork_types::config::SliceConfig;
use mork_types::job::Launch;

use crate::{clock, faults};

/// The one slice every job's scope is made in. No dash: `mork-work` would nest in
/// `mork.slice`, alone there, its weight weighing against nothing of the desktop's.
pub const SLICE: &str = "mork_work.slice";
const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const SYSTEMCTL: &str = "/usr/bin/systemctl";
/// What `systemd-run --user` needs to reach the user's manager, kept out of a job's own environment.
pub const BUS_VARS: [&str; 2] = ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"];
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The argv the supervisor runs for a job's: `argv` itself, or `argv` started in the slice or
/// niced. `clear` names the bus variables `systemd-run` needs but the job's environment hasn't.
pub fn wrap(launch: Launch, argv: Vec<String>, clear: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = match launch {
        Launch::Plain => return argv,
        Launch::Nice => ["/usr/bin/nice", "-n", "19", "/usr/bin/ionice", "-c3"].map(String::from).into(),
        Launch::Slice => [SYSTEMD_RUN, "--user", "--scope", "--quiet", "--collect", "--expand-environment=no"]
            .map(String::from)
            .into(),
    };
    if launch == Launch::Slice {
        out.extend([format!("--slice={SLICE}"), "--".into()]);
        if !clear.is_empty() {
            out.push("/usr/bin/env".into());
            out.extend(clear.iter().flat_map(|v| ["-u".to_owned(), (*v).to_owned()]));
        }
    }
    out.extend(argv);
    out
}

/// How this engine starts its jobs: in the slice, given the config's weights and cap, when
/// `systemd-run --user` works here; else niced, with why.
pub fn choose(home_root: &Path, config: &SliceConfig) -> (Launch, Option<String>) {
    if faults::TEST_SETTINGS {
        return (Launch::Plain, None);
    }
    match probe(home_root).and_then(|()| set_properties(home_root, config)) {
        Ok(()) => (Launch::Slice, None),
        Err(why) => (Launch::Nice, Some(why)),
    }
}

/// `choose`, run while the start opens and loads the state (fac-336), its answer taken before any
/// job is decided (fac-143).
pub struct Choosing {
    root: PathBuf,
    config: SliceConfig,
    probe: Option<JoinHandle<(Launch, Option<String>)>>,
}

/// Starts choosing the jobs' launch, as `choose` does.
pub fn choosing(home_root: &Path, config: &SliceConfig) -> Choosing {
    let (root, config) = (home_root.to_owned(), config.clone());
    let (r, c) = (root.clone(), config.clone());
    let probe = std::thread::Builder::new().name("slice-probe".into()).spawn(move || choose(&r, &c)).ok();
    Choosing { root, config, probe }
}

impl Choosing {
    /// The launch chosen, waiting for the probe; one that couldn't start in the background is run now.
    pub fn chosen(self) -> (Launch, Option<String>) {
        match self.probe.map(JoinHandle::join) {
            Some(Ok(chosen)) => chosen,
            Some(Err(_)) => (Launch::Nice, Some("the slice's probe ended without an answer".into())),
            None => choose(&self.root, &self.config),
        }
    }
}

// A scope must keep the job's pid, its descriptor 3 (bwrap's status pipe) and each `$` of its
// command as written: systemd 259 expands `$$` itself unless told not to.
fn probe(home_root: &Path) -> Result<(), String> {
    let (mut reader, writer) = io::pipe().map_err(|e| e.to_string())?;
    let argv = wrap(Launch::Slice, ["/bin/sh", "-c", "echo $$ >&3"].map(String::from).into(), &[]);
    super::proclog::record(home_root, "slice-probe", &argv, "", false);
    let mut cmd = Command::new(&argv[0]);
    super::launch::without_git_env(&mut cmd);
    cmd.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    use std::os::fd::AsRawFd;
    mork_sys::prepare_wall_spawn(&mut cmd, Some(writer.as_raw_fd()), true);
    let mut child = cmd.spawn().map_err(|e| format!("{SYSTEMD_RUN} didn't start: {e}"))?;
    drop(writer);
    let start = clock::instant();
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        if clock::elapsed(start) > PROBE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("`systemd-run --user --scope` didn't end within {}s", PROBE_TIMEOUT.as_secs()));
        }
        clock::sleep(Duration::from_millis(10));
    };
    let mut said = String::new();
    let _ = reader.read_to_string(&mut said);
    if !status.success() {
        let mut err = String::new();
        let _ = child.stderr.take().map(|mut e| e.read_to_string(&mut err));
        return Err(format!("`systemd-run --user --scope` failed: {}", first_line(&err, &status.to_string())));
    }
    if said.trim() != child.id().to_string() {
        return Err("`systemd-run --user --scope` didn't keep its pid and descriptors".into());
    }
    Ok(())
}

fn set_properties(home_root: &Path, c: &SliceConfig) -> Result<(), String> {
    let props = [
        format!("CPUWeight={}", c.cpu_weight),
        format!("IOWeight={}", c.io_weight),
        format!("MemoryHigh={}", c.memory_high),
    ];
    let mut argv = vec![SYSTEMCTL, "--user", "set-property", "--runtime", SLICE];
    argv.extend(props.iter().map(String::as_str));
    let out = super::launch::short(home_root, &argv, Path::new("/"), PROBE_TIMEOUT)
        .map_err(|e| format!("{SYSTEMCTL} didn't run: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(format!("`systemctl --user set-property {SLICE}` failed: {}", first_line(&err, &out.status.to_string())))
}

fn first_line<'a>(text: &'a str, otherwise: &'a str) -> &'a str {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(otherwise)
}
