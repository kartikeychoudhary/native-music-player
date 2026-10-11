use library::{db, LibError, Library};
use rusqlite::Connection;

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) { (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()) }

#[test]
fn creates_schema_with_wal_and_fts() {
    let (data, _) = dirs();
    let (lib, report) = Library::open(data.path()).unwrap();
    assert!(report.recovered_from.is_none());
    let conn = lib.conn();
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, db::SCHEMA_VERSION);
    conn.execute("INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, composer) VALUES (1, 'Café del Mar', 'X', 'Y', NULL, NULL)", []).unwrap();
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH '\"cafe\"*'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "remove_diacritics folds é");
    assert!(data.path().join("artwork").is_dir());
}

#[test]
fn reopen_keeps_folders() {
    let (data, music) = dirs();
    {
        let (lib, _) = Library::open(data.path()).unwrap();
        lib.add_folder(music.path()).unwrap();
    }
    let (lib, _) = Library::open(data.path()).unwrap();
    let folders = lib.folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert!(folders[0].available);
    assert_eq!(folders[0].track_count, 0);
}

#[test]
fn corrupt_db_is_moved_aside_and_folders_restored() {
    let (data, music) = dirs();
    {
        let (lib, _) = Library::open(data.path()).unwrap();
        lib.add_folder(music.path()).unwrap();
    }
    let db_path = data.path().join("library.db");
    std::fs::write(&db_path, b"definitely not a sqlite database".repeat(200)).unwrap();
    for ext in ["library.db-wal", "library.db-shm"] { let _ = std::fs::remove_file(data.path().join(ext)); }

    let (lib, report) = Library::open(data.path()).unwrap();
    let aside = report.recovered_from.expect("recovered");
    assert!(aside.file_name().unwrap().to_string_lossy().starts_with("library.db.corrupt-"));
    assert!(aside.exists());
    assert_eq!(report.restored_folders.len(), 1);
    assert_eq!(lib.folders().unwrap().len(), 1);
}

#[test]
fn newer_schema_is_refused_and_untouched() {
    let (data, _) = dirs();
    drop(Library::open(data.path()).unwrap());
    let db_path = data.path().join("library.db");
    Connection::open(&db_path).unwrap().pragma_update(None, "user_version", 99).unwrap();

    let err = Library::open(data.path()).err().expect("refused");
    assert!(matches!(err, LibError::NewerSchema { found: 99, .. }), "{err}");
    let v: i64 = Connection::open(&db_path).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 99);
    let moved = std::fs::read_dir(data.path()).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("corrupt"));
    assert!(!moved);
}

#[test]
fn add_folder_rejects_overlaps_and_missing_paths() {
    let (data, music) = dirs();
    let (lib, _) = Library::open(data.path()).unwrap();
    let sub = music.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    lib.add_folder(&sub).unwrap();
    assert!(matches!(lib.add_folder(music.path()), Err(LibError::InvalidArgument(_))), "parent of an existing folder");
    assert!(matches!(lib.add_folder(&sub.join(".")), Err(LibError::InvalidArgument(_))), "same folder");
    assert!(matches!(lib.add_folder(&music.path().join("nope")), Err(LibError::InvalidArgument(_))));
}

fn seed_track(conn: &Connection, folder: i64, path: &str, album: i64, missing: Option<i64>) -> i64 {
    conn.execute(
        "INSERT INTO tracks(folder_id, path, mtime, size, title, artist, album_id, album, container, added_at, missing_since, duration_ms, artwork_id)
         VALUES (?1, ?2, 0, 0, 't', 'A', ?3, 'Al', 'flac', 0, ?4, 1000, (SELECT id FROM artwork LIMIT 1))",
        rusqlite::params![folder, path, album, missing],
    ).unwrap();
    conn.last_insert_rowid()
}

