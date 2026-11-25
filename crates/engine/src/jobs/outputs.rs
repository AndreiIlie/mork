//! The files a card's agent turns produced in their seats' `io/` folders (fac-105, fac-654): listed for its view,
//! and one handed to the system's viewer as a private copy of the bytes read beneath its folder.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use factory_types::JobId;
use factory_types::seen::OutputFile;
use factory_types::state::Job;

use crate::paths::Home;

// Written there by a CLI's protocol, never by its agent: Codex's reply schema and last message.
const PROTOCOL: &[&str] = &["schema.json", "last-message.json"];
// How many a view lists; how deep a turn's are looked for, how many entries of its `io/` are read
// at most, and the longest name kept, so an agent's folder can't hold the engine's loop.
const MAX: usize = 100;
const DEPTH: usize = 4;
const ENTRIES: usize = 1000;
const NAME_MAX: usize = 512;
// What the viewer is given: images, PDFs and text, never a file it could run.
const VIEWABLE: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "pdf", "txt", "log", "md", "json", "jsonl", "csv", "tsv", "diff",
    "patch",
];
// The largest file the viewer is handed a copy of, and how long a copy is kept for it to read:
// the session's runtime folder they lie in is often memory.
const STAGED_MAX: u64 = 64 << 20;
const KEPT_MS: i64 = 3_600_000;

/// The regular files `turns` produced in their seats' `io/` folders, newest first, at most [`MAX`], keyed by folder
/// id: each walked once through its own descriptors, no symlink, without its turns' inputs or protocol files.
pub fn produced<'a>(home: &Home, turns: impl IntoIterator<Item = &'a Job>) -> Vec<OutputFile> {
    let mut seats: BTreeMap<&OsStr, Vec<&Job>> = BTreeMap::new();
    let ios = home.seat_ios();
    for job in turns {
        let io = job.spec.env.get("FACTORY_IO").and_then(|io| Path::new(io).strip_prefix(&ios).ok());
        if let Some((seat, rest)) = io.and_then(first)
            && rest.as_os_str().is_empty()
        {
            seats.entry(seat).or_default().push(job);
        }
    }
    let Ok(root) = File::open(&ios) else { return Vec::new() };
    let mut out = Vec::new();
    for (seat, jobs) in seats.iter().filter_map(|(s, jobs)| Some((s.to_str()?, jobs))) {
        let (mut found, mut left) = (Vec::new(), ENTRIES);
        if let Ok(io) = factory_sys::open_dir_beneath(&root, Path::new(seat)) {
            walk(&io, "", DEPTH, &mut left, &mut found);
        }
        let Some(agent) = jobs.first().map(|j| j.owner.clone()) else { continue };
        out.extend(found.into_iter().filter(|(name, ..)| !jobs.iter().any(|j| supplied(j, name))).map(
            |(name, size, modified_ms)| OutputFile {
                job: JobId::new(seat),
                agent: agent.clone(),
                name,
                size,
                modified_ms,
            },
        ));
    }
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then_with(|| (&a.job, &a.name).cmp(&(&b.job, &b.name))));
    out.truncate(MAX);
    out
}

// Its first component, a plain name, and the rest of it.
fn first(p: &Path) -> Option<(&OsStr, &Path)> {
    let mut parts = p.components();
    let Some(Component::Normal(name)) = parts.next() else { return None };
    Some((name, parts.as_path()))
}

fn supplied(job: &Job, name: &str) -> bool {
    PROTOCOL.contains(&name) || job.io_files.contains_key(name) || job.io_copies.contains_key(name)
}

// The regular files in the open folder `dir`, named from `prefix`, down to `depth` folders and
// within `left` entries read: each folder read through its descriptor, a symlink never followed.
fn walk(dir: &File, prefix: &str, depth: usize, left: &mut usize, found: &mut Vec<(String, u64, i64)>) {
    let Ok(entries) = factory_sys::read_dir_of(dir) else { return };
    for e in entries.flatten() {
        let Some(l) = left.checked_sub(1) else { return };
        *left = l;
        let (Some(own), Ok(meta)) = (e.file_name().to_str().map(str::to_owned), e.metadata()) else { continue };
        let name = format!("{prefix}{own}");
        if meta.is_dir() && depth > 1 {
            if let Ok(sub) = factory_sys::open_dir_beneath(dir, Path::new(&own)) {
                walk(&sub, &format!("{name}/"), depth - 1, left, found);
            }
        } else if meta.is_file() && name.len() <= NAME_MAX {
            found.push((name, meta.len(), modified_ms(&meta)));
        }
    }
}

fn modified_ms(meta: &std::fs::Metadata) -> i64 {
    let since = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    since.map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Seat folder `seat`'s file `name` (fac-654), opened for the viewer beneath that folder: a path inside it, a regular
/// file, reached through no symlink; anything else is refused, a FIFO at once.
pub fn checked(home: &Home, seat: &str, name: &str) -> io::Result<File> {
    if !plain(Path::new(seat)) || Path::new(seat).components().count() != 1 {
        return Err(refused("not a path inside its io/"));
    }
    beneath(&home.seat_io(seat), Path::new(name))
}

// `rel`, opened for the viewer beneath `dir` as [`checked`] says.
fn beneath(dir: &Path, rel: &Path) -> io::Result<File> {
    if !plain(rel) {
        return Err(refused("not a path inside its folder"));
    }
    let kind = rel.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if !kind.is_some_and(|k| VIEWABLE.contains(&k.as_str())) {
        return Err(refused("not a kind of file the viewer is given"));
    }
    let file = factory_sys::open_beneath(dir, rel)?;
    if !file.metadata()?.is_file() {
        return Err(refused("not a regular file"));
    }
    Ok(file)
}

fn plain(p: &Path) -> bool {
    !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))
}

