mod common;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use common::{png, tagged, Tags};
use library::scan::{ScanOptions, ScanSummary};
use library::tags::AudioProps;
use library::Library;
use lofty::picture::PictureType;
use protocol::{Codec, ScanPhase};
use rusqlite::Connection;

struct Fx { _data: tempfile::TempDir, _music: tempfile::TempDir, lib: Library, folder: i64 }

fn setup() -> Fx {
    let data = tempfile::tempdir().unwrap();
    let music = tempfile::tempdir().unwrap();
    let (lib, _) = Library::open(data.path()).unwrap();
    let folder = lib.add_folder(music.path()).unwrap().id;
    Fx { _data: data, _music: music, lib, folder }
}

fn root(fx: &Fx) -> PathBuf { PathBuf::from(&fx.lib.folder(fx.folder).unwrap().path) }

fn build_tree(root: &Path) {
    tagged("flac16_44k.flac", &root.join("Alpha/First Light/01.flac"), &Tags {
        title: Some("Dawn"), artists: &["Alpha"], album: Some("First Light"), track: Some(1), date: Some("2001"), genres: &["Ambient"], ..Default::default()
    });
    tagged("flac16_44k.flac", &root.join("Alpha/First Light/02.flac"), &Tags {
        title: Some("Noon"), artists: &["Alpha", "Beta"], album: Some("First Light"), album_artist: Some("Alpha"), track: Some(2), ..Default::default()
    });
    tagged("mp3_320.mp3", &root.join("Gamma/Night/01.mp3"), &Tags {
        title: Some("Dusk"), artists: &["Gamma"], album: Some("Night"), track: Some(1), genres: &["Jazz"], ..Default::default()
    });
}

fn scan_with(fx: &Fx, roots: &[PathBuf], full: bool, opts: ScanOptions) -> ScanSummary {
    fx.lib.scanner(opts).unwrap().scan(fx.folder, roots, full, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}
fn scan(fx: &Fx) -> ScanSummary { scan_with(fx, &[], true, ScanOptions::default()) }

fn count(conn: &Connection, sql: &str) -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() }

fn touch_later(p: &Path) {
    std::fs::File::options().write(true).open(p).unwrap().set_modified(SystemTime::now() + Duration::from_secs(10)).unwrap();
}

#[test]
fn first_scan_indexes_tracks_albums_artists_genres() {
    let fx = setup();
    build_tree(&root(&fx));
    let s = scan(&fx);
    assert_eq!((s.added, s.updated, s.unchanged), (3, 0, 0));
    let conn = fx.lib.conn();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks"), 3);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM albums"), 2, "01 (no album artist) and 02 (album artist Alpha) group together");
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM artists"), 3);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM genres"), 2);
    assert_eq!(count(&conn, "SELECT track_count FROM albums WHERE title = 'First Light'"), 2);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH '\"dus\"*'"), 1);
    let artist: String = conn.query_row("SELECT artist FROM tracks WHERE title = 'Noon'", [], |r| r.get(0)).unwrap();
    assert_eq!(artist, "Alpha, Beta");
    let codec: String = conn.query_row("SELECT codec FROM tracks WHERE title = 'Dusk'", [], |r| r.get(0)).unwrap();
    assert_eq!(codec, "mp3");
    drop(conn);
    let f = fx.lib.folder(fx.folder).unwrap();
    assert_eq!(f.track_count, 3);
    assert!(f.last_scan_at.is_some());
}

#[test]
fn second_scan_skips_unchanged_files() {
    let fx = setup();
    build_tree(&root(&fx));
    scan(&fx);
    let s = scan(&fx);
    assert_eq!((s.added, s.updated, s.unchanged, s.missing), (0, 0, 3, 0));
}

#[test]
fn changed_file_is_reread_and_reindexed() {
    let fx = setup();
    let r = root(&fx);
    build_tree(&r);
    scan(&fx);
    let p = r.join("Alpha/First Light/01.flac");
    tagged("flac16_44k.flac", &p, &Tags { title: Some("Sunrise"), artists: &["Alpha"], album: Some("First Light"), track: Some(1), ..Default::default() });
    touch_later(&p);
    let s = scan(&fx);
    assert_eq!((s.added, s.updated, s.unchanged), (0, 1, 2));
    let conn = fx.lib.conn();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH '\"sunrise\"'"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH '\"dawn\"'"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM genres WHERE name = 'Ambient'"), 0, "genre orphaned by the retag is cleaned up");
}

