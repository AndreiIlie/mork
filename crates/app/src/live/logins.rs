// A scratch renewal spends the real refresh token, so a changed credential must be returned.

use std::io;
use std::path::{Path, PathBuf};

use mork_engine::{files::sha256_hex, login_files, paths::Home};

#[derive(Debug)]
struct Copy {
    real: PathBuf,
    scratch: PathBuf,
    rel: &'static str,
    hash: String,
}

#[derive(Debug, Default)]
pub struct Logins {
    copied: Vec<Copy>,
}

impl Logins {
    pub fn copy_in(real: &Home, scratch: &Home) -> Result<Self, String> {
        let mut copied = Vec::new();
        for (in_login_home, rel) in [
            (false, login_files::CLAUDE),
            (false, login_files::CODEX),
            (true, ".claude.json"),
            (true, ".codex/config.toml"),
        ] {
            let root = |h: &Home| if in_login_home { h.login_home() } else { h.logins() };
            let (real, scratch) = (root(real), root(scratch));
            let path = Path::new(rel);
            let bytes = match login_files::read(&real, path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("{rel}: {e}")),
            };
            if let Some(parent) = scratch.join(path).parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            login_files::Destination::new(&scratch, path).and_then(|d| d.replace(&bytes)).map_err(|e| e.to_string())?;
            copied.push(Copy { real, scratch, rel, hash: sha256_hex(&bytes) });
        }
        Ok(Self { copied })
    }

    pub fn give_back(&self) -> Vec<String> {
        let mut said = Vec::new();
        for copy in &self.copied {
            let f = copy.rel;
            let path = Path::new(f);
            let Ok(bytes) = login_files::read(&copy.scratch, path) else { continue };
            if sha256_hex(&bytes) == copy.hash {
                continue;
            }
            if !login_files::read(&copy.real, path).is_ok_and(|b| sha256_hex(&b) == copy.hash) {
                said.push(format!("{f}: the real login changed during live-check and was kept"));
                continue;
            }
            let result = login_files::Destination::new(&copy.real, path).and_then(|d| d.replace(&bytes));
            said.push(match result {
                Ok(()) => format!("{f}: renewed by a scratch CLI, written back"),
                Err(e) => format!("{f}: could not write the renewed login back: {e}"),
            });
        }
        said
    }
}
