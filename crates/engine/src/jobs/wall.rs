//! The wall's argv (1.4.11.3): `/usr/bin/bwrap` with its flags and mounts, then
//! `mork wall-exec <ports> -- <argv>`, or `--any-port` for its ports.

use mork_types::job::{JobSpec, Mount, WallMode};
use std::path::{Path, PathBuf};

use crate::files::HiddenPath;

/// bwrap's flags, before the mounts.
pub const FLAGS: &[&str] = &[
    "--die-with-parent",
    "--new-session",
    "--unshare-user",
    "--unshare-pid",
    "--unshare-ipc",
    "--unshare-uts",
    "--unshare-cgroup-try",
    "--disable-userns",
    "--cap-drop",
    "ALL",
    "--clearenv",
];

/// The system's bwrap: Ubuntu's AppArmor lets only this path create user namespaces.
pub const BWRAP: &str = "/usr/bin/bwrap";

/// The descriptor bwrap reports the init's pid on (the supervisor moves its pipe there).
pub const STATUS_FD: i32 = 3;

fn push_mount(a: &mut Vec<String>, m: &Mount) {
    let mut add = |xs: &[&str]| a.extend(xs.iter().map(|s| (*s).to_owned()));
    match m {
        Mount::Ro { path } => add(&["--ro-bind", path, path]),
        Mount::RoTry { path } => add(&["--ro-bind-try", path, path]),
        Mount::Rw { path } => add(&["--bind", path, path]),
        Mount::RwAt { src, dest } => add(&["--bind", src, dest]),
        Mount::RoAt { src, dest } => add(&["--ro-bind", src, dest]),
        Mount::RoTryAt { src, dest } => add(&["--ro-bind-try", src, dest]),
        Mount::Tmpfs { path } => add(&["--tmpfs", path]),
        Mount::Dev { path } => add(&["--dev", path]),
        Mount::Proc { path } => add(&["--proc", path]),
        Mount::Symlink { target, path } => add(&["--symlink", target, path]),
        Mount::RemountRo { path } => add(&["--remount-ro", path]),
    }
}

/// The whole argv of a walled job. `interactive` drops `--new-session` (`mork login`).
pub fn argv(spec: &JobSpec, interactive: bool) -> Vec<String> {
    argv_with(spec, !interactive, !interactive)
}

/// The argv of a walled short command (Codex's limits read): a session of its own, and no
/// status descriptor, since no supervisor reads one.
pub fn argv_short(spec: &JobSpec) -> Vec<String> {
    argv_with(spec, true, false)
}

fn argv_with(spec: &JobSpec, new_session: bool, status: bool) -> Vec<String> {
    let mut a = vec![spec.bwrap.clone()];
    for f in FLAGS {
        if !new_session && *f == "--new-session" {
            continue;
        }
        a.push((*f).to_owned());
    }
    if spec.net.is_some() {
        a.push("--unshare-net".into());
    }
    for m in &spec.mounts {
        push_mount(&mut a, m);
    }
    a.push("--chdir".into());
    a.push(spec.cwd.clone());
    for (k, v) in &spec.env {
        a.extend(["--setenv".to_owned(), k.clone(), v.clone()]);
    }
    if status {
        a.extend(["--json-status-fd".to_owned(), STATUS_FD.to_string()]);
    }
    a.push("--".into());
    a.push(spec.mork_bin.clone());
    a.push("wall-exec".into());
    if spec.wall == WallMode::Partial {
        a.push("--partial".into());
    }
    if let Some(net) = &spec.net {
        a.push("--bridge".into());
        a.extend(net.host_ports.iter().map(u16::to_string));
    } else {
        match &spec.ports {
            Some(ports) => a.extend(ports.iter().map(u16::to_string)),
            None => a.push("--any-port".into()),
        }
    }
    a.push("--".into());
    a.extend(spec.argv.iter().cloned());
    a
}

