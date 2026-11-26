// A review clone's whole `.git/mork/` survives replacement, its proofs reaching our fixer (fac-276, fac-601).
// Agent-written paths are reached beneath their clones without following symlinks (R9).

use std::fs::{File, Permissions};
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use mork_core::decide::Event;
use mork_core::reviews::verdict::{proofs_copy, proofs_round};
use mork_types::CardId;

use super::Engine;
use super::work::fresh_dir;

// What a review clone keeps through its rounds, its proofs among it.
const KEPT: &str = ".git/mork";

// What a review clone's proofs can make the engine read: folders deep, entries, and bytes in all.
const DEPTH: usize = 8;
const ENTRIES: usize = 1000;
const BYTES: u64 = 64 << 20;

impl Engine {
    /// Copies the round's own proofs folder to its fixer's clone, and decides what came of it (fac-377).
    pub(super) fn copy_proofs(&mut self, review: &CardId, card: &CardId, round: u32) {
        let (from, to) = (self.clone_dir(review.as_str()), self.clone_dir(card.as_str()));
        let copied = gather_round(&from, round).and_then(|found| {
            place(&to, &proofs_copy(round), &found)?;
            Ok(u32::try_from(found.len()).unwrap_or(u32::MAX))
        });
        let result = copied.map_err(|e| e.to_string());
        if let Err(e) = &result {
            self.log(&format!("{review}'s round {round} proofs weren't copied to {card}: {e}"));
        }
        self.apply(&Event::ProofsCopied { review: review.clone(), round, result });
    }
}

/// A review clone's folder, made empty but for its `.git/mork/`, its proofs and handoff among it, which
/// its seats find again in a later round.
pub(super) fn fresh_review_dir(home: &crate::paths::Home, card: &CardId, dir: &Path) -> Result<(), String> {
    // The review foreman's handoff goes on to the card's final copy, through every round (fac-287): copied
    // aside before its folder moves, so a put-back cut short still leaves it where the final copy looks.
    super::handoff::set_aside_handoff(home, card, dir)?;
    let aside = home.proofs().join(card.as_str());
    set_aside(dir, &aside).map_err(|e| format!("{card}'s .git/mork wasn't set aside: {e}"))?;
    fresh_dir(dir)?;
    // Git initializes over this directory, so restoring the folder need not wait for the clone job.
    put_back(dir, &aside).map_err(|e| format!("{card}'s .git/mork wasn't put back: {e}"))?;
    // The folder brought the handoff back whole, links and folders included: its copy aside is spent.
    super::handoff::drop_aside(&home.handoffs().join(card.as_str())).map_err(|e| format!("{card}'s handoff: {e}"))
}

/// Removes the clone, keeping its whole `.git/mork/` for a review a later head reopens (fac-416).
/// Failed moves leave the folder at its last name for the next try.
pub(super) fn remove_clone(dir: &Path, aside: &Path, keep: bool) -> io::Result<()> {
    let kept = keep && set_aside(dir, aside)?;
    std::fs::remove_dir_all(dir).or_else(absent)?;
    if !keep {
        return std::fs::remove_dir_all(aside).or_else(absent);
    }
    if kept {
        std::fs::create_dir(dir)?;
        put_back(dir, aside)?;
    }
    Ok(())
}

fn set_aside(dir: &Path, aside: &Path) -> io::Result<bool> {
    let (base, name) = location(aside)?;
    let (saved, present) = (folder_at(base, name)?, folder_at(dir, Path::new(KEPT))?);
    if saved && present {
        return Err(io::Error::other(format!("both {} and {} exist", aside.display(), dir.join(KEPT).display())));
    }
    if present {
        let (root, rel) = location(base)?;
        mork_sys::mkdir_beneath(root, rel)?;
        mork_sys::move_dir_beneath(dir, Path::new(KEPT), base, name)?;
    }
    Ok(saved || present)
}

fn put_back(dir: &Path, aside: &Path) -> io::Result<()> {
    let (base, name) = location(aside)?;
    if !folder_at(base, name)? {
        return Ok(());
    }
    mork_sys::mkdir_beneath(dir, Path::new(".git"))?;
    mork_sys::move_dir_beneath(base, name, dir, Path::new(KEPT))
}

fn folder_at(root: &Path, rel: &Path) -> io::Result<bool> {
    match mork_sys::open_beneath(root, rel) {
        Ok(dir) if dir.metadata()?.is_dir() => Ok(true),
        Ok(_) => Err(io::Error::other(format!("{} isn't a folder", root.join(rel).display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn location(path: &Path) -> io::Result<(&Path, &Path)> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "no folder name");
    Ok((path.parent().ok_or_else(invalid)?, Path::new(path.file_name().ok_or_else(invalid)?)))
}

fn absent(e: io::Error) -> io::Result<()> {
    if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) }
}

struct Proof {
    rel: PathBuf,
    bytes: Vec<u8>,
    exec: bool,
}

// What reading a proofs folder leaves: the files found, what is left to read, and the first thing that kept
// a file or folder from being read whole.
struct Scan<'a> {
    from: &'a Path,
    root: PathBuf,
    entries: usize,
    bytes: u64,
    problem: Option<String>,
    found: Vec<Proof>,
}

