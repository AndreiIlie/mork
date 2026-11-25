use std::io;
use std::path::{Path, PathBuf};

/// Links `~/.local/bin/factory` to `bin`, refusing to replace anything but a factory-build link.
pub fn link_path(user_home: &Path, bin: &Path) -> io::Result<PathBuf> {
    let link = destination(user_home)?;
    crate::files::link_whole(&link, bin)?;
    Ok(link)
}

pub(super) fn installed() -> io::Result<PathBuf> {
    destination(&user_home()?)
}

/// Whether a pin or reload may replace the shortcut, checked before a reload execs its target.
pub fn check_shortcut() -> io::Result<()> {
    replaceable(&user_home()?).map(drop)
}

fn user_home() -> io::Result<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).ok_or_else(|| io::Error::other("no HOME"))
}

fn destination(user_home: &Path) -> io::Result<PathBuf> {
    let link = replaceable(user_home)?;
    std::fs::create_dir_all(user_home.join(".local/bin"))?;
    Ok(link)
}

fn replaceable(user_home: &Path) -> io::Result<PathBuf> {
    let link = user_home.join(".local/bin/factory");
    match std::fs::symlink_metadata(&link) {
        Ok(meta) => {
            let target = std::fs::read_link(&link).ok();
            let a_build = meta.file_type().is_symlink()
                && target.as_ref().and_then(|t| t.file_name()).is_some_and(|n| n == "factory");
            if !a_build {
                return Err(io::Error::other(format!(
                    "{} is there and isn't a link to a factory build ({}); move it away first",
                    link.display(),
                    target.map_or_else(|| "a file".to_owned(), |t| format!("a link to {}", t.display()))
                )));
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(link)
}
