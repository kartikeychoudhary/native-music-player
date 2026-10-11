use std::path::{Path, PathBuf};

/// Cached sizes, in pixels (longest edge).
pub const SIZES: [u32; 3] = [64, 300, 1000];

pub fn path_for(dir: &Path, hash: &str, size: u32) -> PathBuf {
    dir.join(&hash[..2]).join(format!("{hash}-{size}.webp"))
}

pub fn remove_files(dir: &Path, hash: &str) {
    for size in SIZES {
        let _ = std::fs::remove_file(path_for(dir, hash, size));
    }
}
