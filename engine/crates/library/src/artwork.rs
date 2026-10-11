//! Artwork cache: each distinct image is resized once to 64/300/1000 px lossy WebP under
//! `artwork/<hash[..2]>/<hash>-<size>.webp` (spec §3.9).
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::imageops::FilterType;
use protocol::Palette;

/// Cached sizes, in pixels (longest edge).
pub const SIZES: [u32; 3] = [64, 300, 1000];
const WEBP_QUALITY: f32 = 80.0;
const COVER_STEMS: [&str; 3] = ["cover", "folder", "front"];
const COVER_EXTS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

#[derive(Debug, thiserror::Error)]
#[error("artwork: {0}")]
pub struct ArtError(pub String);

pub fn path_for(dir: &Path, hash: &str, size: u32) -> PathBuf {
    dir.join(&hash[..2]).join(format!("{hash}-{size}.webp"))
}

pub fn remove_files(dir: &Path, hash: &str) {
    for size in SIZES {
        let _ = std::fs::remove_file(path_for(dir, hash, size));
    }
}

/// Content hash of the original image bytes: blake3, first 32 hex characters.
pub fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..32].to_string()
}

/// The smallest cached size that's at least `size`, or the largest.
pub fn nearest_size(size: u32) -> u32 {
    SIZES.into_iter().find(|s| *s >= size).unwrap_or(SIZES[SIZES.len() - 1])
}

/// Folder-cover priority of a file name (0 = cover, 1 = folder, 2 = front), case-insensitive.
pub fn cover_rank(file_name: &str) -> Option<usize> {
    let lower = file_name.to_ascii_lowercase();
    let (stem, ext) = lower.rsplit_once('.')?;
    if !COVER_EXTS.contains(&ext) { return None; }
    COVER_STEMS.iter().position(|s| *s == stem)
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(path.parent().expect("cache paths have a parent"))?;
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })
}

/// Decodes `bytes`, writes any missing cache sizes (never upscaling) and returns the hash and palette.
/// Safe to call concurrently for the same image: each writer renames its own temp file into place.
pub fn store(dir: &Path, bytes: &[u8]) -> Result<(String, Palette), ArtError> {
    let h = hash(bytes);
    let img = image::load_from_memory(bytes).map_err(|e| ArtError(e.to_string()))?;
    for size in SIZES {
        let path = path_for(dir, &h, size);
        if path.exists() { continue; }
        let scaled = if img.width().max(img.height()) > size { img.resize(size, size, FilterType::Lanczos3) } else { img.clone() };
        let rgb = scaled.to_rgb8();
        let webp = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height()).encode(WEBP_QUALITY);
        write_atomic(&path, &webp).map_err(|e| ArtError(format!("{}: {e}", path.display())))?;
    }
    let thumb = img.thumbnail(64, 64).to_rgb8();
    Ok((h, crate::palette::extract(thumb.as_raw())))
}
