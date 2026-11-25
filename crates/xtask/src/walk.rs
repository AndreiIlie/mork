//! Walking the workspace's sources.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, sorted, skipping `target/`.
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    files(dir, "rs")
}

/// Every file under `dir` with the extension `ext`, sorted, skipping `target/`, `.git/` and `node_modules/`.
pub fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, ext, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == "target" || n == ".git" || n == "node_modules") {
            continue;
        }
        if p.is_dir() {
            collect(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

/// Whether a file is test code: under a crate's `tests/` folder.
pub fn is_test_file(rel: &Path) -> bool {
    rel.components().any(|c| c.as_os_str() == "tests")
}

/// A file's lines without its `#[cfg(test)] mod …` modules: an attribute counts only when
/// the next code line opens a module, and the module is skipped to its closing brace.
pub fn non_test_lines(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(l) = lines.get(i) {
        if l.trim_start().starts_with("#[cfg(test)]") {
            let next = lines[i + 1..].iter().position(|n| !n.trim().is_empty() && !n.trim_start().starts_with("//"));
            if let Some(off) = next {
                let at = i + 1 + off;
                let opens = lines[at].trim_start();
                let opens = opens.strip_prefix("pub ").unwrap_or(opens);
                if opens.starts_with("mod ") && opens.trim_end().ends_with('{') {
                    i = skip_block(&lines, at);
                    continue;
                }
            }
        }
        out.push(*l);
        i += 1;
    }
    out
}

fn skip_block(lines: &[&str], at: usize) -> usize {
    let mut depth: usize = 0;
    for (k, l) in lines.iter().enumerate().skip(at) {
        depth += l.matches('{').count();
        depth = depth.saturating_sub(l.matches('}').count());
        if depth == 0 {
            return k + 1;
        }
    }
    lines.len()
}

/// The path relative to the root, as a string with `/`.
pub fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}
