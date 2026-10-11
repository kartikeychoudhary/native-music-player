mod common;
use library::artwork;

fn dims(path: &std::path::Path) -> (u32, u32) {
    let img = image::open(path).unwrap();
    (img.width(), img.height())
}

#[test]
fn store_writes_three_downscaled_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = common::png(1500, 1000, [200, 50, 50]);
    let (hash, palette) = artwork::store(dir.path(), &bytes).unwrap();
    assert_eq!(hash.len(), 32);
    assert_eq!(hash, artwork::hash(&bytes));
    assert_eq!(dims(&artwork::path_for(dir.path(), &hash, 1000)), (1000, 667));
    assert_eq!(dims(&artwork::path_for(dir.path(), &hash, 300)), (300, 200));
    assert_eq!(dims(&artwork::path_for(dir.path(), &hash, 64)), (64, 43));
    assert_eq!(palette.dominant, "#c83232");
}

#[test]
fn small_images_are_not_upscaled() {
    let dir = tempfile::tempdir().unwrap();
    let (hash, _) = artwork::store(dir.path(), &common::png(50, 40, [1, 2, 3])).unwrap();
    for size in artwork::SIZES {
        assert_eq!(dims(&artwork::path_for(dir.path(), &hash, size)), (50, 40));
    }
}

#[test]
fn storing_twice_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = common::png(400, 400, [9, 9, 9]);
    let (a, _) = artwork::store(dir.path(), &bytes).unwrap();
    let modified = std::fs::metadata(artwork::path_for(dir.path(), &a, 300)).unwrap().modified().unwrap();
    let (b, _) = artwork::store(dir.path(), &bytes).unwrap();
    assert_eq!(a, b);
    assert_eq!(std::fs::metadata(artwork::path_for(dir.path(), &a, 300)).unwrap().modified().unwrap(), modified, "existing files are kept");
}

#[test]
fn garbage_bytes_are_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(artwork::store(dir.path(), b"not an image").is_err());
}

#[test]
fn cover_file_priority() {
    assert_eq!(artwork::cover_rank("Cover.JPG"), Some(0));
    assert_eq!(artwork::cover_rank("folder.png"), Some(1));
    assert_eq!(artwork::cover_rank("front.webp"), Some(2));
    assert_eq!(artwork::cover_rank("front.jpeg"), Some(2));
    assert_eq!(artwork::cover_rank("back.jpg"), None);
    assert_eq!(artwork::cover_rank("cover.gif"), None);
    assert_eq!(artwork::cover_rank("cover"), None);
}

#[test]
fn nearest_cached_size() {
    assert_eq!(artwork::nearest_size(10), 64);
    assert_eq!(artwork::nearest_size(64), 64);
    assert_eq!(artwork::nearest_size(65), 300);
    assert_eq!(artwork::nearest_size(5000), 1000);
}

#[test]
fn artwork_info_resolves_the_nearest_cached_size_and_palette() {
    use common::{tagged, Tags};
    use lofty::picture::PictureType;
    let blue = common::png(400, 400, [0, 0, 255]);
    let s = common::scanned(|root| {
        tagged("flac16_44k.flac", &root.join("a.flac"), &Tags { album: Some("A"), pictures: &[(PictureType::CoverFront, &blue)], ..Default::default() });
    });
    let id: i64 = s.lib.conn().query_row("SELECT id FROM artwork", [], |r| r.get(0)).unwrap();
    let info = s.lib.artwork_info(id, 200).unwrap();
    assert!(info.path.ends_with("-300.webp"), "{}", info.path);
    assert!(std::path::Path::new(&info.path).exists());
    assert_eq!(info.palette.unwrap().dominant, "#0000ff");
    assert!(matches!(s.lib.artwork_info(9999, 64), Err(library::LibError::NotFound(_))));
}
