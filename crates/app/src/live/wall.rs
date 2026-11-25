// Dummy secrets and sockets are planted outside a wall made as an agent's is (1.11, 5).

use std::net::TcpListener;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener};
use std::path::{Path, PathBuf};
use std::time::Duration;

use factory_engine::jobs::context::Context;
use factory_engine::jobs::{folder, launch};
use factory_engine::paths::Home;
use factory_types::job::{JobKind, WallMode};
use factory_types::{AgentKey, JobId};
use serde_json::Value;

use super::Report;
use super::probe::Spec;
use super::scratch::Scratch;

// Held open while the probe runs.
struct Planted {
    secret: PathBuf,
    abstract_name: String,
    _abstract: UnixListener,
    tcp: TcpListener,
    // A world-writable socket in a host folder off the allowlist (`/var/crash`), or why it
    // couldn't be made there: then its check fails (M5.20).
    crash: Result<(PathBuf, UnixListener), String>,
}

impl Planted {
    fn new(s: &Scratch) -> Result<Self, String> {
        let secret = s.root.join("outside/dummy-secret.txt");
        std::fs::create_dir_all(s.root.join("outside")).map_err(|e| e.to_string())?;
        std::fs::write(&secret, "dummy-live-check-secret-4b1d\n").map_err(|e| e.to_string())?;
        let abstract_name = format!("factory-live-{}", std::process::id());
        let addr = SocketAddr::from_abstract_name(abstract_name.as_bytes()).map_err(|e| e.to_string())?;
        let abstract_listener = UnixListener::bind_addr(&addr).map_err(|e| e.to_string())?;
        let tcp = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        Ok(Self { secret, abstract_name, _abstract: abstract_listener, tcp, crash: crash() })
    }
}

fn crash() -> Result<(PathBuf, UnixListener), String> {
    let dir = factory_engine::faults::test_env("FACTORY_TEST_CRASH_DIR").unwrap_or_else(|| "/var/crash".into());
    let path = Path::new(&dir).join(format!("factory-live-{}.sock", std::process::id()));
    let l = UnixListener::bind(&path).map_err(|e| format!("{} can't be made: {e}", path.display()))?;
    if let Err(e) = factory_sys::chmod(&path, 0o777) {
        let _ = std::fs::remove_file(&path);
        return Err(format!("{} can't be made world-writable: {e}", path.display()));
    }
    Ok((path, l))
}

