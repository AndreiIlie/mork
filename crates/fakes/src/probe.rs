//! The probe (1.10.1): run inside a wall, it reports what an agent can reach. It never reads a
//! real secret: tests plant dummy ones.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram, UnixStream};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

/// What to try.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Spec {
    /// Files to try reading.
    pub read: Vec<String>,
    /// Paths to test for existence.
    pub exists: Vec<String>,
    /// Unix stream sockets to connect to (and read a line from).
    pub unix: Vec<String>,
    /// Abstract stream socket names to connect to.
    pub abstract_stream: Vec<String>,
    /// Abstract datagram socket names to connect to.
    pub abstract_dgram: Vec<String>,
    /// `host:port` TCP connects.
    pub tcp: Vec<String>,
    /// Raw TCP exchanges (`addr` + `send`); reports the first line of the response.
    pub tcp_send: Vec<TcpExchange>,
    /// Folders to try writing a file into.
    pub writable: Vec<String>,
    /// Commands to run, each a program and its arguments (`$JOB` filled in), reported by exit code
    /// and the end of their stderr.
    pub exec: Vec<Vec<String>>,
    /// Folders to search for socket files, following no symlink (`find -type s`).
    pub sockets: Vec<String>,
    /// A text to search for, hex-encoded (so the spec doesn't hold it), in every readable file
    /// under `search_roots` (no symlink followed).
    pub search_hex: Option<String>,
    pub search_roots: Vec<String>,
    /// Folders the search skips, besides `/proc`, `/sys` and `/dev`.
    pub search_skip: Vec<String>,
}

/// One TCP request/response probe.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TcpExchange {
    pub addr: String,
    pub send: String,
}

fn err_word(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(1) => "EPERM".into(),
        Some(13) => "EACCES".into(),
        Some(2) => "ENOENT".into(),
        Some(111) => "ECONNREFUSED".into(),
        Some(30) => "EROFS".into(),
        _ => format!("ERR {e}"),
    }
}

fn try_read(p: &str) -> String {
    match std::fs::read_to_string(p) {
        Ok(t) => format!("READ {}", t.chars().take(40).collect::<String>()),
        Err(e) => err_word(&e),
    }
}

fn unix(p: &str) -> String {
    match UnixStream::connect(p) {
        Ok(mut s) => {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 200];
            let n = s.read(&mut buf).unwrap_or(0);
            format!("CONNECTED {}", String::from_utf8_lossy(&buf[..n]).trim())
        }
        Err(e) => err_word(&e),
    }
}

fn abstract_stream(name: &str) -> String {
    let r = SocketAddr::from_abstract_name(name.as_bytes()).and_then(|a| UnixStream::connect_addr(&a));
    match r {
        Ok(_) => "CONNECTED".into(),
        Err(e) => err_word(&e),
    }
}

fn abstract_dgram(name: &str) -> String {
    let r = UnixDatagram::unbound()
        .and_then(|s| SocketAddr::from_abstract_name(name.as_bytes()).and_then(|a| s.connect_addr(&a)));
    match r {
        Ok(()) => "CONNECTED".into(),
        Err(e) => err_word(&e),
    }
}

fn tcp(target: &str) -> String {
    use std::net::ToSocketAddrs;
    let Some(addr) = target.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return "ERR resolve".into() };
    match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(_) => "CONNECTED".into(),
        Err(e) => err_word(&e),
    }
}

fn tcp_send(ex: &TcpExchange) -> String {
    use std::io::{BufRead, BufReader};
    use std::net::ToSocketAddrs;
    let Some(addr) = ex.addr.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return "ERR resolve".into() };
    let mut s = match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(s) => s,
        Err(e) => return err_word(&e),
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    if let Err(e) = s.write_all(ex.send.as_bytes()) {
        return err_word(&e);
    }
    let mut line = String::new();
    let _ = BufReader::new(s.take(256)).read_line(&mut line);
    line.trim_end_matches(['\r', '\n']).to_owned()
}

fn writable(dir: &str) -> String {
    let p = std::path::Path::new(dir).join(format!("probe-write-{}", std::process::id()));
    match std::fs::write(&p, b"x") {
        Ok(()) => {
            let _ = std::fs::remove_file(&p);
            "WROTE".into()
        }
        Err(e) => err_word(&e),
    }
}

fn exec(argv: &[String]) -> Value {
    let Some((program, args)) = argv.split_first() else { return json!({"code": null, "err": "empty"}) };
    match Command::new(fill(program)).args(args.iter().map(|a| fill(a))).output() {
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let tail: String = err.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
            json!({"code": o.status.code(), "err": tail})
        }
        Err(e) => json!({"code": null, "err": e.to_string()}),
    }
}

fn run(program: &str, args: &[&str]) -> Value {
    match Command::new(program).args(args).output() {
        Ok(o) => {
            json!({"code": o.status.code(), "err": String::from_utf8_lossy(&o.stderr).trim().chars().take(120).collect::<String>()})
        }
        Err(e) => json!({"code": null, "err": e.to_string()}),
    }
}

