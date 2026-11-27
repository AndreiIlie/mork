use std::path::PathBuf;

use crate::paths::Home;

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
