// Reads .git without running git; MORK_BUILD_COMMIT, set by a `mork build` (1.8.3), wins.
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=MORK_BUILD_COMMIT");
    let (commit, dirty) = match std::env::var("MORK_BUILD_COMMIT") {
        Ok(c) if c.len() == 40 => (c, false),
        _ => (head_commit().unwrap_or_else(|| "0".repeat(40)), true),
    };
    println!("cargo:rustc-env=MORK_COMMIT={commit}");
    println!("cargo:rustc-env=MORK_DIRTY={dirty}");
}

fn head_commit() -> Option<String> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    let git = manifest.ancestors().map(|a| a.join(".git")).find(|g| g.is_dir())?;
    let head_path = git.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());
    let head = fs::read_to_string(&head_path).ok()?;
    let head = head.trim();
    let Some(reference) = head.strip_prefix("ref: ") else { return Some(head.to_owned()) };
    let loose = git.join(reference);
    println!("cargo:rerun-if-changed={}", loose.display());
    if let Ok(c) = fs::read_to_string(&loose) {
        return Some(c.trim().to_owned());
    }
    packed(&git, reference)
}

fn packed(git: &Path, reference: &str) -> Option<String> {
    let text = fs::read_to_string(git.join("packed-refs")).ok()?;
    text.lines().find_map(|l| {
        let (sha, name) = l.split_once(' ')?;
        (name == reference).then(|| sha.to_owned())
    })
}
