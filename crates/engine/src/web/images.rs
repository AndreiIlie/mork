//! A message's published image over the bridge (fac-696): `/images/<job>/<file>` reads `<job>/published/<file>`
//! beneath the jobs folder alone, a regular PNG or JPEG by its bytes, and nothing else.

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

/// What the route's path names beneath the jobs folder: one plain job and one plain file, each decoded once.
pub fn image_rel(url_path: &str) -> Option<PathBuf> {
    let (job, file) = url_path.strip_prefix("/images/")?.split_once('/')?;
    let plain = |raw: &str| {
        let s = percent_decode_str(raw).decode_utf8().ok()?;
        let bad = s.is_empty() || s == "." || s == ".." || s.contains(['/', '\\', '\0']);
        (!bad).then(|| s.into_owned())
    };
    Some(PathBuf::from(plain(job)?).join("published").join(plain(file)?))
}

/// Opens the image the path names, following no link, in `jobs`' own path too, and never leaving it, with its type.
pub fn open_image(jobs: &Path, url_path: &str) -> Option<(File, &'static str)> {
    let file = mork_sys::open_beneath_no_symlinks(jobs, &image_rel(url_path)?).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut head = [0u8; 8];
    file.read_exact_at(&mut head, 0).ok()?;
    if head == *b"\x89PNG\r\n\x1a\n" {
        Some((file, "image/png"))
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some((file, "image/jpeg"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_names_one_plain_job_and_file_beneath_published() {
        let rel = image_rel("/images/20261125-090000-fac-1-foreman-ab12/1-shot%20one.png");
        assert_eq!(rel, Some(PathBuf::from("20261125-090000-fac-1-foreman-ab12/published/1-shot one.png")));
        for bad in [
            "/images/job",
            "/images//a.png",
            "/images/job/",
            "/images/../a.png",
            "/images/job/..",
            "/images/%2e%2e/a.png",
            "/images/job/a%2fb.png",
            "/images/job/a/b.png",
            "/images/job/a%5cb.png",
            "/images/job/a%00.png",
            "/static/job/a.png",
        ] {
            assert_eq!(image_rel(bad), None, "{bad}");
        }
    }
}