/// The folders a wall's mounts are built from.
#[derive(Debug, Clone, Default)]
pub struct WallPlaces {
    /// The host's top-level entries and the wall's own live folders, as [`system`] supplies them.
    pub system: Vec<Mount>,
    /// Mork home: an empty tmpfs under the binds, made read-only after them, so the folders holding a bind's
    /// mount point can't be written even where the home lies under the wall's own `/tmp` (1.4.2.2).
    pub sealed: Option<String>,
    /// The app's home, temporary and runtime folders, hidden beneath the explicit binds.
    pub hidden: Vec<String>,
    /// Paths shown empty after every bind, including aliases such as `/data`.
    pub masked: Vec<HiddenPath>,
    /// A regular empty file owned by the engine, used by the file masks.
    pub empty_file: String,
    pub agent_home: String,
    pub caches: Vec<(String, String)>,
    /// The installed browsers (fac-114), bound read-only when the folder exists: outside the
    /// writable homes, so no seat changes a binary the others launch.
    pub browsers: Option<String>,
    pub run_dir: String,
    /// Read-only: toolchains, the running binary's folder, `builds/`, the checks folder.
    pub read_only: Vec<String>,
    /// A peer's clone may disappear between enumeration and bwrap startup.
    pub peer_clones: Vec<String>,
    /// Read-write: the job's `io/`, the clone.
    pub read_write: Vec<String>,
    /// The seat's credential set, bound at `/run/credentials`.
    pub credentials: Option<String>,
    /// A turn's skills, bound read-only where they are and at [`SKILLS_AT`] for CLI discovery.
    pub skills: Option<String>,
    /// A closing turn's working folder (fac-147): an empty read-only tmpfs where its session ran, so
    /// nothing of a clone there is in the wall, gone or not.
    pub empty: Option<String>,
    /// A card's project's data folder (fac-173), bound read-only at [`DATA_AT`] when it exists.
    pub data: Option<String>,
    /// Files shown empty and read-only (fac-389), each bound from an empty file (source, then destination): a
    /// walled Codex turn's user config, which its app-server would read.
    pub blanked: Vec<(String, String)>,
    /// The operator's Claude Code sessions folder (fac-265), bound read-only under the waker home's `.claude`:
    /// only the orchestrator waker's wall has it.
    pub sessions: Option<String>,
    /// The operator's Claude Code sockets folder (fac-265), bound read-only where it is; the waker's alone.
    pub sockets: Option<String>,
}

/// Where the waker sees Claude Code's sessions, relative to its HOME.
pub const SESSIONS_AT: &str = ".claude/sessions";

/// Where a wall holds its project's data folder.
pub const DATA_AT: &str = "/data";

/// Where Codex and agy find the installed skills, relative to the seat's HOME.
pub const SKILLS_AT: &str = ".agents/skills";

/// Prepares the skills mount point without following links in the seat's home.
pub fn skills_at(agent_home: &Path) -> Result<(), String> {
    crate::seat_homes::mkdir(agent_home, Path::new(SKILLS_AT))
        .map_err(|e| format!("{}: {e}", agent_home.join(SKILLS_AT).display()))
}

/// The variable Playwright and Mork's manuals find the installed browsers by (fac-114).
pub const BROWSERS_ENV: &str = "PLAYWRIGHT_BROWSERS_PATH";

/// The resolver's file, the one the wall's DNS needs (1.4.11.1).
pub const RESOLVER: &str = "/run/systemd/resolve/stub-resolv.conf";

/// The host's top-level entries read-only, private live folders, and the resolver's file under `/run`.
pub fn system() -> std::io::Result<Vec<Mount>> {
    // The private root must accept destinations such as /data before it is sealed (fac-471).
    let mut entries: Vec<_> = std::fs::read_dir("/")?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut mounts = Vec::new();
    for entry in entries {
        if ["proc", "dev", "tmp", "run"].iter().any(|name| entry.file_name() == *name) {
            continue;
        }
        let path = entry.path().display().to_string();
        if entry.file_type()?.is_symlink() {
            let target = std::fs::read_link(entry.path())?.display().to_string();
            mounts.push(Mount::Symlink { target, path });
        } else {
            mounts.push(Mount::Ro { path });
        }
    }
    mounts.extend([
        Mount::Proc { path: "/proc".into() },
        Mount::Dev { path: "/dev".into() },
        Mount::Tmpfs { path: "/tmp".into() },
        Mount::Tmpfs { path: "/var/tmp".into() },
        Mount::Tmpfs { path: "/run".into() },
        Mount::RoTry { path: RESOLVER.into() },
    ]);
    Ok(mounts)
}

fn binds(p: &WallPlaces) -> Vec<Mount> {
    let mut binds = vec![Mount::Rw { path: p.agent_home.clone() }, Mount::Ro { path: p.run_dir.clone() }];
    binds.extend(p.read_only.iter().map(|r| Mount::Ro { path: r.clone() }));
    binds.extend(p.peer_clones.iter().map(|r| Mount::RoTry { path: r.clone() }));
    binds.extend(p.read_write.iter().map(|r| Mount::Rw { path: r.clone() }));
    binds.extend(p.skills.iter().map(|s| Mount::Ro { path: s.clone() }));
    binds.sort_by_key(|b| match b {
        Mount::Ro { path } | Mount::RoTry { path } | Mount::Rw { path } => path.len(),
        Mount::RwAt { .. }
        | Mount::RoAt { .. }
        | Mount::RoTryAt { .. }
        | Mount::Tmpfs { .. }
        | Mount::Dev { .. }
        | Mount::Proc { .. }
        | Mount::Symlink { .. }
        | Mount::RemountRo { .. } => 0,
    });
    binds.dedup();
    binds
}

