//! Files written whole (1.3.1): to a temporary name in the same folder, flushed, renamed.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// A path whose contents must be replaced by an empty file or directory in a wall.
#[derive(Debug, Clone)]
pub struct HiddenPath {
    pub path: PathBuf,
    pub directory: bool,
}

/// Existing child directories, excluding symlinks; an absent parent has no children.
pub fn child_dirs(root: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Matching names below a trusted root, without descending through links, node_modules, .git or target.
/// Vanished paths are skipped; every other scan error is returned.
pub fn hidden_names(root: &Path, patterns: &[String]) -> io::Result<Vec<HiddenPath>> {
    let mut hidden = Vec::new();
    if patterns.is_empty() {
        return Ok(hidden);
    }
    let Some(root) = unless_missing(fs::canonicalize(root))? else { return Ok(hidden) };
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        let Some(entries) = unless_missing(fs::read_dir(dir))? else { continue };
        for entry in entries {
            let Some(entry) = unless_missing(entry)? else { continue };
            let name = entry.file_name();
            let Some(kind) = unless_missing(entry.file_type())? else { continue };
            let directory = kind.is_dir();
            if patterns.iter().any(|p| name_matches(p.as_bytes(), name.as_encoded_bytes())) {
                hidden.push(HiddenPath { path: entry.path(), directory });
            } else if directory && !["node_modules", ".git", "target"].iter().any(|n| name == *n) {
                pending.push(entry.path());
            }
        }
    }
    Ok(hidden)
}

fn unless_missing<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn name_matches(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n, mut star, mut retry) = (0, 0, None, 0);
    while n < name.len() {
        if pattern.get(p) == Some(&b'*') {
            star = Some(p);
            p += 1;
            retry = n;
        } else if pattern.get(p) == name.get(n) {
            p += 1;
            n += 1;
        } else if let Some(at) = star {
            p = at + 1;
            retry += 1;
            n = retry;
        } else {
            return false;
        }
    }
    while pattern.get(p) == Some(&b'*') {
        p += 1;
    }
    p == pattern.len()
}

pub fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_whole_with(path, bytes, |opts| opts)
}

/// As [`write_whole`], creating the temporary file with Unix permissions `mode` before writing.
pub fn write_whole_mode(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    write_whole_with(path, bytes, |opts| opts.mode(mode))
}

fn write_whole_with(
    path: &Path,
    bytes: &[u8],
    configure: impl Fn(&mut fs::OpenOptions) -> &mut fs::OpenOptions,
) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("no parent folder"))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let (tmp, mut f) = temporary(dir, name, |tmp| {
        let mut opts = fs::OpenOptions::new();
        configure(opts.write(true).create_new(true)).open(tmp)
    })?;
    let result = (|| {
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Replaces `path` atomically with a symlink to `target`'s absolute path.
pub fn link_whole(path: &Path, target: &Path) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("no parent folder"))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("link");
    let target = std::path::absolute(target)?;
    let (tmp, ()) = temporary(dir, name, |tmp| std::os::unix::fs::symlink(&target, tmp))?;
    let result = fs::rename(&tmp, path);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn temporary<T>(
    dir: &Path,
    name: &str,
    create: impl Fn(&Path) -> io::Result<T>,
) -> io::Result<(std::path::PathBuf, T)> {
    loop {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
        match create(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            // A crashed process can leave a temporary file behind before its pid is reused.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_whole(path, &bytes)
}

/// Reads a JSON file; `Ok(None)` when it doesn't exist.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).map(Some).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Rotates a log over `max` bytes: `<name>.1` to `<name>.<keep>`, the oldest dropped (1.3). A
/// lock beside it keeps two writers (the app and its supervisors) from rotating at once.
pub fn rotate(path: &Path, max: u64, keep: u32) {
    if !fs::metadata(path).is_ok_and(|m| m.len() > max) {
        return;
    }
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return };
    let dir = path.parent().unwrap_or(Path::new("."));
    let Ok(lock) =
        fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!(".{name}.lock")))
    else {
        return;
    };
    if lock.lock().is_err() {
        return;
    }
    // Another writer may have rotated it while this one waited.
    if fs::metadata(path).is_ok_and(|m| m.len() > max) {
        for i in (1..keep).rev() {
            let _ = fs::rename(dir.join(format!("{name}.{i}")), dir.join(format!("{name}.{}", i + 1)));
        }
        let _ = fs::rename(path, dir.join(format!("{name}.1")));
    }
    let _ = lock.unlock();
}