fn refused(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why.to_owned())
}

/// A copy of the [`checked`] `file` for the viewer in a folder of its own in the private `dir`: read through its
/// descriptor, at most [`STAGED_MAX`], read-only and never executable; copies over [`KEPT_MS`] old are removed first.
pub fn stage(dir: &Path, file: &File, name: &str) -> io::Result<PathBuf> {
    let len = file.metadata()?.len();
    if len > STAGED_MAX {
        let why = format!("{} MB, more than the {} MB the viewer is handed", len >> 20, STAGED_MAX >> 20);
        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    }
    let base = Path::new(name).file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no name"))?;
    private(dir)?;
    let now = crate::clock::now_ms();
    sweep(dir, now);
    let own = dir.join(format!("{now}-{}", crate::random::salt()));
    std::fs::DirBuilder::new().mode(0o700).create(&own)?;
    let path = own.join(base);
    let copied = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(&path)
        .and_then(|mut out| io::copy(&mut file.take(len), &mut out));
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&own);
        return Err(e);
    }
    Ok(path)
}

// `dir`, made private when missing; refused when it is anything else, so a copy never lands where
// another can read it.
fn private(dir: &Path) -> io::Result<()> {
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        Ok(()) | Err(_) => {}
    }
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
        let why = format!("{} isn't a private folder", dir.display());
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, why));
    }
    Ok(())
}

// Removes the copies handed to a viewer over KEPT_MS ago, read by then, each in its own folder.
fn sweep(dir: &Path, now_ms: i64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if e.metadata().is_ok_and(|m| now_ms.saturating_sub(modified_ms(&m)) > KEPT_MS) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Opens a file a turn produced in the system's viewer: a [`stage`]d copy of what [`checked`]
/// opened, in `$XDG_RUNTIME_DIR/factory-viewer`, the desktop session's own, outside every wall.
pub fn open(home: &Home, seat: &str, name: &str) -> io::Result<()> {
    view(home, &checked(home, seat, name)?, name)
}

/// Opens a message's image (fac-278) in the system's viewer, as [`open`] does a produced file:
/// the copy [`staged_image`] made of it.
pub fn open_image(home: &Home, path: &str) -> io::Result<()> {
    super::launch::viewer(home.root(), &staged_image(home, path, &runtime()?.join("factory-viewer"))?)
}

/// A message's image, [`stage`]d in `dir`: `path` names a file of a seat's `io/`, a job's published copy or a file of
/// its `io/`, or one pasted, read beneath that folder alone through no symlink, a regular PNG or JPEG by its bytes.
pub fn staged_image(home: &Home, path: &str, dir: &Path) -> io::Result<PathBuf> {
    let (folder, rel) = image_folder(home, Path::new(path))?;
    if !plain(rel) {
        return Err(refused("not a path inside its folder"));
    }
    let file = factory_sys::open_beneath(&folder, rel)?;
    if !file.metadata()?.is_file() {
        return Err(refused("not a regular file"));
    }
    let mut head = [0u8; 16];
    let n = std::os::unix::fs::FileExt::read_at(&file, &mut head, 0)?;
    // What the window's thumbnails draw, and never a file a viewer could run.
    let kind = match image::guess_format(&head[..n]) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Jpeg) => "jpg",
        _ => return Err(refused("not a PNG or JPEG image")),
    };
    stage(dir, &file, &rel.with_extension(kind).display().to_string())
}

// The folder an image path is read beneath, and its path there: a seat's `io/` (fac-654), a job's published copies
// or its `io/`, or the pasted images.
fn image_folder<'a>(home: &Home, path: &'a Path) -> io::Result<(PathBuf, &'a Path)> {
    let (jobs, seats, pasted) = (home.jobs(), home.seat_ios(), home.pasted());
    if let Ok(rel) = path.strip_prefix(&pasted) {
        return Ok((pasted, rel));
    }
    if let Some((seat, rel)) = path.strip_prefix(&seats).ok().and_then(first) {
        return Ok((seats.join(seat), rel));
    }
    let not_ours = |_| refused("not a seat's, a job's or a pasted file");
    let mut parts = path.strip_prefix(&jobs).map_err(not_ours)?.components();
    let (Some(Component::Normal(job)), Some(Component::Normal(folder))) = (parts.next(), parts.next()) else {
        return Err(refused("not a job's file"));
    };
    if folder != "io" && folder != super::published::FOLDER {
        return Err(refused("not in a job's io/ nor its published copies"));
    }
    Ok((jobs.join(job).join(folder), parts.as_path()))
}

fn view(home: &Home, file: &File, name: &str) -> io::Result<()> {
    super::launch::viewer(home.root(), &stage(&runtime()?.join("factory-viewer"), file, name)?)
}

fn runtime() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_absolute());
    runtime.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no XDG_RUNTIME_DIR to copy it to"))
}
