//! The keyed bridge serves published and pasted images.

use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use image::{ImageDecoder, ImageFormat, ImageReader};
use percent_encoding::percent_decode_str;

use super::http;
use crate::paths::Home;

pub(super) const PASTE: &str = "/images/pasted";
const MAX_BYTES: u64 = 64 << 20;
type Status = (u16, &'static str);

fn plain(raw: &str) -> Option<String> {
    let s = percent_decode_str(raw).decode_utf8().ok()?;
    let bad = s.is_empty() || s == "." || s == ".." || s.contains(['/', '\\', '\0']);
    (!bad).then(|| s.into_owned())
}

/// What the route's path names beneath the jobs folder: one plain job and one plain file, each decoded once.
pub fn image_rel(url_path: &str) -> Option<PathBuf> {
    let (job, file) = url_path.strip_prefix("/images/")?.split_once('/')?;
    Some(PathBuf::from(plain(job)?).join("published").join(plain(file)?))
}

/// Opens only a regular image beneath the matching store, without following links inside the store.
pub fn open_image(home: &Home, url_path: &str) -> Option<(File, &'static str)> {
    let file = if let Some(raw) = url_path.strip_prefix("/images/pasted/") {
        let root = File::open(home.root()).ok()?;
        mork_sys::open_file_beneath(&root, &Path::new("pasted").join(plain(raw)?)).ok()?
    } else {
        mork_sys::open_beneath_no_symlinks(&home.jobs(), &image_rel(url_path)?).ok()?
    };
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

pub(super) fn upload(stream: &mut TcpStream, head: &http::Head, leftover: Vec<u8>, home: &Home) -> io::Result<()> {
    let result = body_length(head).and_then(|length| {
        let mut bytes = leftover;
        if bytes.len() as u64 > length {
            return Err((400, "Invalid image length"));
        }
        let remaining = length - bytes.len() as u64;
        (&mut *stream).take(remaining).read_to_end(&mut bytes).map_err(|_| (400, "Incomplete image upload"))?;
        if bytes.len() as u64 != length {
            return Err((400, "Incomplete image upload"));
        }
        store(home, &bytes)
    });
    match result {
        Ok(path) => {
            let body = path.to_str().ok_or_else(|| io::Error::other("paste path is not UTF-8"))?;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            stream.flush()
        }
        Err((code, why)) => http::write_status(stream, code, why),
    }
}

fn body_length(head: &http::Head) -> Result<u64, Status> {
    let mut lengths = head.headers.iter().filter(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let (_, value) = lengths.next().ok_or((411, "Length Required"))?;
    if lengths.next().is_some()
        || head.header("transfer-encoding").is_some()
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err((400, "Invalid image length"));
    }
    let length = value.parse::<u64>().map_err(|_| (400, "Invalid image length"))?;
    if length > MAX_BYTES { Err((413, "Image too large")) } else { Ok(length) }
}

fn store(home: &Home, bytes: &[u8]) -> Result<PathBuf, Status> {
    let kind = match image::guess_format(bytes) {
        Ok(kind @ (ImageFormat::Png | ImageFormat::Jpeg)) => kind,
        _ => return Err((415, "Expected a PNG or JPEG image")),
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), kind);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_BYTES);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| (422, "Invalid image"))?;
    let (width, height) = decoder.dimensions();
    if decoder.total_bytes() > MAX_BYTES || u64::from(width) * u64::from(height) > MAX_BYTES / 4 {
        return Err((413, "Image too large"));
    }
    let orientation = decoder.orientation().map_err(|_| (422, "Invalid image"))?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|_| (422, "Invalid image"))?;
    image.apply_orientation(orientation);
    let rgba = image.into_rgba8();
    crate::clipboard::save(home, rgba.width() as usize, rgba.height() as usize, &rgba)
        .map_err(|_| (500, "Could not save pasted image"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_length_is_unambiguous_and_bounded() {
        let length = |headers: &str| {
            let raw = format!("POST /images/pasted HTTP/1.1\r\n{headers}\r\n");
            body_length(&http::parse_head(raw.as_bytes()).unwrap())
        };
        assert_eq!(length("Content-Length: 123\r\n"), Ok(123));
        for bad in [
            "",
            "Content-Length: 67108865\r\n",
            "Content-Length: 5\r\nContent-Length: 6\r\n",
            "Content-Length: 5\r\nTransfer-Encoding: chunked\r\n",
        ] {
            assert!(length(bad).is_err());
        }
    }

    #[test]
    fn uploads_use_the_confined_png_store() {
        let home = Home::new(std::env::temp_dir().join(crate::random::uuid()));
        let saved = crate::clipboard::save(&home, 1, 1, &[0, 25, 0, 255]).unwrap();
        let bytes = std::fs::read(&saved).unwrap();
        assert_eq!(store(&home, &bytes), Ok(saved.clone()));
        assert!(store(&home, &bytes[..15]).is_err());
        assert!(store(&home, b"plain text").is_err());
        let name = saved.file_name().unwrap().to_str().unwrap();
        let (mut opened, kind) = open_image(&home, &format!("/images/pasted/{name}")).unwrap();
        let mut served = Vec::new();
        opened.read_to_end(&mut served).unwrap();
        assert_eq!((kind, served), ("image/png", bytes));
        std::os::unix::fs::symlink(&saved, home.pasted().join("linked.png")).unwrap();
        assert!(open_image(&home, "/images/pasted/linked.png").is_none());
        assert!(open_image(&home, "/images/pasted/sub%2Ffile.png").is_none());
        std::fs::remove_dir_all(home.root()).unwrap();
    }

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