#[test]
fn deleted_file_is_kept_as_missing_then_purged_after_30_days() {
    let fx = setup();
    let r = root(&fx);
    build_tree(&r);
    scan(&fx);
    std::fs::remove_file(r.join("Gamma/Night/01.mp3")).unwrap();
    let t0 = library::unix_now();
    let s = scan_with(&fx, &[], true, ScanOptions { now: Some(t0), ..Default::default() });
    assert_eq!(s.missing, 1);
    {
        let conn = fx.lib.conn();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks WHERE missing_since IS NOT NULL"), 1);
        assert_eq!(count(&conn, "SELECT track_count FROM albums WHERE title = 'Night'"), 0, "hidden from counts");
    }
    let s = scan_with(&fx, &[], true, ScanOptions { now: Some(t0 + 29 * 86400), ..Default::default() });
    assert_eq!(s.purged, 0);
    let s = scan_with(&fx, &[], true, ScanOptions { now: Some(t0 + 31 * 86400), ..Default::default() });
    assert_eq!(s.purged, 1);
    let conn = fx.lib.conn();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks"), 2);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM albums"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM artists WHERE name = 'Gamma'"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM genres WHERE name = 'Jazz'"), 0);
}

#[test]
fn unmounted_root_marks_missing_and_remount_restores() {
    let fx = setup();
    let r = root(&fx);
    build_tree(&r);
    scan(&fx);
    {
        let conn = fx.lib.conn();
        conn.execute_batch("INSERT INTO playlists(name, created_at, updated_at) VALUES ('Mix', 0, 0);
                            INSERT INTO playlist_items(playlist_id, track_id, position) SELECT 1, id, 0 FROM tracks LIMIT 1;").unwrap();
    }
    let unplugged = r.with_file_name(format!("{}-unplugged", r.file_name().unwrap().to_string_lossy()));
    std::fs::rename(&r, &unplugged).unwrap();
    assert!(!fx.lib.folder(fx.folder).unwrap().available);
    let s = scan(&fx);
    assert_eq!(s.missing, 3);
    assert_eq!(count(&fx.lib.conn(), "SELECT COUNT(*) FROM tracks"), 3, "nothing deleted");
    std::fs::rename(&unplugged, &r).unwrap();
    let s = scan(&fx);
    assert_eq!(s.restored, 3);
    let conn = fx.lib.conn();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks WHERE missing_since IS NOT NULL"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM playlist_items"), 1, "playlists survive the unplug");
}

#[test]
fn embedded_art_beats_folder_cover_and_images_are_deduplicated() {
    let fx = setup();
    let album = root(&fx).join("Art/Album");
    let blue = png(32, 32, [0, 0, 255]);
    let red = png(32, 32, [255, 0, 0]);
    std::fs::create_dir_all(&album).unwrap();
    std::fs::write(album.join("Cover.png"), &red).unwrap();
    std::fs::write(album.join("front.png"), png(8, 8, [0, 255, 0])).unwrap();
    for name in ["01.flac", "02.flac"] {
        tagged("flac16_44k.flac", &album.join(name), &Tags { album: Some("Album"), artists: &["A"], pictures: &[(PictureType::CoverFront, &blue)], ..Default::default() });
    }
    tagged("flac16_44k.flac", &album.join("03.flac"), &Tags { album: Some("Album"), artists: &["A"], ..Default::default() });
    scan(&fx);
    let conn = fx.lib.conn();
    let hash_of = |file: &str| -> String {
        conn.query_row("SELECT a.hash FROM tracks t JOIN artwork a ON a.id = t.artwork_id WHERE t.path LIKE ?1", [format!("%{file}")], |r| r.get(0)).unwrap()
    };
    assert_eq!(hash_of("01.flac"), library::artwork::hash(&blue));
    assert_eq!(hash_of("02.flac"), library::artwork::hash(&blue));
    assert_eq!(hash_of("03.flac"), library::artwork::hash(&red), "cover.* beats front.*");
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM artwork"), 2);
    let album_art: String = conn.query_row("SELECT a.hash FROM albums al JOIN artwork a ON a.id = al.artwork_id", [], |r| r.get(0)).unwrap();
    assert_eq!(album_art, library::artwork::hash(&blue));
    assert!(library::artwork::path_for(&fx.lib.artwork_dir(), &album_art, 300).exists());
}

