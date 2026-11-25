// Reads no secret's content: says only whether each path could be read, each socket reached (1.11, 5).

use std::net::{SocketAddr as TcpAddr, TcpStream};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What to look at.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Spec {
    /// Files that must not be readable.
    pub secrets: Vec<String>,
    /// Paths that must not be there.
    pub absent: Vec<String>,
    /// Abstract socket names that must not connect.
    pub abstract_names: Vec<String>,
    /// `host:port`s that must not connect.
    pub tcp: Vec<String>,
    /// A socket planted outside the allowlist, when one could be.
    pub planted: Option<String>,
    /// The live check's own pid on the host: the host's `/proc` would show it.
    pub host_pid: u32,
}

/// Runs inside the wall; prints what it saw.
pub fn run(args: &[String]) -> i32 {
    let Some(spec) = args.first().and_then(|a| serde_json::from_str::<Spec>(a).ok()) else {
        eprintln!("factory live-probe <spec as JSON>");
        return 2;
    };
    println!("{}", look(&spec));
    0
}

fn look(spec: &Spec) -> Value {
    let readable: Vec<&String> = spec.secrets.iter().filter(|p| std::fs::read(p).is_ok()).collect();
    let present: Vec<&String> = spec.absent.iter().filter(|p| Path::new(p).symlink_metadata().is_ok()).collect();
    let abstract_ok: Vec<&String> = spec
        .abstract_names
        .iter()
        .filter(|n| SocketAddr::from_abstract_name(n.as_bytes()).is_ok_and(|a| UnixStream::connect_addr(&a).is_ok()))
        .collect();
    let tcp_ok: Vec<&String> = spec
        .tcp
        .iter()
        .filter(|a| {
            a.parse::<TcpAddr>().is_ok_and(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok())
        })
        .collect();
    let planted = spec
        .planted
        .as_ref()
        .map(|p| json!({"seen": Path::new(p).symlink_metadata().is_ok(), "connects": UnixStream::connect(p).is_ok()}));
    let mut sockets = Vec::new();
    sockets_under(Path::new("/"), 0, &mut sockets);
    sockets.sort();
    let host_proc = std::fs::read(format!("/proc/{}/cmdline", spec.host_pid))
        .is_ok_and(|c| String::from_utf8_lossy(&c).contains("live-check"));
    json!({"readable": readable, "present": present, "abstract": abstract_ok, "tcp": tcp_ok,
           "planted": planted, "sockets": sockets, "fds": fds(), "host_proc": host_proc})
}

// No symlink followed.
fn sockets_under(dir: &Path, depth: u32, out: &mut Vec<String>) {
    if depth > 24 || ["/proc", "/sys", "/dev"].iter().any(|s| dir == Path::new(s)) {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            sockets_under(&e.path(), depth + 1, out);
        } else if kind.is_socket() {
            out.push(e.path().display().to_string());
        }
    }
}

fn fds() -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc/self/fd").into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if ["0", "1", "2"].contains(&n.as_str()) {
            continue;
        }
        let target = std::fs::read_link(e.path()).map(|p| p.display().to_string()).unwrap_or_default();
        // The listing's own descriptor points at this process's `fd` folder.
        if target.ends_with("/fd") && target.starts_with("/proc/") {
            continue;
        }
        out.push(format!("{n} → {target}"));
    }
    out
}