impl Drop for Planted {
    fn drop(&mut self) {
        if let Ok((p, _)) = &self.crash {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Step 5.
pub fn run(s: &Scratch, real: &Home, bin: &Path, report: &mut Report) {
    let planted = match Planted::new(s) {
        Ok(p) => p,
        Err(why) => return report.check("the dummy secrets and sockets planted", Err(why)),
    };
    let spec = spec_of(s, real, &planted);
    match probe(s, bin, &spec) {
        Ok(seen) => judge(&seen, &planted, s, report),
        Err(why) => report.check("the probe runs in a wall", Err(why)),
    }
}

fn spec_of(s: &Scratch, real: &Home, planted: &Planted) -> Spec {
    let config = factory_engine::config::path().map(|p| p.display().to_string());
    let token = |name: &str| {
        std::env::var_os("HOME").map(|h| Path::new(&h).join(".config/factory").join(name).display().to_string())
    };
    let secrets = [
        Some(planted.secret.display().to_string()),
        Some(s.home.root().join("state.db").display().to_string()),
        Some(real.root().join("state.db").display().to_string()),
        config,
        token("bitbucket-token"),
        token("merge-token"),
    ];
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
    Spec {
        secrets: secrets.into_iter().flatten().collect(),
        absent: vec![
            s.home.factory_sock().display().to_string(),
            real.factory_sock().display().to_string(),
            "/run/dbus/system_bus_socket".into(),
            format!("{runtime}/bus"),
        ],
        abstract_names: vec!["/tmp/.X11-unix/X0".into(), "/tmp/.X11-unix/X1".into(), planted.abstract_name.clone()],
        tcp: planted.tcp.local_addr().map(|a| vec![a.to_string()]).unwrap_or_default(),
        planted: planted.crash.as_ref().ok().map(|(p, _)| p.display().to_string()),
        host_pid: std::process::id(),
    }
}

// Walled as an agent's turn is, so it sees what a turn would.
fn probe(s: &Scratch, bin: &Path, spec: &Spec) -> Result<Value, String> {
    let id = format!("{}-main-live-probe", factory_engine::clock::now_ms());
    let io = s.home.job(&id).join("io");
    // The wall a turn gets: `Context::places`, with the scratch config's toolchains (M5.19).
    let config = factory_engine::config::read(s.config_path()).map_err(|e| e.to_string())?;
    let ctx = Context::new(s.home.clone(), config, bin.to_owned(), Default::default());
    let places = ctx.places(&io, Vec::new(), Vec::new(), None, &[])?;
    let (id, owner) = (JobId::new(&id), AgentKey::new("main/live-probe"));
    let env = ctx.env(&id, &owner, None, &[], &io);
    let argv =
        vec![bin.display().to_string(), "live-probe".into(), serde_json::to_string(spec).map_err(|e| e.to_string())?];
    let agent_home = s.home.agent_home().display().to_string();
    let now = factory_engine::clock::now_ms();
    let job = ctx.spec(&id, JobKind::Turn, &owner, argv, &agent_home, &places, env, Some(vec![443, 80, 53]), now);
    if job.wall != WallMode::Full {
        return Err("the walls are off in this build".into());
    }
    let new = folder::NewFolder { spec: &job, stdin: b"", io_files: &[] };
    let dir = folder::write(&s.home.root().join("jobs"), &new).map_err(|e| e.to_string())?;
    launch::start_supervisor(bin, &dir.dir, s.home.root(), false).map_err(|e| e.to_string())?;
    let start = factory_engine::clock::instant();
    while dir.exit().ok().flatten().is_none() {
        if super::scratch::interrupted() {
            return Err("interrupted".into());
        }
        if factory_engine::clock::elapsed(start) > Duration::from_secs(120) {
            return Err("the probe didn't end in 2 minutes".into());
        }
        factory_engine::clock::sleep(Duration::from_millis(200));
    }
    let out = std::fs::read_to_string(dir.out()).unwrap_or_default();
    out.lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l).ok().filter(|v| v.get("sockets").is_some()))
        .ok_or_else(|| {
            format!(
                "the probe printed nothing; its errors: {}",
                std::fs::read_to_string(dir.err()).unwrap_or_default().trim()
            )
        })
}

fn judge(seen: &Value, planted: &Planted, s: &Scratch, report: &mut Report) {
    let none = |key: &str| match seen[key].as_array() {
        Some(a) if a.is_empty() => Ok(()),
        Some(a) => Err(format!("{a:?}")),
        None => Err("not reported".to_owned()),
    };
    report.check("inside the wall, no planted secret is readable", none("readable"));
    report.check("inside the wall, factory.sock and D-Bus are absent", none("present"));
    let host = if seen["host_proc"] == false { Ok(()) } else { Err("the host's processes are visible".to_owned()) };
    report.check("inside the wall, the host's /proc is absent", host);
    report.check("inside the wall, Xwayland's and a planted abstract socket are refused", none("abstract"));
    report.check("inside the wall, a localhost port off the allowlist is refused", none("tcp"));
    let agent_sock = s.home.run_dir().join("agent.sock").display().to_string();
    let sockets = seen["sockets"].as_array().cloned().unwrap_or_default();
    let only = if sockets == [Value::from(agent_sock.clone())] { Ok(()) } else { Err(format!("{sockets:?}")) };
    report.check("inside the wall, the only socket is run/agent.sock", only);
    let crash = match &planted.crash {
        Ok(_) if seen["planted"]["seen"] == false && seen["planted"]["connects"] == false => Ok(()),
        Ok(_) => Err(format!("{}", seen["planted"])),
        Err(why) => Err(why.clone()),
    };
    report.check("inside the wall, a socket planted in /var/crash is invisible and refused", crash);
    report.check("inside the wall, only descriptors 0–2 are open", none("fds"));
}
