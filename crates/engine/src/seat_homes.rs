//! The engine prepares a private home for each seat and mounts only the shared leaves.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use mork_types::config::{Cli, MorkConfig};
use serde_json::Value;

use crate::jobs::wall::WallPlaces;
use crate::{login_files, paths::Home};

pub(crate) fn mkdir(root: &Path, rel: &Path) -> io::Result<()> {
    let mut path = PathBuf::new();
    for part in rel.components() {
        path.push(part);
        mork_sys::mkdir_beneath(root, &path)?;
    }
    Ok(())
}

pub(crate) fn git_user(author: &str) -> (&str, &str) {
    let author = author.trim();
    author.split_once('<').map_or((author, ""), |(n, e)| (n.trim(), e.trim_end_matches('>').trim()))
}

pub(crate) fn git_identity(author: &str) -> String {
    let (name, email) = git_user(author);
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!("[user]\n\tname = \"{}\"\n\temail = \"{}\"\n", escape(name), escape(email))
}

/// Applies the home after configured environment overrides, for each job that shares logins.
pub fn env(home: &Home, dir: &Path, vars: &mut BTreeMap<String, String>) {
    vars.remove("CODEX_HOME");
    vars.remove("CLAUDE_CONFIG_DIR");
    vars.insert("HOME".into(), dir.display().to_string());
    vars.insert("CLAUDE_SECURESTORAGE_CONFIG_DIR".into(), home.logins().join("claude").display().to_string());
}

/// Prepares the private CLI home and binds the credentials visible to a CLI job.
pub fn login_places(home: &Home, dir: &Path, cli: Cli, places: &mut WallPlaces) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let logins = home.logins();
    std::fs::create_dir_all(&logins)?;
    for name in ["claude", "codex", "agy"] {
        mork_sys::mkdir_beneath(&logins, Path::new(name))?;
    }
    let links: &[(&str, &str)] = match cli {
        Cli::Codex => &[(".codex/auth.json", login_files::CODEX), (".codex/auth.lock", "codex/auth.lock")],
        Cli::Agy => &[(crate::cli::agy::TOKEN, login_files::AGY)],
        Cli::Claude => &[],
    };
    for (local, shared) in links {
        let rel = Path::new(local);
        if let Some(parent) = rel.parent() {
            mkdir(dir, parent)?;
        }
        if dir == home.login_home() && *shared == login_files::CODEX && login_files::regular(dir, rel).is_ok() {
            continue;
        }
        mork_sys::link_beneath(dir, rel, &logins.join(shared))?;
    }
    places.agent_home = dir.display().to_string();
    places.read_write.push(logins.display().to_string());
    Ok(())
}

pub(crate) fn prepare(home: &Home, dir: &Path, cli: Cli, author: &str, places: &mut WallPlaces) -> io::Result<()> {
    login_places(home, dir, cli, places)?;
    mork_sys::write_beneath(dir, Path::new(".gitconfig"), git_identity(author).as_bytes())?;
    if cli == Cli::Claude {
        let rel = Path::new(".claude.json");
        let source = account_json(&home.login_home(), rel)?;
        let mut local = account_json(dir, rel)?;
        crate::claude_accounts::account_fields(&mut local, &source).map_err(io::Error::other)?;
        mork_sys::write_beneath(dir, rel, &serde_json::to_vec(&local)?)?;
    }
    Ok(())
}

fn account_json(dir: &Path, rel: &Path) -> io::Result<Value> {
    match login_files::regular(dir, rel) {
        Ok(file) => Ok(serde_json::from_reader(file)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(e) => Err(e),
    }
}

pub(crate) fn caches(root: &Path, dir: &Path, config: &MorkConfig, places: &mut WallPlaces) -> io::Result<()> {
    for (entries, folder) in [(&config.caches, true), (&config.cache_files, false)] {
        for name in entries {
            let rel = Path::new(name);
            for base in [root, dir] {
                if folder {
                    mkdir(base, rel)?;
                } else {
                    mkdir(base, rel.parent().unwrap_or(Path::new("")))?;
                    let file = mork_sys::append_beneath(base, rel)?;
                    if !file.metadata()?.is_file() {
                        return Err(io::Error::other(format!("expected a regular cache file at {name}")));
                    }
                }
            }
            places.caches.push((root.join(rel).display().to_string(), dir.join(rel).display().to_string()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_author_preserves_gitconfig_punctuation() {
        let identity = git_identity(r#"A "B" \ C <tag#work@example.org>"#);
        assert_eq!(identity, "[user]\n\tname = \"A \\\"B\\\" \\\\ C\"\n\temail = \"tag#work@example.org\"\n");
    }

    #[test]
    fn login_paths_refuse_a_link_in_the_parent() -> io::Result<()> {
        let root = std::env::temp_dir().join(crate::random::uuid());
        std::fs::create_dir(&root)?;
        std::fs::create_dir(root.join("real"))?;
        std::fs::write(root.join("real/token"), b"fixture")?;
        std::os::unix::fs::symlink("real", root.join("alias"))?;
        assert!(mork_sys::link_beneath(&root, Path::new("alias/login"), Path::new("token")).is_err());
        assert!(login_files::read(&root, Path::new("alias/token")).is_err());
        std::fs::remove_dir_all(root)
    }
}