// One round's proofs alone, whole or an error: a missing folder is none saved, a file unread a failure.
// Its folder spends a level and an entry, keeping the copy's bounds as they were (fac-416).
fn gather_round(from: &Path, round: u32) -> io::Result<Vec<Proof>> {
    let scan = gather_in(from, Path::new(&proofs_round(round)), (DEPTH - 1, ENTRIES - 1))?;
    match scan.problem {
        Some(why) => Err(io::Error::other(why)),
        None => Ok(scan.found),
    }
}

fn gather_in<'a>(from: &'a Path, root: &Path, (depth, entries): (usize, usize)) -> io::Result<Scan<'a>> {
    let mut scan = Scan { from, root: root.to_path_buf(), entries, bytes: BYTES, problem: None, found: Vec::new() };
    let dir = match mork_sys::open_beneath(from, root) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(scan),
        Err(e) => return Err(e),
    };
    if dir.metadata()?.is_dir() {
        scan.walk(&dir, Path::new(""), depth);
    } else {
        scan.problem = Some(format!("{} isn't a folder", root.display()));
    }
    Ok(scan)
}

fn place(to: &Path, dest: &str, found: &[Proof]) -> io::Result<()> {
    let dest = PathBuf::from(dest);
    for p in found {
        let file = dest.join(&p.rel);
        // Each folder below `.git`, never `.git` itself: our clone's is only reached.
        let mut folders: Vec<&Path> = file.ancestors().skip(1).filter(|a| a.components().count() > 1).collect();
        folders.reverse();
        for folder in folders {
            mork_sys::mkdir_beneath(to, folder)?;
        }
        mork_sys::write_beneath(to, &file, &p.bytes)?;
        if p.exec {
            mork_sys::open_beneath(to, &file)?.set_permissions(Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

impl Scan<'_> {
    fn fail(&mut self, why: String) {
        self.problem.get_or_insert(why);
    }

    // The regular files in the open folder `dir`, at `rel` under the root, down to `depth` folders:
    // each folder read through its descriptor, a symlink or anything else skipped.
    fn walk(&mut self, dir: &File, rel: &Path, depth: usize) {
        let entries = match mork_sys::read_dir_of(dir) {
            Ok(entries) => entries,
            Err(e) => return self.fail(format!("{} couldn't be listed: {e}", rel.display())),
        };
        for e in entries {
            let Some(left) = self.entries.checked_sub(1) else {
                return self.fail(format!("more than {ENTRIES} entries"));
            };
            self.entries = left;
            let Ok(e) = e else { return self.fail(format!("{} couldn't be listed", rel.display())) };
            let rel = rel.join(e.file_name());
            let Ok(meta) = e.metadata() else {
                self.fail(format!("{} couldn't be read", rel.display()));
                continue;
            };
            if meta.is_dir() {
                self.enter(dir, &rel, &e.file_name(), depth);
            } else if meta.is_file() {
                self.read(rel);
            }
        }
    }

    fn enter(&mut self, dir: &File, rel: &Path, name: &std::ffi::OsStr, depth: usize) {
        if depth <= 1 {
            return self.fail(format!("{} is nested deeper than {DEPTH} folders", rel.display()));
        }
        match mork_sys::open_dir_beneath(dir, Path::new(name)) {
            Ok(sub) => self.walk(&sub, rel, depth - 1),
            Err(e) => self.fail(format!("{} couldn't be opened: {e}", rel.display())),
        }
    }

    // The file as it is when opened, still a regular file and within the bytes left.
    fn read(&mut self, rel: PathBuf) {
        let file = match mork_sys::open_beneath(self.from, &self.root.join(&rel)) {
            Ok(file) => file,
            Err(e) => return self.fail(format!("{} couldn't be opened: {e}", rel.display())),
        };
        let Some(meta) = file.metadata().ok().filter(|m| m.is_file()) else {
            return self.fail(format!("{} changed as it was read", rel.display()));
        };
        if meta.len() > self.bytes {
            return self.fail(format!("more than {BYTES} bytes, at {}", rel.display()));
        }
        let mut bytes = Vec::new();
        if let Err(e) = file.take(meta.len()).read_to_end(&mut bytes) {
            return self.fail(format!("{} couldn't be read: {e}", rel.display()));
        }
        self.bytes = self.bytes.saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        self.found.push(Proof { rel, bytes, exec: meta.permissions().mode() & 0o111 != 0 });
    }
}