// A root that is itself a socket counts (the resolver's file).
fn sockets_under(root: &std::path::Path, out: &mut Vec<String>) {
    use std::os::unix::fs::FileTypeExt;
    if std::fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_socket()) {
        out.push(root.display().to_string());
    }
    find_sockets(root, out);
}

// Follows no symlink.
fn find_sockets(root: &std::path::Path, out: &mut Vec<String>) {
    use std::os::unix::fs::FileTypeExt;
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_socket() {
            out.push(e.path().display().to_string());
        } else if t.is_dir() && !t.is_symlink() {
            find_sockets(&e.path(), out);
        }
    }
}

// Follows no symlink; `/proc`, `/sys` and `/dev` are never searched.
fn find_text(root: &std::path::Path, needle: &[u8], skip: &[String], out: &mut Vec<String>) {
    let skipped = ["/proc", "/sys", "/dev"].iter().map(|s| (*s).to_owned()).chain(skip.iter().cloned());
    if skipped.into_iter().any(|p| root == std::path::Path::new(&p)) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() && !t.is_symlink() {
            find_text(&e.path(), needle, skip, out);
        } else if t.is_file()
            && e.metadata().is_ok_and(|m| m.len() <= 1 << 20)
            && let Ok(bytes) = std::fs::read(e.path())
            && bytes.windows(needle.len()).any(|w| w == needle)
        {
            out.push(e.path().display().to_string());
        }
    }
}

// `$JOB` in a path is this job's id (`MORK_JOB`), so a spec can name the job's own folder; `$IO` its
// `MORK_IO`, its seat's folder (fac-654).
fn fill(p: &str) -> String {
    let p = p.replace("$HOME", &std::env::var("HOME").unwrap_or_default());
    let p = p.replace("$IO", &std::env::var("MORK_IO").unwrap_or_default());
    p.replace("$JOB", &std::env::var("MORK_JOB").unwrap_or_default())
}

// Keyed by the path as the spec wrote it.
fn map(items: &[String], f: fn(&str) -> String) -> BTreeMap<String, String> {
    items.iter().map(|i| (i.clone(), f(&fill(i)))).collect()
}

/// Runs every check of `spec`, and the ones always made, as one JSON object.
pub fn report(spec: &Spec) -> Value {
    // The folder read itself holds a descriptor; it isn't one the probe was given.
    let fds: Vec<String> = std::fs::read_dir("/proc/self/fd")
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    std::fs::read_link(e.path()).map(|t| !t.to_string_lossy().ends_with("/fd")).unwrap_or(false)
                })
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let pids: Vec<i64> = std::fs::read_dir("/proc")
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect())
        .unwrap_or_default();
    let status: Vec<String> = std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|l| ["Uid", "CapEff", "NoNewPrivs"].iter().any(|k| l.starts_with(k)))
        .map(str::to_owned)
        .collect();
    let mut sockets = Vec::new();
    for r in &spec.sockets {
        sockets_under(&std::path::PathBuf::from(fill(r)), &mut sockets);
    }
    sockets.sort();
    sockets.dedup();
    // Which roots the scan could walk: a root not there, or not bound, shows here.
    let walked: BTreeMap<String, bool> =
        spec.sockets.iter().map(|r| (r.clone(), std::fs::read_dir(fill(r)).is_ok())).collect();
    let mut found = Vec::new();
    let needle: Vec<u8> = spec
        .search_hex
        .as_deref()
        .map(|h| (0..h.len() / 2).filter_map(|i| u8::from_str_radix(h.get(2 * i..2 * i + 2)?, 16).ok()).collect())
        .unwrap_or_default();
    if !needle.is_empty() {
        for r in &spec.search_roots {
            find_text(std::path::Path::new(&fill(r)), &needle, &spec.search_skip, &mut found);
        }
    }
    json!({
        "type": "probe",
        "read": map(&spec.read, try_read),
        "exists": spec.exists.iter().map(|p| (p.clone(), std::path::Path::new(&fill(p)).exists())).collect::<BTreeMap<_, _>>(),
        "unix": map(&spec.unix, unix),
        "abstract_stream": map(&spec.abstract_stream, abstract_stream),
        "abstract_dgram": map(&spec.abstract_dgram, abstract_dgram),
        "tcp": map(&spec.tcp, tcp),
        "tcp_send": spec.tcp_send.iter().map(tcp_send).collect::<Vec<_>>(),
        "writable": map(&spec.writable, writable),
        "exec": spec.exec.iter().map(|a| exec(a)).collect::<Vec<_>>(),
        "sockets": sockets,
        "walked": walked,
        "found": found,
        "fds": fds,
        "pids": pids,
        "status": status,
        "unshare_user": run("unshare", &["-U", "true"]),
        "mount": run("mount", &["-t", "tmpfs", "none", "/mnt"]),
        "cwd": std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        "env_keys": std::env::vars().map(|(k, _)| k).collect::<Vec<_>>(),
    })
}