#[test]
fn unreadable_file_is_indexed_as_unplayable() {
    let fx = setup();
    let r = root(&fx);
    std::fs::copy(common::fixture("empty.mp3"), r.join("empty.mp3")).unwrap();
    assert_eq!(scan(&fx).added, 1);
    let (title, unplayable): (String, Option<String>) =
        fx.lib.conn().query_row("SELECT title, unplayable FROM tracks", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(title, "empty");
    assert_eq!(unplayable.as_deref(), Some("UNSUPPORTED_FORMAT"));
}

#[test]
fn fallback_probe_fills_properties_for_dsd() {
    let fx = setup();
    let r = root(&fx);
    std::fs::copy(common::fixture("dsd64.dsf"), r.join("dsd64.dsf")).unwrap();
    let fallback: library::scan::Fallback = Arc::new(|p: &Path| {
        Some(AudioProps { duration_ms: Some(1234), codec: Some(Codec::Dsd), container: library::tags::container_of(p), sample_rate: Some(2_822_400), channels: Some(2), ..Default::default() })
    });
    scan_with(&fx, &[], true, ScanOptions { fallback: Some(fallback), ..Default::default() });
    let (title, codec, dur, unplayable): (String, String, i64, Option<String>) = fx.lib.conn()
        .query_row("SELECT title, codec, duration_ms, unplayable FROM tracks", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    assert_eq!((title.as_str(), codec.as_str(), dur, unplayable), ("dsd64", "dsd", 1234, None));
}

#[test]
fn partial_scan_only_touches_given_roots() {
    let fx = setup();
    let r = root(&fx);
    build_tree(&r);
    scan(&fx);
    std::fs::remove_file(r.join("Gamma/Night/01.mp3")).unwrap();
    let p = r.join("Alpha/First Light/01.flac");
    tagged("flac16_44k.flac", &p, &Tags { title: Some("Changed"), artists: &["Alpha"], album: Some("First Light"), ..Default::default() });
    touch_later(&p);
    let s = scan_with(&fx, std::slice::from_ref(&p), false, ScanOptions::default());
    assert_eq!((s.updated, s.missing), (1, 0), "the deleted Gamma file is out of scope");
    let s = scan_with(&fx, &[r.join("Gamma/Night")], false, ScanOptions::default());
    assert_eq!(s.missing, 1);
    let s = scan_with(&fx, &[r.join("Gamma")], false, ScanOptions::default());
    assert_eq!(s.missing, 0, "already missing");
}

#[test]
fn progress_reports_phases_in_order() {
    let fx = setup();
    build_tree(&root(&fx));
    let mut phases = vec![];
    let mut last = None;
    fx.lib.scanner(ScanOptions::default()).unwrap()
        .scan(fx.folder, &[], true, &mut |p| { phases.push(p.phase); last = Some(p.clone()); }, &AtomicBool::new(false)).unwrap();
    assert_eq!(phases.first(), Some(&ScanPhase::Walking));
    assert!(phases.contains(&ScanPhase::Reading));
    let last = last.unwrap();
    assert_eq!(last.phase, ScanPhase::Done);
    assert_eq!((last.processed, last.total, last.added), (3, 3, 3));
}

#[test]
fn cancelled_scan_stops() {
    let fx = setup();
    build_tree(&root(&fx));
    let err = fx.lib.scanner(ScanOptions::default()).unwrap().scan(fx.folder, &[], true, &mut |_| {}, &AtomicBool::new(true)).unwrap_err();
    assert!(matches!(err, library::LibError::Cancelled));
}

/// Makes `dir` unlistable until dropped (so the scan's `read_dir` of it fails). `None` when the
/// platform has no way to do that here.
struct Locked(PathBuf);

impl Locked {
    #[cfg(unix)]
    fn new(dir: &Path) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).ok()?;
        // root ignores mode bits, so the lock would be a no-op.
        if std::fs::read_dir(dir).is_ok() { std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).ok()?; return None; }
        Some(Locked(dir.to_path_buf()))
    }
    #[cfg(windows)]
    fn new(dir: &Path) -> Option<Self> {
        let user = std::env::var("USERNAME").ok()?;
        let ok = std::process::Command::new("icacls").arg(dir).arg("/deny").arg(format!("{user}:(RD)")).output().ok()?.status.success();
        if !ok { return None; }
        let locked = Locked(dir.to_path_buf());
        if std::fs::read_dir(dir).is_ok() { return None; }
        Some(locked)
    }
    #[cfg(not(any(unix, windows)))]
    fn new(_: &Path) -> Option<Self> { None }
}

impl Drop for Locked {
    fn drop(&mut self) {
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755)); }
        #[cfg(windows)]
        { let user = std::env::var("USERNAME").unwrap_or_default(); let _ = std::process::Command::new("icacls").arg(&self.0).arg("/remove:d").arg(user).output(); }
    }
}

#[test]
fn unreadable_folder_is_not_marked_missing() {
    let fx = setup();
    let r = root(&fx);
    build_tree(&r);
    scan(&fx);
    let Some(lock) = Locked::new(&r.join("Gamma")) else { eprintln!("can't make a folder unreadable here; skipping"); return };
    let s = scan(&fx);
    assert_eq!((s.missing, s.unchanged), (0, 2), "Gamma/Night/01.mp3 is unknown, not gone");
    drop(lock);
    assert_eq!(count(&fx.lib.conn(), "SELECT COUNT(*) FROM tracks WHERE missing_since IS NOT NULL"), 0);
    assert_eq!(scan(&fx).unchanged, 3);
}
