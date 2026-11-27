use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::path::{Component, Path};

use mork_types::JobId;
use mork_types::seen::OutputFile;
use mork_types::state::Job;

use crate::paths::Home;

// Written there by a CLI's protocol, never by its agent: Codex's reply schema and last message.
const PROTOCOL: &[&str] = &["schema.json", "last-message.json"];
// How many a view lists; how deep a turn's are looked for, how many entries of its `io/` are read
// at most, and the longest name kept, so an agent's folder can't hold the engine's loop.
const MAX: usize = 100;
const DEPTH: usize = 4;
const ENTRIES: usize = 1000;
const NAME_MAX: usize = 512;
/// The regular files `turns` produced in their seats' `io/` folders, newest first, at most [`MAX`], keyed by folder
/// id: each walked once through its own descriptors, no symlink, without its turns' inputs or protocol files.
pub fn produced<'a>(home: &Home, turns: impl IntoIterator<Item = &'a Job>) -> Vec<OutputFile> {
    let mut seats: BTreeMap<&OsStr, Vec<&Job>> = BTreeMap::new();
    let ios = home.seat_ios();
    for job in turns {
        let io = job.spec.env.get("MORK_IO").and_then(|io| Path::new(io).strip_prefix(&ios).ok());
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
        if let Ok(io) = mork_sys::open_dir_beneath(&root, Path::new(seat)) {
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
    let Ok(entries) = mork_sys::read_dir_of(dir) else { return };
    for e in entries.flatten() {
        let Some(l) = left.checked_sub(1) else { return };
        *left = l;
        let (Some(own), Ok(meta)) = (e.file_name().to_str().map(str::to_owned), e.metadata()) else { continue };
        let name = format!("{prefix}{own}");
        if meta.is_dir() && depth > 1 {
            if let Ok(sub) = mork_sys::open_dir_beneath(dir, Path::new(&own)) {
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
