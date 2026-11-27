use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use mork_types::config::Cli;

const PROXIES: &[&str] = &["rustc", "rustdoc", "rustfmt", "cargo-fmt", "cargo-clippy", "clippy-driver", "rustup"];

#[derive(Default)]
pub(super) struct Tools {
    pub clis: Vec<Cli>,
    pub mounts: BTreeSet<PathBuf>,
    pub links: BTreeMap<String, PathBuf>,
    pub rustup: Option<PathBuf>,
    pub cargo: bool,
    pub npm: bool,
}

fn ephemeral(path: &Path) -> bool {
    ["/run", "/tmp", "/var/tmp"].iter().any(|root| path.starts_with(root))
}

fn executable(dir: &Path, name: &str) -> Option<(PathBuf, PathBuf)> {
    let place = dir.canonicalize().ok()?.join(name);
    let metadata = fs::metadata(&place).ok()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 || ephemeral(&place) {
        return None;
    }
    let target = place.canonicalize().ok()?;
    Some((place, target))
}

fn npm_package(target: &Path) -> Option<&Path> {
    let modules = target.ancestors().find(|p| p.ends_with("lib/node_modules"))?;
    let first = target.ancestors().find(|p| p.parent() == Some(modules))?;
    if first.file_name()?.as_encoded_bytes().starts_with(b"@") {
        target.ancestors().find(|p| p.parent() == Some(first))
    } else {
        Some(first)
    }
}

impl Tools {
    fn add(&mut self, name: &str, place: &Path, target: &Path, home: &Path) {
        let package = npm_package(target);
        let mount = package.unwrap_or(place);
        if mount.starts_with(home) {
            self.mounts.insert(mount.to_owned());
        }
        if !["/usr/bin", "/bin", "/usr/local/bin"].iter().any(|dir| place.parent() == Some(Path::new(dir))) {
            self.links.insert(name.into(), if package.is_some() { target } else { place }.to_owned());
        }
    }

    fn add_rustup(&mut self, root: PathBuf, home: &Path, entries: impl Iterator<Item = PathBuf>) {
        if root.starts_with(home) {
            self.mounts.extend(entries);
        }
        self.rustup = Some(root);
    }
}

pub(super) fn installed(home: &Path) -> Result<Tools, String> {
    let path = env::var_os("PATH").unwrap_or_default();
    let mut tools = Tools::default();
    for name in ["claude", "codex", "agy", "node", "npm", "npx", "cargo"] {
        let Some((place, target)) = env::split_paths(&path).find_map(|d| executable(&d, name)) else { continue };
        tools.add(name, &place, &target, home);
        if let Some(cli) = Cli::parse(name) {
            tools.clis.push(cli);
        }
        tools.npm |= name == "npm";
        if name == "cargo" {
            tools.cargo = true;
            let dir = place.parent().ok_or("cargo has no parent folder")?;
            for proxy in PROXIES {
                if let Some((p, t)) = executable(dir, proxy) {
                    tools.add(proxy, &p, &t, home);
                }
            }
            let root = env::var_os("RUSTUP_HOME").map_or_else(|| home.join(".rustup"), PathBuf::from);
            if root.is_dir() {
                let root = root.canonicalize().map_err(|e| e.to_string())?;
                let entries = ["toolchains", "update-hashes", "settings.toml"].map(|n| root.join(n));
                tools.add_rustup(root, home, entries.into_iter().filter(|p| p.exists()));
            }
        }
    }
    if tools.clis.is_empty() {
        return Err("install claude, codex or agy and put it on PATH".into());
    }
    Ok(tools)
}

pub(super) fn web_assets(bin: &Path) -> Option<PathBuf> {
    let parent = bin.parent()?;
    [Some(parent), parent.ancestors().nth(2)]
        .into_iter()
        .flatten()
        .map(|p| p.join("web/dist"))
        .find(|p| p.join("index.html").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_mount_only_the_needed_files_and_packages() {
        let home = Path::new("/home/a");
        let mut tools = Tools::default();
        for (name, place, target) in [
            ("npm", "/usr/bin/npm", "/usr/share/nodejs/npm/bin/npm-cli.js"),
            ("agy", "/opt/vendor/bin/agy", "/opt/vendor/bin/agy"),
            ("codex", "/home/a/.local/bin/codex", "/home/a/.local/lib/node_modules/@openai/codex/bin/codex.js"),
            ("node", "/home/a/bin/node", "/home/a/bin/node"),
            ("claude", "/home/a/.local/bin/claude", "/home/a/.local/share/claude/versions/1"),
            ("cargo", "/home/a/.cargo/bin/cargo", "/home/a/.cargo/bin/rustup"),
        ] {
            tools.add(name, Path::new(place), Path::new(target), home);
        }
        assert_eq!(
            tools.mounts,
            [
                "/home/a/bin/node",
                "/home/a/.local/bin/claude",
                "/home/a/.cargo/bin/cargo",
                "/home/a/.local/lib/node_modules/@openai/codex"
            ]
            .map(PathBuf::from)
            .into_iter()
            .collect()
        );
        assert!(!tools.links.contains_key("npm"));
        assert_eq!(tools.links["agy"], Path::new("/opt/vendor/bin/agy"));
        assert_eq!(tools.links["codex"], Path::new("/home/a/.local/lib/node_modules/@openai/codex/bin/codex.js"));
        assert_eq!(tools.links["claude"], Path::new("/home/a/.local/bin/claude"));
        let mut shared = Tools::default();
        let place = Path::new("/home/a/.local/bin/cargo");
        shared.add("cargo", place, place, home);
        shared.add_rustup("/opt/rustup".into(), home, [PathBuf::from("/opt/rustup/toolchains")].into_iter());
        assert_eq!(shared.mounts, [place.to_owned()].into_iter().collect());
        assert_eq!(shared.rustup.as_deref(), Some(Path::new("/opt/rustup")));
    }
}
