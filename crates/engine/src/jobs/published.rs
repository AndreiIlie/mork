//! The images a seat's reply attaches (fac-278): each is read beneath its turn's `$FACTORY_IO` through no symlink and
//! kept as a copy in the job's `published/`, outside every wall, so no path an agent pointed elsewhere is ever read.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

/// The folder of a job's published copies, in its job folder, outside every wall.
pub const FOLDER: &str = "published";

/// Replaces each image the reply names with its copy, read beneath `io`, the job's `$FACTORY_IO`, or why it made
/// none, trusting no path the agent wrote; a source named again is copied once, its result reused (fac-614).
pub fn publish(jobs: &Path, job: &str, io: &Path, reply: &mut Value) {
    let mut n = 0usize;
    let mut done: HashMap<String, String> = HashMap::new();
    let mut list = |images: &mut Value| {
        let Value::Array(images) = images else { return };
        for image in images {
            let Some(src) = image.as_str().map(str::to_owned) else { continue };
            n += 1;
            let result = done.entry(src.clone()).or_insert_with(|| match copy_of(jobs, job, io, &src, n) {
                Ok(copy) => copy,
                Err(e) => format!("not published ({e}): {src}"),
            });
            *image = Value::String(result.clone());
        }
    };
    if let Some(images) = reply.pointer_mut("/hand/images") {
        list(images);
    }
    if let Some(Value::Array(messages)) = reply.get_mut("messages") {
        messages.iter_mut().filter_map(|m| m.get_mut("images")).for_each(list);
    }
}

/// Copies `src`, read beneath `io` as a reply's images are, into job `job`'s `published/` as `say-<k>-<name>`, `k` the
/// first number no copy there holds (fac-654): a message keeps the file as it was sent. The copy's path.
pub fn say_copy(jobs: &Path, job: &str, io: &Path, src: &str) -> io::Result<String> {
    let (name, bytes) = read_in(io, src)?;
    let dir = folder(jobs, job)?;
    let mut k = 1usize;
    while dir.join(format!("say-{k}-{name}")).symlink_metadata().is_ok() {
        k += 1;
    }
    write_copy(&dir, &format!("say-{k}-{name}"), &bytes)
}

fn copy_of(jobs: &Path, job: &str, io: &Path, src: &str, n: usize) -> io::Result<String> {
    let (name, bytes) = read_in(io, src)?;
    write_copy(&folder(jobs, job)?, &format!("{n}-{name}"), &bytes)
}

// The name and bytes of `src`, read through the turn's own `io` only: another folder, a `..`, a link and anything but
// a regular file fail.
fn read_in(io: &Path, src: &str) -> io::Result<(String, Vec<u8>)> {
    let refused = |why: &str| io::Error::new(io::ErrorKind::InvalidInput, why.to_owned());
    let rel = Path::new(src).strip_prefix(io).map_err(|_| refused("not in your io/ folder, $FACTORY_IO"))?;
    let name = match rel.file_name().and_then(|f| f.to_str()) {
        Some(name) if rel.components().all(|c| matches!(c, Component::Normal(_))) => name.to_owned(),
        _ => return Err(refused("not a plain path")),
    };
    let mut file = factory_sys::open_beneath(io, rel)?;
    if !file.metadata()?.is_file() {
        return Err(refused("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((name, bytes))
}

fn folder(jobs: &Path, job: &str) -> io::Result<PathBuf> {
    factory_sys::mkdir_beneath(&jobs.join(job), Path::new(FOLDER))?;
    Ok(jobs.join(job).join(FOLDER))
}

fn write_copy(dir: &Path, copy: &str, bytes: &[u8]) -> io::Result<String> {
    factory_sys::write_beneath(dir, Path::new(copy), bytes)?;
    Ok(dir.join(copy).display().to_string())
}
