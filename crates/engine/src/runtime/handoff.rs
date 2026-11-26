// A seat wrote the clone (fac-287): each folder and file is read beneath it through no symlink (R9), and anything
// but a folder or a regular file stays. Copied into the records before the clone goes; kept until its note is deleted.

use std::fs::{File, Permissions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use mork_core::memory::HANDOFF;
use mork_types::note::HandoffFile;
use mork_types::{CardId, ProjectId};
use serde::{Deserialize, Serialize};

use super::Engine;

impl Engine {
    pub(super) fn remove_handoff(&self, project: &ProjectId, dir: &str) {
        let dir = self.opts.home.records().join(project.as_str()).join(dir);
        if let Err(e) = unrecord(&dir) {
            self.log(&format!("{} wasn't removed: {e}", dir.display()));
        }
    }
}

/// A review clone's handoff set aside before its clone is made again. A copy already aside is whole and
/// stays until its clone has its `.git/mork/` back or it is filed: only a file the clone has that it hasn't is added.
pub(super) fn set_aside_handoff(home: &crate::paths::Home, card: &CardId, clone: &Path) -> Result<(), String> {
    let aside = home.handoffs().join(card.as_str());
    set_aside(&aside, clone).map(|_| ()).map_err(|e| format!("{card}'s handoff wasn't set aside: {e}"))
}

// The clone's handoff set aside, as its copy there. One already recorded stays, and the clone's
// files it lacks are added to it. None when nothing is recorded aside.
fn set_aside(aside: &Path, clone: &Path) -> io::Result<Option<Copied>> {
    if recorded(aside)?.is_none() {
        return preserve(clone, aside).and_then(|_| recorded(aside));
    }
    let more = aside.with_file_name(format!("{}.more", aside.file_name().unwrap_or_default().to_string_lossy()));
    let merged = preserve(clone, &more).and_then(|seen| seen.map_or(Ok(()), |m| add_to(aside, &more, m)));
    let _ = unrecord(&more);
    merged?;
    recorded(aside)
}

/// A handoff still aside as the clone goes (its put-back never finished) is the card's whole one,
/// made the copy `to`, with what the clone still holds beyond it. Kept aside until `drop_aside`.
pub fn settle_aside(aside: &Path, clone: &Path, to: &Path) -> io::Result<Option<Copied>> {
    if recorded(aside)?.is_none() {
        return Ok(None);
    }
    // The clone may already be gone: only a folder there adds to the copy.
    let kept = if clone.exists() { set_aside(aside, clone)? } else { recorded(aside)? };
    let Some(kept) = kept else { return Ok(None) };
    unrecord(to)?;
    for f in &kept.files {
        let dest = to.join(&f.path);
        std::fs::create_dir_all(dest.parent().unwrap_or(to))?;
        std::fs::copy(aside.join(&f.path), dest)?;
    }
    crate::files::write_whole(&manifest(to), &serde_json::to_vec(&kept).map_err(io::Error::other)?)?;
    Ok(Some(kept))
}

/// The copy aside removed once its handoff is filed.
pub fn drop_aside(aside: &Path) -> io::Result<()> {
    unrecord(aside)
}

// A copy's record goes before its files: cut short between, what is left is no copy, never a
// record of files that are gone.
pub(super) fn unrecord(to: &Path) -> io::Result<()> {
    std::fs::remove_file(manifest(to)).or_else(absent)?;
    std::fs::remove_dir_all(to).or_else(absent)
}

// The files of `more` that `aside`'s copy lacks, moved into it and recorded; its own stay.
fn add_to(aside: &Path, more: &Path, seen: Copied) -> io::Result<()> {
    let Some(mut kept) = recorded(aside)? else { return Ok(()) };
    let lacking: Vec<HandoffFile> =
        seen.files.into_iter().filter(|f| !kept.files.iter().any(|k| k.path == f.path)).collect();
    if lacking.is_empty() {
        return Ok(());
    }
    for f in &lacking {
        let to = aside.join(&f.path);
        std::fs::create_dir_all(to.parent().unwrap_or(aside))?;
        std::fs::rename(more.join(&f.path), to)?;
    }
    kept.files.extend(lacking);
    kept.files.sort_by(|a, b| a.path.cmp(&b.path));
    crate::files::write_whole(&manifest(aside), &serde_json::to_vec(&kept).map_err(io::Error::other)?)
}

/// Each file of `kept` written beneath the clone's `folder` (one in `.git`) through no link, with the permission
/// bits it was kept with.
pub(super) fn put_back(clone: &Path, folder: &str, aside: &Path, kept: &Copied) -> io::Result<()> {
    mork_sys::mkdir_beneath(clone, Path::new(".git"))?;
    for f in &kept.files {
        let file = Path::new(folder).join(&f.path);
        let mut folders: Vec<&Path> = file.ancestors().skip(1).filter(|a| a.components().count() > 1).collect();
        folders.reverse();
        for folder in folders {
            mork_sys::mkdir_beneath(clone, folder)?;
        }
        let from = aside.join(&f.path);
        mork_sys::write_beneath(clone, &file, &std::fs::read(&from)?)?;
        let mode = std::fs::metadata(&from)?.permissions().mode() & 0o777;
        mork_sys::open_beneath(clone, &file)?.set_permissions(Permissions::from_mode(mode))?;
    }
    Ok(())
}

fn absent(e: io::Error) -> io::Result<()> {
    if e.kind() == ErrorKind::NotFound { Ok(()) } else { Err(e) }
}

/// What a handoff's copy kept: its files, by their path under the handoff, and what it left
/// behind, a link or anything else no regular file.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Copied {
    /// Each regular file copied, by its path under the handoff.
    pub files: Vec<HandoffFile>,
    /// Each path under the handoff not copied.
    pub skipped: Vec<String>,
}

