//! Findings located in a clone (4.8.2): the file is there, reached without leaving the clone (a symlink out is
//! refused), isn't a folder, and the line exists (an empty file has line 1); `./a/../b.ts` is read as `b.ts`.

use std::path::{Component, Path, PathBuf};

fn normal(file: &str) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    for c in Path::new(file).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(format!("{file} leaves the clone"));
                }
            }
            Component::RootDir | Component::Prefix(_) => return Err(format!("{file} isn't a path inside the clone")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("the finding names no file".into());
    }
    Ok(out)
}

/// Locates one finding: its path as read, or why not.
pub fn locate(clone: &Path, file: &str, line: i64) -> Result<String, String> {
    let rel = normal(file)?;
    let root = clone.canonicalize().map_err(|e| format!("the clone: {e}"))?;
    let full = root.join(&rel);
    let real = full.canonicalize().map_err(|_| format!("{} isn't in the clone", rel.display()))?;
    if !real.starts_with(&root) {
        return Err(format!("{} leads out of the clone", rel.display()));
    }
    if real.is_dir() {
        return Err(format!("{} is a folder", rel.display()));
    }
    let text = std::fs::read(&real).map_err(|e| format!("{}: {e}", rel.display()))?;
    let lines = text.split(|b| *b == b'\n').count() - usize::from(text.ends_with(b"\n"));
    let lines = i64::try_from(lines.max(1)).unwrap_or(i64::MAX);
    if line < 1 || line > lines {
        return Err(format!("{}:{line} isn't a line: the file has {lines}", rel.display()));
    }
    Ok(rel.display().to_string())
}
