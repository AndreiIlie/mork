// A build carries its own `skills/` wherever it is installed (fac-88).
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() -> std::io::Result<()> {
    // Read at run time: the build folders share one target, so a path compiled into this script would name an older checkout.
    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| std::io::Error::other("Cargo supplied no CARGO_MANIFEST_DIR"))?,
    );
    let skills = manifest.join("../../skills");
    // A folder is scanned whole: an added, changed or removed skill builds again.
    println!("cargo:rerun-if-changed={}", skills.display());
    let mut files = Vec::new();
    walk(&skills, &skills, &mut files)?;
    files.sort();
    let mut out = String::from(
        "/// The skills' files: their paths in `skills/`, and their bytes.\npub const FILES: &[(&str, &[u8])] = &[\n",
    );
    for (rel, path) in &files {
        let _ = writeln!(out, "    ({rel:?}, include_bytes!({:?})),", path.display().to_string());
    }
    out.push_str("];\n");
    let dir = std::env::var_os("OUT_DIR").ok_or_else(|| std::io::Error::other("Cargo supplied no OUT_DIR"))?;
    std::fs::write(Path::new(&dir).join("skills.rs"), out)
}

// Links are left out: a skill is a file of the repository itself.
fn walk(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let path = e.path();
        let kind = e.file_type()?;
        if kind.is_dir() {
            walk(root, &path, files)?;
        } else if kind.is_file() {
            let rel = path.strip_prefix(root).map_err(std::io::Error::other)?;
            let rel = rel.to_str().ok_or_else(|| std::io::Error::other("a skill path is not UTF-8"))?;
            files.push((rel.to_owned(), path.canonicalize()?));
        }
    }
    Ok(())
}
