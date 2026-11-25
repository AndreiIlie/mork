// Written back whole with the agent home file's mode, unless that file changed meanwhile: a renewed
// login spends the renewal key the agent home's file holds (1.11, M5.33).

use std::path::{Path, PathBuf};

use factory_engine::files::sha256_hex;

/// The login files, relative to an agent home (1.7.2).
pub const LOGINS: &[&str] = &[".claude/.credentials.json", ".claude.json", ".codex/auth.json", ".codex/config.toml"];

/// The logins copied into a scratch agent home: each file, and its content's hash then.
#[derive(Debug, Default)]
pub struct Logins {
    real: PathBuf,
    scratch: PathBuf,
    copied: Vec<(&'static str, String)>,
}

fn hash_of(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| sha256_hex(&b))
}

impl Logins {
    /// Copies each login the agent home `real` has into the scratch agent home `scratch`.
    pub fn copy_in(real: &Path, scratch: &Path) -> Result<Self, String> {
        let mut copied = Vec::new();
        for f in LOGINS {
            let from = real.join(f);
            if !from.is_file() {
                continue;
            }
            let bytes = std::fs::read(&from).map_err(|e| format!("{}: {e}", from.display()))?;
            let to = scratch.join(f);
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
            copied.push((*f, sha256_hex(&bytes)));
        }
        Ok(Self { real: real.to_owned(), scratch: scratch.to_owned(), copied })
    }

    /// Returns one line per login written back (none for an unchanged one); an agent home file that
    /// changed since the copy is kept.
    pub fn give_back(&self) -> Vec<String> {
        let mut said = Vec::new();
        for (f, at_copy) in &self.copied {
            let (real, scratch) = (self.real.join(f), self.scratch.join(f));
            let Ok(renewed) = std::fs::read(&scratch) else { continue };
            if sha256_hex(&renewed) == *at_copy {
                continue;
            }
            if hash_of(&real).as_deref() != Some(at_copy.as_str()) {
                said.push(format!(
                    "{f}: a scratch CLI renewed it, but the agent home's own file changed meanwhile and is kept; if a CLI fails to log in, log it in again (factory login)"
                ));
                continue;
            }
            said.push(match write_back(&real, &renewed) {
                Ok(()) => format!("{f}: renewed by a scratch CLI, written back to the agent home"),
                Err(e) => format!("{f}: renewed by a scratch CLI, but it couldn't be written back: {e}"),
            });
        }
        said
    }
}

// Whole or not at all: a temporary file beside it, with its mode, renamed over it.
fn write_back(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o7777;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.live-check"));
    std::fs::write(&tmp, bytes)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&tmp, path)
}
