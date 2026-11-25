// A turn's images (fac-278, fac-654): named in its seat's `io/` by the job bringing them, so no turn's copy replaces
// another's there, and read where an agent or Andrei left them, through no link an agent planted.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Component, Path};

use crate::paths::Home;

/// The copies job `job` makes of `images` in `io`, by their names there, and their paths.
pub(super) fn images_of(images: &[String], job: &str, io: &Path) -> (BTreeMap<String, String>, Vec<String>) {
    let mut copies = BTreeMap::new();
    let mut shown = Vec::new();
    for (n, src) in images.iter().enumerate() {
        let name = Path::new(src).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let rel = format!("images/{job}-{}-{name}", n + 1);
        shown.push(io.join(&rel).display().to_string());
        copies.insert(rel, src.clone());
    }
    (copies, shown)
}

/// An image's bytes. One under the home's job or seat folders is opened beneath a job's `io/` or `published/`, or a
/// seat's `io/`, through no symlink, and must be a regular file (M2.1); Andrei's may be anywhere.
pub(super) fn read_image(home: &Home, src: &str) -> io::Result<Vec<u8>> {
    let refused = |why: &str| io::Error::new(io::ErrorKind::InvalidInput, why.to_owned());
    let path = Path::new(src);
    let mut file = if let Ok(rel) = path.strip_prefix(home.jobs()) {
        let (job, folder, rel) = in_job(rel).ok_or_else(|| refused("not in a job's io/ nor its published copies"))?;
        factory_sys::open_beneath(&home.job(job).join(folder), rel)?
    } else if let Ok(rel) = path.strip_prefix(home.seat_ios()) {
        let (seat, rel) = named(rel).ok_or_else(|| refused("not in a seat's io/"))?;
        factory_sys::open_beneath(&home.seat_io(seat), rel)?
    } else {
        std::fs::File::open(path)?
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

// Its first component, a plain name, and what lies below it, never nothing.
fn named(p: &Path) -> Option<(&str, &Path)> {
    let mut parts = p.components();
    let Some(Component::Normal(name)) = parts.next() else { return None };
    let below = parts.as_path();
    (!below.as_os_str().is_empty()).then_some((name.to_str()?, below))
}

fn in_job(p: &Path) -> Option<(&str, &'static str, &Path)> {
    let (job, below) = named(p)?;
    let (folder, rel) = named(below)?;
    let folder = ["io", crate::jobs::published::FOLDER].into_iter().find(|f| folder == *f)?;
    Some((job, folder, rel))
}