fn hidden_folders(p: &WallPlaces) -> Vec<String> {
    let mut paths = p.hidden.clone();
    paths.extend(p.sealed.iter().cloned());
    paths.sort_by_key(|p| (p.len(), p.clone()));
    paths.dedup();
    paths
}

fn source_dest(mount: &Mount) -> Option<(&str, &str)> {
    match mount {
        Mount::Ro { path } | Mount::Rw { path } | Mount::RoTry { path } => Some((path, path)),
        Mount::RwAt { src, dest } | Mount::RoAt { src, dest } | Mount::RoTryAt { src, dest } => Some((src, dest)),
        _ => None,
    }
}

fn mask_destinations(p: &WallPlaces, mounts: &[Mount]) -> Vec<HiddenPath> {
    let sources: Vec<_> = mounts
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            source_dest(m).map(|(src, dest)| {
                (i, std::fs::canonicalize(src).unwrap_or_else(|_| PathBuf::from(src)), Path::new(dest))
            })
        })
        .collect();
    let mut paths = Vec::new();
    for hidden in &p.masked {
        for (i, src, dest) in &sources {
            if let Ok(relative) = hidden.path.strip_prefix(src) {
                let path = dest.join(relative);
                // A later tmpfs already hides the source. Making a mask there would reveal its ancestors.
                if !mounts[i + 1..].iter().any(|m| matches!(m,Mount::Tmpfs {path: at} if path.starts_with(at))) {
                    paths.push(HiddenPath { path, directory: hidden.directory });
                }
            } else if hidden.directory && src.starts_with(&hidden.path) {
                paths.push(HiddenPath { path: dest.to_path_buf(), directory: src.is_dir() });
            }
        }
    }
    paths.sort_by_key(|p| (p.path.as_os_str().len(), p.path.clone()));
    let mut kept: Vec<HiddenPath> = Vec::new();
    for path in paths {
        if !kept.iter().any(|p| p.path == path.path || (p.directory && path.path.starts_with(&p.path))) {
            kept.push(path);
        }
    }
    kept
}

fn masks(p: &WallPlaces, mounts: &[Mount]) -> Vec<Mount> {
    let mut masks = Vec::new();
    for hidden in mask_destinations(p, mounts) {
        let path = hidden.path.display().to_string();
        if hidden.directory {
            masks.extend([Mount::Tmpfs { path: path.clone() }, Mount::RemountRo { path }]);
        } else {
            masks.push(Mount::RoAt { src: p.empty_file.clone(), dest: path });
        }
    }
    masks
}

/// The host entries, private folders, binds and secret masks, with writable child binds preserved.
pub fn mounts(p: &WallPlaces) -> Vec<Mount> {
    let mut m = p.system.clone();
    let hidden = hidden_folders(p);
    m.extend(hidden.iter().map(|path| Mount::Tmpfs { path: path.clone() }));
    m.extend(binds(p));
    m.extend(p.caches.iter().map(|(src, dest)| Mount::RwAt { src: src.clone(), dest: dest.clone() }));
    m.extend(p.browsers.iter().map(|b| Mount::RoTry { path: b.clone() }));
    // The skills must be mounted after the seat's home so the CLI sees them.
    if let Some(s) = &p.skills {
        m.push(Mount::RoAt { src: s.clone(), dest: format!("{}/{SKILLS_AT}", p.agent_home) });
    }
    m.extend(p.blanked.iter().map(|(src, dest)| Mount::RoAt { src: src.clone(), dest: dest.clone() }));
    if let Some(c) = &p.credentials {
        m.push(Mount::RoAt { src: c.clone(), dest: "/run/credentials".into() });
    }
    if let Some(d) = &p.data {
        m.push(Mount::RoTryAt { src: d.clone(), dest: DATA_AT.into() });
    }
    if let Some(s) = &p.sessions {
        m.push(Mount::RoTryAt { src: s.clone(), dest: format!("{}/{SESSIONS_AT}", p.agent_home) });
    }
    if let Some(k) = &p.sockets {
        m.push(Mount::RoTryAt { src: k.clone(), dest: k.clone() });
    }
    m.extend(masks(p, &m));
    if let Some(e) = &p.empty {
        m.extend([Mount::Tmpfs { path: e.clone() }, Mount::RemountRo { path: e.clone() }]);
    }
    m.extend(hidden.into_iter().map(|path| Mount::RemountRo { path }));
    m.push(Mount::RemountRo { path: "/".into() });
    m
}
