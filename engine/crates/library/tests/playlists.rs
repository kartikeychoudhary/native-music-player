mod common;
use common::{demo_tree, scanned, tagged, Tags};
use library::LibError;

fn titles(d: &protocol::PlaylistDetail) -> Vec<String> { d.items.iter().map(|i| i.track.title.clone()).collect() }

#[test]
fn create_rename_list_delete() {
    let s = scanned(demo_tree);
    assert!(matches!(s.lib.create_playlist("   "), Err(LibError::InvalidArgument(_))));
    let b = s.lib.create_playlist("beta mix").unwrap();
    let a = s.lib.create_playlist(" Alpha Mix ").unwrap();
    assert_eq!(a.name, "Alpha Mix");
    let names: Vec<_> = s.lib.playlists().unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["Alpha Mix", "beta mix"]);
    assert_eq!(s.lib.rename_playlist(b.id, "Zed").unwrap().name, "Zed");
    s.lib.delete_playlist(a.id).unwrap();
    assert_eq!(s.lib.playlists().unwrap().len(), 1);
    assert!(matches!(s.lib.playlist(a.id), Err(LibError::NotFound(_))));
}

#[test]
fn add_insert_reorder_remove_keep_positions_dense() {
    let s = scanned(demo_tree);
    let p = s.lib.create_playlist("Mix").unwrap();
    let ids: Vec<i64> = ["Dawn", "Noon", "Peak"].iter().map(|t| s.id_of(t)).collect();
    s.lib.add_to_playlist(p.id, &ids, None).unwrap();
    let summary = s.lib.add_to_playlist(p.id, &[s.id_of("Span")], Some(1)).unwrap();
    assert_eq!(summary.track_count, 4);
    assert_eq!(titles(&s.lib.playlist(p.id).unwrap()), ["Dawn", "Span", "Noon", "Peak"]);
    s.lib.reorder_playlist(p.id, 0, 3).unwrap();
    let d = s.lib.playlist(p.id).unwrap();
    assert_eq!(titles(&d), ["Span", "Noon", "Peak", "Dawn"]);
    s.lib.remove_from_playlist(p.id, &[d.items[1].item_id]).unwrap();
    assert_eq!(titles(&s.lib.playlist(p.id).unwrap()), ["Span", "Peak", "Dawn"]);
    let positions: Vec<i64> = {
        let conn = s.lib.conn();
        let mut stmt = conn.prepare("SELECT position FROM playlist_items WHERE playlist_id = ?1 ORDER BY position").unwrap();
        stmt.query_map([p.id], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(positions, [0, 1, 2]);
    assert!(matches!(s.lib.reorder_playlist(p.id, 0, 9), Err(LibError::InvalidArgument(_))));
    assert!(matches!(s.lib.add_to_playlist(p.id, &[999_999], None), Err(LibError::NotFound(_))));
    let dup = s.lib.add_to_playlist(p.id, &[s.id_of("Span")], Some(99)).unwrap();
    assert_eq!(dup.track_count, 4, "duplicates allowed; index past the end appends");
}

#[test]
fn missing_tracks_stay_in_playlists() {
    let s = scanned(demo_tree);
    let p = s.lib.create_playlist("Night").unwrap();
    s.lib.add_to_playlist(p.id, &[s.id_of("Dusk")], None).unwrap();
    std::fs::remove_file(s.root().join("The Gamma/Night/01.mp3")).unwrap();
    s.rescan();
    let d = s.lib.playlist(p.id).unwrap();
    assert_eq!(d.items.len(), 1);
    assert!(d.items[0].track.missing);
}

fn file_url(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let enc: String = s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"/._-:".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    if cfg!(windows) { format!("file:///{enc}") } else { format!("file://{enc}") }
}

#[test]
fn import_resolves_relative_unicode_and_file_urls() {
    let s = scanned(|root| {
        demo_tree(root);
        tagged("flac16_44k.flac", &root.join("Beyoncé/日本語.flac"), &Tags { title: Some("日本語"), artists: &["Beyoncé"], ..Default::default() });
    });
    let root = s.root();
    let peak = s.lib.tracks_by_ids(&[s.id_of("Peak")]).unwrap()[0].path.clone();
    let m3u = root.join("playlists").join("Road Trip.m3u8");
    std::fs::create_dir_all(m3u.parent().unwrap()).unwrap();
    let body = format!(
        "\u{feff}#EXTM3U\n#EXTINF:1,Alpha - Dawn\n../Alpha/First Light/01.flac\n..\\Beta\\Bridges\\01.flac\n{}\n/does/not/exist.flac\n\n{}\n",
        file_url(&root.join("Beyoncé").join("日本語.flac")),
        peak,
    );
    std::fs::write(&m3u, body).unwrap();
    let r = s.lib.import_m3u(&m3u).unwrap();
    assert_eq!(r.playlist.name, "Road Trip");
    assert_eq!(r.skipped, 1);
    assert_eq!(titles(&s.lib.playlist(r.playlist.id).unwrap()), ["Dawn", "Span", "日本語", "Peak"]);
}

#[test]
fn export_then_import_round_trips() {
    let s = scanned(demo_tree);
    let p = s.lib.create_playlist("Round").unwrap();
    s.lib.add_to_playlist(p.id, &[s.id_of("Noon"), s.id_of("Dusk")], None).unwrap();
    let out = s.root().join("Round.m3u8");
    s.lib.export_m3u(p.id, &out).unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("#EXTM3U\n#PLAYLIST:Round\n"));
    assert!(text.contains("#EXTINF:"));
    assert!(text.contains("Alpha, Beta - Noon"));
    assert!(!text.contains(&s.root().to_string_lossy().to_string()), "paths inside the playlist's folder are relative");
    let r = s.lib.import_m3u(&out).unwrap();
    assert_eq!(r.skipped, 0);
    assert_eq!(titles(&s.lib.playlist(r.playlist.id).unwrap()), ["Noon", "Dusk"]);
}

#[test]
fn import_of_a_missing_file_is_file_missing() {
    let s = scanned(demo_tree);
    assert!(matches!(s.lib.import_m3u(&s.root().join("nope.m3u8")), Err(LibError::FileMissing(_))));
}

/// Playlist edits read, then write, on the shared connection. While the scanner (another
/// connection) holds the write lock and commits, they must wait for it, not fail with "database is locked".
#[test]
fn playlist_edits_wait_for_another_writer() {
    let s = scanned(demo_tree);
    let p = s.lib.create_playlist("Mix").unwrap();
    let (dawn, noon) = (s.id_of("Dawn"), s.id_of("Noon"));
    s.lib.add_to_playlist(p.id, &[dawn], None).unwrap();
    let edits: [(&str, &dyn Fn() -> library::Result<protocol::Playlist>); 3] = [
        ("add_to_playlist", &|| s.lib.add_to_playlist(p.id, &[noon], None)),
        ("reorder_playlist", &|| s.lib.reorder_playlist(p.id, 0, 1)),
        ("remove_from_playlist", &|| { let first = s.lib.playlist(p.id)?.items[0].item_id; s.lib.remove_from_playlist(p.id, &[first]) }),
    ];
    for (name, edit) in edits {
        let other = library::db::connect(&s.lib.db_path()).unwrap();
        other.execute_batch("BEGIN IMMEDIATE; UPDATE folders SET last_scan_at = 1;").unwrap();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            other.execute_batch("COMMIT").unwrap();
        });
        let r = edit();
        writer.join().unwrap();
        r.unwrap_or_else(|e| panic!("{name} failed while another connection was writing: {e}"));
    }
    assert_eq!(s.lib.playlist(p.id).unwrap().items.len(), 1);
}
