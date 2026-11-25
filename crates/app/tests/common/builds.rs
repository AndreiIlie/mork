//! The factory's own repo, for a rig whose cards build it.

use std::path::Path;

use super::Rig;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The factory's own repo with `n` commits on master; their hashes, oldest first.
pub fn factory_repo(rig: &Rig, n: usize) -> Vec<String> {
    let repo = rig.t.root.join("factory-repo");
    git(&repo, &["init", "-q", "-b", "master"]);
    git(&repo, &["config", "user.email", "t@example.invalid"]);
    git(&repo, &["config", "user.name", "T"]);
    (0..n)
        .map(|i| {
            std::fs::write(repo.join("file.txt"), format!("{i}")).unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-q", "-m", &format!("commit {i}")]);
            git(&repo, &["rev-parse", "HEAD"])
        })
        .collect()
}
