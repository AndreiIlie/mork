//! Static web asset serving confined beneath `web_assets` with SPA `index.html` fallback (fac-671).

use std::fs::File;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

/// Normalizes a URL path into a safe relative path beneath `web_assets`; rejects traversal or bad bytes.
pub fn safe_rel_path(url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode_str(url_path).decode_utf8().ok()?;
    let trimmed = decoded.strip_prefix('/')?;
    let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Some(PathBuf::from("index.html"));
    }
    let mut rel = PathBuf::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\0') || seg.contains('\\') {
            return None;
        }
        rel.push(seg);
    }
    Some(rel)
}

/// Content-Type for a served static file, by lowercase file extension.
pub fn content_type(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Opens a regular static file beneath `root` without following symlinks, falling back to `index.html`
/// for a path naming no existing file that is extensionless or a `/cards/<id>` page, dotted ids included.
pub fn open_asset(root: &Path, url_path: &str) -> Option<(File, &'static str)> {
    let rel = safe_rel_path(url_path)?;
    if let Some(file) = open_regular(root, &rel) {
        return Some((file, content_type(&rel)));
    }
    // A card id may carry a dot (`ex-583.1`), so every `/cards/<id>` is the app's route.
    let route = rel.extension().is_none() || rel.parent() == Some(Path::new("cards"));
    if rel != Path::new("index.html") && route {
        let index = Path::new("index.html");
        return open_regular(root, index).map(|file| (file, content_type(index)));
    }
    None
}

fn open_regular(root: &Path, rel: &Path) -> Option<File> {
    let file = mork_sys::open_beneath_no_symlinks(root, rel).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;

    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mork-web-assets-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn safe_rel_path_blocks_traversal_and_maps_root_to_index() {
        assert_eq!(safe_rel_path("/"), Some(PathBuf::from("index.html")));
        assert_eq!(safe_rel_path("/assets/app.js"), Some(PathBuf::from("assets/app.js")));
        assert_eq!(safe_rel_path("/cards/fac-671"), Some(PathBuf::from("cards/fac-671")));
        assert_eq!(safe_rel_path("/cards/fac-671/"), Some(PathBuf::from("cards/fac-671")));
        for bad in ["/../etc/passwd", "/%2e%2e/secret", "/a/./b", "/a//b", "/a\\b", "/a%00b", "relative"] {
            assert_eq!(safe_rel_path(bad), None, "should reject {bad:?}");
        }
    }

    #[test]
    fn serves_files_falls_back_on_navigation_routes_and_blocks_symlinks() {
        let base = scratch("serve");
        let root = base.join("dist");
        let outside = base.join("outside.txt");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("index.html"), "<h1>app</h1>").unwrap();
        fs::write(root.join("assets/main.js"), "console.log(1)").unwrap();
        fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("leak.txt")).unwrap();

        let read_body = |path: &str| {
            let (mut f, ct) = open_asset(&root, path)?;
            let mut s = String::new();
            f.read_to_string(&mut s).ok()?;
            Some((s, ct))
        };

        assert_eq!(read_body("/"), Some(("<h1>app</h1>".into(), "text/html; charset=utf-8")));
        assert_eq!(read_body("/assets/main.js"), Some(("console.log(1)".into(), "text/javascript; charset=utf-8")));
        assert_eq!(read_body("/cards/fac-671"), Some(("<h1>app</h1>".into(), "text/html; charset=utf-8")));
        assert_eq!(read_body("/cards/ex-583.1"), Some(("<h1>app</h1>".into(), "text/html; charset=utf-8")));
        assert_eq!(read_body("/cards/pr-example.com-123"), Some(("<h1>app</h1>".into(), "text/html; charset=utf-8")));
        assert_eq!(read_body("/cards/a/b.js"), None);
        assert_eq!(read_body("/assets/missing.js"), None);
        assert_eq!(read_body("/leak.txt"), None);
        assert_eq!(read_body("/../outside.txt"), None);
        assert_eq!(read_body("/assets"), Some(("<h1>app</h1>".into(), "text/html; charset=utf-8")));

        let _ = fs::remove_dir_all(&base);
    }
}