/// Where a whole copy into `to` is recorded: written once the copy is done, before the clone's
/// removal starts, so a removal cut short and run again still knows what was kept.
pub fn manifest(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(".json");
    to.with_file_name(name)
}

/// The copy recorded for `to`, if one was made whole and each of its files is still there as
/// recorded: a record left by a removal cut short, before records went first, is no copy.
pub fn recorded(to: &Path) -> io::Result<Option<Copied>> {
    let copied: Copied = match std::fs::read(manifest(to)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    for f in &copied.files {
        match std::fs::symlink_metadata(to.join(&f.path)) {
            Ok(m) if m.is_file() && m.len() == f.bytes => {}
            Ok(_) => return Ok(None),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        }
    }
    Ok(Some(copied))
}

// What a seat made that is no folder or file of its handoff: gone, a link, or a file in a folder's
// place. Any other failure to read it fails the copy, and the clone stays.
fn not_there(e: &io::Error) -> bool {
    e.kind() == ErrorKind::NotFound || mork_sys::not_followed(e)
}

/// Copies the clone's handoff into `to`, made fresh, and records it whole; a handoff with no
/// regular file in it copies and records nothing.
pub fn preserve(clone: &Path, to: &Path) -> io::Result<Option<Copied>> {
    preserve_folder(clone, HANDOFF, to)
}

/// `preserve` of the clone's `folder`: none there keeps an older copy in `to`, an empty one drops it.
pub fn preserve_folder(clone: &Path, folder: &str, to: &Path) -> io::Result<Option<Copied>> {
    let root = match mork_sys::open_beneath(clone, Path::new(folder)) {
        Ok(root) => root,
        Err(e) if not_there(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    if !root.metadata()?.is_dir() {
        return Ok(None);
    }
    unrecord(to)?;
    let mut copied = Copied::default();
    // Folders as a list, not by recursion: no handoff's depth reaches the engine's stack.
    let mut folders = vec![(root, PathBuf::new())];
    while let Some((dir, rel)) = folders.pop() {
        for e in mork_sys::read_dir_of(&dir)? {
            let e = e?;
            let rel = rel.join(e.file_name());
            let kind = e.file_type()?;
            let kept = if kind.is_dir() {
                // A folder swapped for a link since it was listed opens no more, and is left as one.
                match mork_sys::open_dir_beneath(&dir, Path::new(&e.file_name())) {
                    Ok(sub) => {
                        folders.push((sub, rel.clone()));
                        true
                    }
                    Err(e) if not_there(&e) => false,
                    Err(e) => return Err(e),
                }
            } else {
                kind.is_file() && copy(clone, &Path::new(folder).join(&rel), &rel, to, &mut copied)?
            };
            if !kept {
                copied.skipped.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    if copied.files.is_empty() {
        return Ok(None);
    }
    copied.files.sort_by(|a, b| a.path.cmp(&b.path));
    copied.skipped.sort();
    crate::files::write_whole(&manifest(to), &serde_json::to_vec(&copied).map_err(io::Error::other)?)?;
    Ok(Some(copied))
}

// The file as it is when opened, if still a regular one, streamed into the records.
fn copy(clone: &Path, at: &Path, rel: &Path, to: &Path, copied: &mut Copied) -> io::Result<bool> {
    let mut from = match mork_sys::open_beneath(clone, at) {
        Ok(f) => f,
        Err(e) if not_there(&e) => return Ok(false),
        Err(e) => return Err(e),
    };
    let meta = from.metadata()?;
    if !meta.is_file() {
        return Ok(false);
    }
    let dest = to.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut to_file = File::create_new(&dest)?;
    let bytes = io::copy(&mut from, &mut to_file)?;
    // The usable bits only, so a copy runs as its source did: setuid, setgid and sticky aren't carried.
    to_file.set_permissions(Permissions::from_mode(meta.permissions().mode() & 0o777))?;
    copied.files.push(HandoffFile { path: rel.to_string_lossy().into_owned(), bytes });
    Ok(true)
}
