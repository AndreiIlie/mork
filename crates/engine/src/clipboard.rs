//! The desktop's clipboard (fac-31): its image, read over X11 (on GNOME through XWayland, which
//! shares the Wayland clipboard; iced reads only text), and saved as a PNG under the home.

use std::path::PathBuf;

use crate::paths::Home;

/// The clipboard's image, saved by `save`: its path, none when the clipboard holds no image, or
/// why it couldn't be read or saved. The read waits up to seconds for the clipboard's owner.
pub fn image(home: &Home) -> Result<Option<PathBuf>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("the clipboard can't be read: {e}"))?;
    let data = match clipboard.get_image() {
        Ok(d) => d,
        Err(arboard::Error::ContentNotAvailable) => return Ok(None),
        Err(e) => return Err(format!("the clipboard's image can't be read: {e}")),
    };
    save(home, data.width, data.height, &data.bytes).map(Some)
}

/// An image's RGBA pixels written as a PNG under `Home::pasted`, named by its content, so the
/// same image pasted again is the same file: its path.
pub fn save(home: &Home, width: usize, height: usize, rgba: &[u8]) -> Result<PathBuf, String> {
    let png = encode(width, height, rgba).map_err(|e| format!("the pasted image can't be saved: {e}"))?;
    let dir = home.pasted();
    let hash = crate::files::sha256_hex(&png);
    let path = dir.join(format!("{}.png", &hash[..16]));
    std::fs::create_dir_all(&dir)
        .and_then(|()| crate::files::write_whole(&path, &png))
        .map_err(|e| format!("the pasted image can't be saved in {}: {e}", dir.display()))?;
    Ok(path)
}

fn encode(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let size = |n: usize| u32::try_from(n).map_err(|_| format!("{n} px is too large"));
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, size(width)?, size(height)?);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}
