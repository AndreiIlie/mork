//! The door's file reads on this machine (fac-1): bounded, with no symlink followed anywhere on the path.
//! A remote target's are `door_ssh`'s `stat` and `dd`, their bytes shaped here alike.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use mork_types::door::FILE_GET_MAX;
use mork_types::door_plan::Ran;

fn failed(words: String) -> Ran {
    Ran { exit: Some(1), stderr: words, ..Ran::default() }
}

// No symlink at any part of the path, the root's own included; no `..`; a regular file only.
pub(super) fn read_beneath(root: &str, path: &str, max: usize) -> Ran {
    let f = match mork_sys::open_beneath_no_symlinks(Path::new(root), Path::new(path)) {
        Ok(f) => f,
        Err(e) => return failed(format!("{path}: {e}")),
    };
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return failed(format!("{path}: not a regular file"));
    }
    let mut buf = Vec::new();
    let _ = f.take(max as u64 + 1).read_to_end(&mut buf);
    let truncated = buf.len() > max;
    buf.truncate(max);
    Ran { exit: Some(0), stdout: String::from_utf8_lossy(&buf).into_owned(), truncated, ..Ran::default() }
}

// As `read_beneath`, the whole file copied to `into`; one over the cap is refused unread.
pub(super) fn get_beneath(root: &str, path: &str, into: &Path) -> Ran {
    let f = match mork_sys::open_beneath_no_symlinks(Path::new(root), Path::new(path)) {
        Ok(f) => f,
        Err(e) => return failed(format!("{path}: {e}")),
    };
    match f.metadata() {
        Ok(m) if !m.is_file() => return failed(format!("{path}: not a regular file")),
        Ok(m) if m.len() > FILE_GET_MAX => return over(path),
        Ok(_) => {}
        Err(e) => return failed(format!("{path}: {e}")),
    }
    let mut buf = Vec::new();
    if let Err(e) = f.take(FILE_GET_MAX + 1).read_to_end(&mut buf) {
        return failed(format!("{path}: {e}"));
    }
    deliver(path, &buf, into)
}

fn over(path: &str) -> Ran {
    failed(format!("{path}: over file.get's {} MB: refused, never cut", FILE_GET_MAX >> 20))
}

// A file's bytes, read whole, written to `into` as a new file: one there already, or a symlink
// planted in its place, is never written through. Answers where it went and its size.
pub(super) fn deliver(path: &str, bytes: &[u8], into: &Path) -> Ran {
    use std::os::unix::fs::OpenOptionsExt;
    // Read one byte past the cap: it grew past it as it was read.
    if bytes.len() as u64 > FILE_GET_MAX {
        return over(path);
    }
    // `create_new` is `O_EXCL`: a symlink at `into` fails the open, never followed.
    let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(into) {
        Ok(f) => f,
        Err(e) => return failed(format!("{path}: the copy can't be written to its destination: {e}")),
    };
    if let Err(e) = f.write_all(bytes) {
        let _ = std::fs::remove_file(into);
        return failed(format!("{path}: the copy can't be written to its destination: {e}"));
    }
    let answer = serde_json::json!({"file": into.display().to_string(), "size": bytes.len()});
    Ran { exit: Some(0), stdout: answer.to_string(), ..Ran::default() }
}

// Read from its end: at most `max` bytes, whatever the log's size. No symlink on its path is
// followed, its folders' included.
pub(super) fn tail(path: &str, lines: u64, max: usize) -> Ran {
    let mut f = match mork_sys::open_no_symlinks(Path::new(path)) {
        Ok(f) => f,
        Err(e) => return failed(format!("{path}: {e}")),
    };
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return failed(format!("{path}: not a regular file"));
    }
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(max as u64);
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(from)).is_ok() {
        let _ = f.take(max as u64).read_to_end(&mut buf);
    }
    last_lines(&buf, from > 0, lines)
}

/// A log's last `lines` lines, of the bytes read from its end; `inside`: read from inside it, so
/// its first line begins mid-line.
pub(super) fn last_lines(buf: &[u8], inside: bool, lines: u64) -> Ran {
    let text = String::from_utf8_lossy(buf).into_owned();
    let mut all: Vec<&str> = text.lines().collect();
    if inside && !all.is_empty() {
        all.remove(0);
    }
    let n = usize::try_from(lines).unwrap_or(usize::MAX);
    let keep = &all[all.len().saturating_sub(n)..];
    // Fewer lines than asked, with more before them: cut at the byte bound.
    let truncated = keep.len() < n && inside;
    Ran { exit: Some(0), stdout: keep.join("\n"), truncated, ..Ran::default() }
}
