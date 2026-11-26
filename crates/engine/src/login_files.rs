//! CLI credentials shared between seat homes. Host-side operations stay under the opened directory.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::paths::Home;

pub const CLAUDE: &str = "claude/.credentials.json";
pub const CODEX: &str = "codex/auth.json";
pub const AGY: &str = "agy/antigravity-oauth-token";

/// Opens a regular file beneath the root, through no symlink in the relative path.
pub fn regular(root: &Path, rel: &Path) -> io::Result<File> {
    require_regular(mork_sys::open_beneath(root, rel)?)
}

fn require_regular(file: File) -> io::Result<File> {
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("expected a regular login file"));
    }
    Ok(file)
}

pub fn read(root: &Path, rel: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    regular(root, rel)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Holds the parent directory open when a seat can change the login path.
pub struct Destination {
    pub(crate) path: PathBuf,
    pinned: Option<(File, OsString)>,
}

impl Destination {
    pub fn new(root: &Path, rel: &Path) -> io::Result<Self> {
        let parent = rel.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let parent = mork_sys::open_beneath(root, parent)?;
        let name = rel.file_name().ok_or_else(|| io::Error::other("missing login file name"))?.into();
        Ok(Self { path: root.join(rel), pinned: Some((parent, name)) })
    }

    pub(crate) fn in_login_home(path: PathBuf) -> Self {
        Self { path, pinned: None }
    }

    pub(crate) fn open(&self) -> io::Result<File> {
        match &self.pinned {
            Some((parent, name)) => require_regular(mork_sys::open_file_beneath(parent, Path::new(name))?),
            None => File::open(&self.path),
        }
    }

    fn write_path(&self) -> PathBuf {
        match &self.pinned {
            Some((parent, name)) => Path::new("/proc/self/fd").join(parent.as_raw_fd().to_string()).join(name),
            None => self.path.clone(),
        }
    }

    pub fn replace(&self, bytes: &[u8]) -> io::Result<()> {
        let permissions = match fs::symlink_metadata(self.write_path()) {
            Ok(meta) if meta.is_file() => Some(meta.permissions()),
            Ok(_) => return Err(io::Error::other("expected a regular login file")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        self.stage(bytes, permissions)?.commit()
    }

    pub(crate) fn stage(&self, bytes: &[u8], permissions: Option<Permissions>) -> io::Result<Staged<'_>> {
        let temporary = self.write_path().with_extension(crate::random::uuid());
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary)?;
        let staged = Staged { destination: self, temporary };
        file.write_all(bytes)?;
        if let Some(mode) = permissions {
            file.set_permissions(mode)?;
        }
        file.sync_all()?;
        Ok(staged)
    }
}

pub(crate) struct Staged<'a> {
    destination: &'a Destination,
    temporary: PathBuf,
}

impl Staged<'_> {
    pub(crate) fn commit(self) -> io::Result<()> {
        fs::rename(&self.temporary, self.destination.write_path()).map_err(|e| {
            io::Error::new(e.kind(), format!("could not replace {}: {e}", self.destination.path.display()))
        })
    }
}

impl Drop for Staged<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}

/// Publishes the regular file that an interactive login leaves in place of its link.
pub fn publish_codex(home: &Home) -> io::Result<()> {
    let local = home.login_home();
    let rel = Path::new(".codex/auth.json");
    if std::fs::symlink_metadata(local.join(rel))?.is_symlink() {
        return Ok(());
    }
    let logins = home.logins();
    let _writer = codex_lock(&logins, "codex/auth.lock")?;
    let _commit = codex_lock(&logins, "codex/auth.json.lock")?;
    let bytes = read(&local, rel)?;
    Destination::new(&logins, Path::new(CODEX))?.replace(&bytes)?;
    std::fs::remove_file(local.join(rel))?;
    mork_sys::link_beneath(&local, rel, &logins.join(CODEX))
}

fn codex_lock(logins: &Path, name: &str) -> io::Result<File> {
    let file = mork_sys::append_beneath(logins, Path::new(name))?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("expected a regular Codex lock file"));
    }
    file.lock()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn staged_login_keeps_its_directory_and_cleans_up_on_drop() -> io::Result<()> {
        let root = std::env::temp_dir().join(crate::random::uuid());
        let directory = root.join("claude");
        fs::create_dir_all(&directory)?;
        fs::write(root.join(CLAUDE), b"before")?;
        fs::set_permissions(root.join(CLAUDE), Permissions::from_mode(0o640))?;
        let destination = Destination::new(&root, Path::new(CLAUDE))?;
        let original = root.join("original");
        fs::rename(&directory, &original)?;
        fs::create_dir(&directory)?;
        let mut before = Vec::new();
        destination.open()?.read_to_end(&mut before)?;
        assert_eq!(before, b"before");
        drop(destination.stage(b"discard", None)?);
        assert_eq!(fs::read_dir(&original)?.count(), 1);
        destination.replace(b"after")?;
        let saved = original.join(".credentials.json");
        assert_eq!(fs::read(&saved)?, b"after");
        assert_eq!(fs::metadata(saved)?.permissions().mode() & 0o777, 0o640);
        assert_eq!(fs::read_dir(directory)?.count(), 0);
        fs::remove_dir_all(root)
    }
}
