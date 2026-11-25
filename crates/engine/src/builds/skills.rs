//! Each binary keeps its own skill content so a reload cannot change the files
//! still mounted by running turns (fac-88).

use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/skills.rs"));

/// The running binary's skills plus any configured extra skills folder (fac-721), written out when
/// they aren't yet: their folder, or none when there are none.
pub fn materialize(home: &crate::paths::Home, extra: Option<&Path>) -> io::Result<Option<PathBuf>> {
    materialize_into(&home.root().join("skills"), extra)
}

/// Writes the built-in skills and any configured `extra` folder into a content-named folder under `root`.
pub fn materialize_into(root: &Path, extra: Option<&Path>) -> io::Result<Option<PathBuf>> {
    let added = match extra {
        Some(dir) => read_extra(dir)?,
        None => Vec::new(),
    };
    if added.is_empty() {
        return write_out(root, FILES);
    }
    let mut merged: Vec<(&str, &[u8])> = FILES.to_vec();
    for (rel, bytes) in &added {
        merged.push((rel.as_str(), bytes.as_slice()));
    }
    merged.sort_by(|a, b| a.0.cmp(b.0));
    write_out(root, &merged)
}

// Reads regular files under a configured skills root without following any symlink; a missing root
// adds nothing, while any other root error or built-in/.claude-plugin collision fails startup.
fn read_extra(root: &Path) -> io::Result<Vec<(String, Vec<u8>)>> {
    let dir = match factory_sys::open_no_symlinks(root) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    if !dir.metadata()?.is_dir() {
        return Err(io::Error::other(format!("{} is not a directory", root.display())));
    }
    let mut out = Vec::new();
    let mut folders = vec![(dir, PathBuf::new())];
    while let Some((cur, rel)) = folders.pop() {
        walk_dir(root, &cur, &rel, &mut folders, &mut out)?;
    }
    Ok(out)
}

fn walk_dir(
    root: &Path,
    cur: &std::fs::File,
    rel: &Path,
    folders: &mut Vec<(std::fs::File, PathBuf)>,
    out: &mut Vec<(String, Vec<u8>)>,
) -> io::Result<()> {
    for entry in factory_sys::read_dir_of(cur)? {
        let entry = entry?;
        let name = entry.file_name();
        let next = rel.join(&name);
        let kind = entry.file_type()?;
        if !kind.is_dir() && !kind.is_file() {
            continue;
        }
        if next == Path::new(".claude-plugin") {
            return Err(io::Error::other("configured skill path `.claude-plugin` is reserved"));
        }
        if kind.is_dir() {
            match factory_sys::open_dir_beneath(cur, Path::new(&name)) {
                Ok(sub) => folders.push((sub, next)),
                Err(e) if factory_sys::not_followed(&e) || e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        } else {
            read_file(root, &next, out)?;
        }
    }
    Ok(())
}

fn read_file(root: &Path, rel: &Path, out: &mut Vec<(String, Vec<u8>)>) -> io::Result<()> {
    let key = rel.to_str().ok_or_else(|| io::Error::other("a configured skill path is not UTF-8"))?;
    if FILES.iter().any(|(built_in, _)| *built_in == key) {
        return Err(io::Error::other(format!("configured skill path `{key}` collides with a built-in skill")));
    }
    let mut file = match factory_sys::open_beneath_no_symlinks(root, rel) {
        Ok(f) => f,
        Err(e) if factory_sys::not_followed(&e) || e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !file.metadata()?.is_file() {
        return Ok(());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    out.push((key.to_owned(), bytes));
    Ok(())
}

/// Writes `files` into a folder named for their content under `root`, unless it is there.
pub fn write_out(root: &Path, files: &[(&str, &[u8])]) -> io::Result<Option<PathBuf>> {
    if files.is_empty() {
        return Ok(None);
    }
    let mut h = Sha256::new();
    for (rel, bytes) in files {
        h.update((rel.len() as u64).to_le_bytes());
        h.update(rel.as_bytes());
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    let hash: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
    let dir = root.join(&hash);
    if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
        return Ok(Some(dir));
    }
    // Whole or not at all: a crash leaves only the temporary folder, made again next time.
    let tmp = root.join(format!(".tmp-{hash}"));
    let _ = std::fs::remove_dir_all(&tmp);
    for (rel, bytes) in files {
        let path = tmp.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
        factory_sys::chmod(&path, 0o444)?;
    }
    std::fs::rename(&tmp, &dir)?;
    Ok(Some(dir))
}