#[test]
fn remove_folder_cascades_and_cleans_orphans() {
    let (data, music) = dirs();
    let (lib, _) = Library::open(data.path()).unwrap();
    let f = lib.add_folder(music.path()).unwrap();
    let art_file = library::artwork::path_for(&lib.artwork_dir(), "abcdef0123456789abcdef0123456789", 64);
    std::fs::create_dir_all(art_file.parent().unwrap()).unwrap();
    std::fs::write(&art_file, b"x").unwrap();
    {
        let conn = lib.conn();
        conn.execute("INSERT INTO artwork(hash) VALUES ('abcdef0123456789abcdef0123456789')", []).unwrap();
        conn.execute("INSERT INTO artists(name, sort_name) VALUES ('A', 'a')", []).unwrap();
        conn.execute("INSERT INTO albums(title, artist_id, group_key, added_at) VALUES ('Al', 1, 'a\u{1f}al', 0)", []).unwrap();
        let t = seed_track(&conn, f.id, "/m/1.flac", 1, None);
        conn.execute("INSERT INTO track_artists VALUES (?1, 1, 0)", [t]).unwrap();
        conn.execute("INSERT INTO tracks_fts(rowid, title, artist, album) VALUES (?1, 't', 'A', 'Al')", [t]).unwrap();
    }
    lib.remove_folder(f.id).unwrap();
    assert!(lib.folders().unwrap().is_empty());
    let conn = lib.conn();
    for table in ["tracks", "albums", "artists", "artwork", "tracks_fts"] {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{table} emptied");
    }
    assert!(!art_file.exists(), "orphaned artwork files are deleted");
}

#[test]
fn refresh_stats_counts_present_tracks_only() {
    let (data, music) = dirs();
    let (lib, _) = Library::open(data.path()).unwrap();
    let f = lib.add_folder(music.path()).unwrap();
    let conn = lib.conn();
    conn.execute("INSERT INTO artists(name, sort_name) VALUES ('A', 'a')", []).unwrap();
    conn.execute("INSERT INTO albums(title, artist_id, group_key, added_at) VALUES ('Al', 1, 'k', 0)", []).unwrap();
    let a = seed_track(&conn, f.id, "/m/1.flac", 1, None);
    let b = seed_track(&conn, f.id, "/m/2.flac", 1, Some(5));
    conn.execute("INSERT INTO track_artists VALUES (?1, 1, 0), (?2, 1, 0)", [a, b]).unwrap();
    db::refresh_stats(&conn).unwrap();
    let (tc, dur): (i64, i64) = conn.query_row("SELECT track_count, duration_ms FROM albums WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((tc, dur), (1, 1000));
    let (atc, aac): (i64, i64) = conn.query_row("SELECT track_count, album_count FROM artists WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((atc, aac), (1, 1));
}

#[test]
fn folder_mirror_follows_add_and_remove() {
    let (data, music) = dirs();
    let (lib, _) = Library::open(data.path()).unwrap();
    let f = lib.add_folder(music.path()).unwrap();
    assert_eq!(db::read_folder_mirror(data.path()), vec![f.path.clone()]);
    lib.remove_folder(f.id).unwrap();
    assert!(db::read_folder_mirror(data.path()).is_empty());
}

#[test]
fn upgrading_removes_duplicate_playlist_entries() {
    let (data, music) = dirs();
    {
        let (lib, _) = Library::open(data.path()).unwrap();
        let f = lib.add_folder(music.path()).unwrap();
        let conn = lib.conn();
        conn.execute("INSERT INTO artists(name, sort_name) VALUES ('A', 'a')", []).unwrap();
        conn.execute("INSERT INTO albums(title, artist_id, group_key, added_at) VALUES ('Al', 1, 'k', 0)", []).unwrap();
        let t = seed_track(&conn, f.id, "/m/a.flac", 1, None);
        let u = seed_track(&conn, f.id, "/m/b.flac", 1, None);
        conn.execute("INSERT INTO playlists(name, created_at, updated_at) VALUES ('p', 0, 0)", []).unwrap();
        // A database from before the uniqueness rule, holding a duplicate.
        conn.execute_batch("DROP INDEX playlist_items_unique; PRAGMA user_version = 2;").unwrap();
        for (track, pos) in [(t, 0), (u, 1), (t, 2)] {
            conn.execute("INSERT INTO playlist_items(playlist_id, track_id, position) VALUES (1, ?1, ?2)", [track, pos]).unwrap();
        }
    }
    let (lib, _) = Library::open(data.path()).unwrap();
    let d = lib.playlist(1).unwrap();
    assert_eq!(d.playlist.track_count, 2);
    assert_eq!(d.items.iter().map(|i| i.track.path.as_str()).collect::<Vec<_>>(), ["/m/a.flac", "/m/b.flac"], "the first copy stays");
}
