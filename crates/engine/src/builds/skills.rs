//! Each binary keeps its own skill content so a reload cannot change the files
//! still mounted by running turns (fac-88).

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/skills.rs"));

/// The running binary's skills, written out when they aren't yet: their folder, or none when
/// the binary has none.
pub fn materialize(home: &crate::paths::Home) -> std::io::Result<Option<PathBuf>> {
    write_out(&home.root().join("skills"), FILES)
}

/// Writes `files` into a folder named for their content under `root`, unless it is there.
pub fn write_out(root: &Path, files: &[(&str, &[u8])]) -> std::io::Result<Option<PathBuf>> {
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
