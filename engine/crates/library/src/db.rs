use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{LibError, Result};

const MIGRATIONS: &[&str] = &[include_str!("schema/001_init.sql"), include_str!("schema/002_live_track_indexes.sql")];
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;
const MIRROR: &str = "folders.json";

/// Opens `path` with Aurora's connection settings. Doesn't migrate.
pub fn connect(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        tracing::warn!(mode, "SQLite refused WAL journal mode");
    }
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(LibError::NewerSchema { found: version, supported: SCHEMA_VERSION });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

fn try_open(path: &Path) -> Result<Connection> {
    let mut conn = connect(path)?;
    let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(LibError::Corrupt);
    }
    migrate(&mut conn)?;
    Ok(conn)
}

pub struct Opened { pub conn: Connection, pub recovered_from: Option<PathBuf> }

/// Opens and migrates `path`, creating it if needed. A damaged file is moved aside to
/// `<name>.corrupt-<unix secs>` and recreated (spec §6). A database from a newer Aurora, or one that
/// merely can't be opened (locked, no permission), is reported and left alone.
pub fn open(path: &Path) -> Result<Opened> {
    match try_open(path) {
        Ok(conn) => Ok(Opened { conn, recovered_from: None }),
        Err(e) if e.is_corruption() && path.exists() => {
            tracing::error!(error = %e, path = %path.display(), "library database is damaged; moving it aside");
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "library.db".into());
            let aside = path.with_file_name(format!("{name}.corrupt-{}", crate::unix_now()));
            std::fs::rename(path, &aside)?;
            for ext in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(path.with_file_name(format!("{name}{ext}")));
            }
            Ok(Opened { conn: try_open(path)?, recovered_from: Some(aside) })
        }
        Err(e) => Err(e),
    }
}

/// Recomputes the denormalized counts that list queries read (albums, artists, genres).
pub fn refresh_stats(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "UPDATE albums SET
           track_count = (SELECT COUNT(*) FROM tracks t WHERE t.album_id = albums.id AND t.missing_since IS NULL),
           duration_ms = (SELECT COALESCE(SUM(t.duration_ms), 0) FROM tracks t WHERE t.album_id = albums.id AND t.missing_since IS NULL),
           codec = (SELECT t.codec FROM tracks t WHERE t.album_id = albums.id AND t.missing_since IS NULL AND t.codec IS NOT NULL
                    GROUP BY t.codec ORDER BY COUNT(*) DESC, t.codec LIMIT 1),
           max_sample_rate = (SELECT MAX(t.sample_rate) FROM tracks t WHERE t.album_id = albums.id AND t.missing_since IS NULL),
           max_bit_depth = (SELECT MAX(t.bit_depth) FROM tracks t WHERE t.album_id = albums.id AND t.missing_since IS NULL);
         UPDATE artists SET
           track_count = (SELECT COUNT(*) FROM track_artists ta JOIN tracks t ON t.id = ta.track_id
                          WHERE ta.artist_id = artists.id AND t.missing_since IS NULL),
           album_count = (SELECT COUNT(*) FROM albums a WHERE a.artist_id = artists.id AND a.track_count > 0),
           artwork_id = (SELECT a.artwork_id FROM albums a WHERE a.artist_id = artists.id AND a.track_count > 0
                         AND a.artwork_id IS NOT NULL ORDER BY a.year DESC, a.id LIMIT 1);
         UPDATE genres SET
           track_count = (SELECT COUNT(*) FROM track_genres tg JOIN tracks t ON t.id = tg.track_id
                          WHERE tg.genre_id = genres.id AND t.missing_since IS NULL),
           album_count = (SELECT COUNT(DISTINCT t.album_id) FROM track_genres tg JOIN tracks t ON t.id = tg.track_id
                          WHERE tg.genre_id = genres.id AND t.missing_since IS NULL);",
    )?;
    Ok(())
}

/// Deletes albums, artists, genres and artwork rows nothing refers to any more. Returns the hashes
/// of deleted artwork so the caller can delete the cached files.
pub fn cleanup_orphans(conn: &Connection) -> Result<Vec<String>> {
    conn.execute_batch(
        "DELETE FROM albums WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = albums.id);
         DELETE FROM artists WHERE NOT EXISTS (SELECT 1 FROM track_artists WHERE artist_id = artists.id)
           AND NOT EXISTS (SELECT 1 FROM album_artists WHERE artist_id = artists.id)
           AND NOT EXISTS (SELECT 1 FROM albums WHERE artist_id = artists.id);
         DELETE FROM genres WHERE NOT EXISTS (SELECT 1 FROM track_genres WHERE genre_id = genres.id);",
    )?;
    const UNUSED: &str = "NOT EXISTS (SELECT 1 FROM tracks WHERE artwork_id = artwork.id)
                          AND NOT EXISTS (SELECT 1 FROM albums WHERE artwork_id = artwork.id)";
    let hashes = conn
        .prepare(&format!("SELECT hash FROM artwork WHERE {UNUSED}"))?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    conn.execute(&format!("DELETE FROM artwork WHERE {UNUSED}"), [])?;
    Ok(hashes)
}

pub fn read_folder_mirror(dir: &Path) -> Vec<String> {
    std::fs::read(dir.join(MIRROR)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn write_folder_mirror(dir: &Path, folders: &[String]) -> Result<()> {
    let tmp = dir.join(format!("{MIRROR}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(folders).expect("strings serialize"))?;
    std::fs::rename(tmp, dir.join(MIRROR))?;
    Ok(())
}
