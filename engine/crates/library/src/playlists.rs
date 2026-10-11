//! User playlists and M3U8 import/export (spec §3.9).
use std::path::{Component, Path, PathBuf};

use protocol::{ImportResult, Playlist, PlaylistDetail, PlaylistItem};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::{unix_now, LibError, Library, Result};

const MAX_NAME: usize = 200;

fn clean_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME {
        return Err(LibError::InvalidArgument(format!("playlist names are 1–{MAX_NAME} characters")));
    }
    Ok(n.to_string())
}

/// Decodes `%XX` escapes (as UTF-8) in a `file://` URL path.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolves `.` and `..` without touching the disk.
pub fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn summary(conn: &Connection, id: i64) -> Result<Playlist> {
    conn.query_row(
        "SELECT p.id, p.name, COUNT(t.id), COALESCE(SUM(t.duration_ms), 0), p.updated_at
         FROM playlists p LEFT JOIN playlist_items i ON i.playlist_id = p.id LEFT JOIN tracks t ON t.id = i.track_id
         WHERE p.id = ?1 GROUP BY p.id",
        [id],
        |r| Ok(Playlist { id: r.get(0)?, name: r.get(1)?, track_count: r.get(2)?, duration_ms: r.get::<_, i64>(3)?.max(0) as u64, updated_at: r.get(4)? }),
    )
    .optional()?
    .ok_or_else(|| LibError::NotFound(format!("playlist {id}")))
}

fn item_ids(tx: &Transaction, id: i64) -> Result<Vec<i64>> {
    Ok(tx
        .prepare("SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn renumber(tx: &Transaction, id: i64, items: &[i64]) -> Result<()> {
    let mut stmt = tx.prepare_cached("UPDATE playlist_items SET position = ?1 WHERE id = ?2")?;
    for (pos, item) in items.iter().enumerate() {
        stmt.execute(params![pos as i64, item])?;
    }
    tx.execute("UPDATE playlists SET updated_at = ?1 WHERE id = ?2", params![unix_now(), id])?;
    Ok(())
}

fn ensure(tx: &Transaction, id: i64) -> Result<()> {
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)", [id], |r| r.get(0))?;
    if exists { Ok(()) } else { Err(LibError::NotFound(format!("playlist {id}"))) }
}

impl Library {
    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let conn = self.conn();
        let ids = conn
            .prepare("SELECT id FROM playlists ORDER BY name COLLATE NOCASE, id")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter().map(|id| summary(&conn, id)).collect()
    }

    pub fn playlist(&self, id: i64) -> Result<PlaylistDetail> {
        let conn = self.conn();
        let playlist = summary(&conn, id)?;
        // (item id, track id) in playlist order; tracks_by_ids keeps that order.
        let items: Vec<(i64, i64)> = conn
            .prepare("SELECT id, track_id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id")?
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(conn);
        let tracks = self.tracks_by_ids(&items.iter().map(|(_, t)| *t).collect::<Vec<_>>())?;
        Ok(PlaylistDetail { playlist, items: items.into_iter().zip(tracks).map(|((item_id, _), track)| PlaylistItem { item_id, track }).collect() })
    }

    pub fn create_playlist(&self, name: &str) -> Result<Playlist> {
        let name = clean_name(name)?;
        let conn = self.conn();
        let now = unix_now();
        conn.execute("INSERT INTO playlists(name, created_at, updated_at) VALUES (?1, ?2, ?2)", params![name, now])?;
        summary(&conn, conn.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<Playlist> {
        let name = clean_name(name)?;
        let conn = self.conn();
        if conn.execute("UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, unix_now(), id])? == 0 {
            return Err(LibError::NotFound(format!("playlist {id}")));
        }
        summary(&conn, id)
    }

    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        let deleted = self.conn().execute("DELETE FROM playlists WHERE id = ?1", [id])?;
        if deleted == 0 {
            return Err(LibError::NotFound(format!("playlist {id}")));
        }
        Ok(())
    }

    pub fn add_to_playlist(&self, id: i64, track_ids: &[i64], index: Option<u32>) -> Result<Playlist> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure(&tx, id)?;
        let mut items = item_ids(&tx, id)?;
        let mut added = Vec::with_capacity(track_ids.len());
        for t in track_ids {
            let known: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tracks WHERE id = ?1)", [t], |r| r.get(0))?;
            if !known {
                return Err(LibError::NotFound(format!("track {t}")));
            }
            // A track is in a playlist at most once: one already there (or earlier in this call) is skipped.
            if tx.execute("INSERT OR IGNORE INTO playlist_items(playlist_id, track_id, position) VALUES (?1, ?2, 0)", params![id, t])? == 1 {
                added.push(tx.last_insert_rowid());
            }
        }
        let at = index.map_or(items.len(), |i| (i as usize).min(items.len()));
        items.splice(at..at, added);
        renumber(&tx, id, &items)?;
        tx.commit()?;
        summary(&conn, id)
    }

    pub fn remove_from_playlist(&self, id: i64, item_ids_to_remove: &[i64]) -> Result<Playlist> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure(&tx, id)?;
        for chunk in item_ids_to_remove.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut values: Vec<i64> = vec![id];
            values.extend_from_slice(chunk);
            tx.execute(&format!("DELETE FROM playlist_items WHERE playlist_id = ? AND id IN ({marks})"), params_from_iter(values.iter()))?;
        }
        let items = item_ids(&tx, id)?;
        renumber(&tx, id, &items)?;
        tx.commit()?;
        summary(&conn, id)
    }

    pub fn reorder_playlist(&self, id: i64, from: u32, to: u32) -> Result<Playlist> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure(&tx, id)?;
        let mut items = item_ids(&tx, id)?;
        let (from, to) = (from as usize, to as usize);
        if from >= items.len() || to >= items.len() {
            return Err(LibError::InvalidArgument(format!("positions must be below {}", items.len())));
        }
        let moved = items.remove(from);
        items.insert(to, moved);
        renumber(&tx, id, &items)?;
        tx.commit()?;
        summary(&conn, id)
    }

    /// Finds the library track an M3U entry points at: relative entries resolve against the
    /// playlist's folder; `file://` URLs are decoded; either slash style is accepted.
    fn resolve_m3u_entry(&self, entry: &str, base: &Path) -> Option<i64> {
        let raw = match entry.strip_prefix("file://") {
            Some(rest) => {
                let decoded = percent_decode(rest);
                let b = decoded.as_bytes();
                // file:///C:/x -> C:/x on Windows-style paths.
                if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { decoded[1..].to_string() } else { decoded }
            }
            None => entry.to_string(),
        };
        let native = if cfg!(windows) { raw.replace('/', "\\") } else { raw.replace('\\', "/") };
        let p = PathBuf::from(native);
        let abs = lexical_normalize(&if p.is_absolute() { p } else { base.join(p) });
        let key = abs.to_string_lossy().into_owned();
        self.track_id_for_path(&key).or_else(|| {
            if !cfg!(windows) {
                return None;
            }
            // Windows paths are case-insensitive; the DB key keeps whatever case the scan saw.
            let found = self.conn().query_row("SELECT id FROM tracks WHERE path = ?1 COLLATE NOCASE", [&key], |r| r.get(0)).optional();
            found.ok().flatten()
        })
    }

    pub fn import_m3u(&self, path: &Path) -> Result<ImportResult> {
        let bytes = std::fs::read(path).map_err(|_| LibError::FileMissing(path.display().to_string()))?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let base = crate::normalize_root(&base).unwrap_or(base);
        let mut ids = vec![];
        let mut skipped = 0;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            match self.resolve_m3u_entry(line, &base) {
                Some(id) => ids.push(id),
                None => skipped += 1,
            }
        }
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported".into());
        let created = self.create_playlist(&name)?;
        let playlist = if ids.is_empty() { created } else { self.add_to_playlist(created.id, &ids, None)? };
        Ok(ImportResult { playlist, skipped })
    }

    pub fn export_m3u(&self, id: i64, path: &Path) -> Result<()> {
        let detail = self.playlist(id)?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let dir = crate::normalize_root(&dir).unwrap_or(dir);
        let mut out = format!("#EXTM3U\n#PLAYLIST:{}\n", detail.playlist.name);
        for item in &detail.items {
            let t = &item.track;
            let secs = t.duration_ms.map_or(-1, |ms| (ms / 1000) as i64);
            let label = if t.artist.is_empty() { t.title.clone() } else { format!("{} - {}", t.artist, t.title) };
            let p = Path::new(&t.path);
            let shown = p.strip_prefix(&dir).map(Path::to_path_buf).unwrap_or_else(|_| p.to_path_buf());
            out.push_str(&format!("#EXTINF:{secs},{label}\n{}\n", shown.display()));
        }
        let tmp = path.with_extension("m3u8.tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("/a%20b/%E6%97%A5.flac"), "/a b/日.flac");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn lexical_normalization() {
        assert_eq!(lexical_normalize(Path::new("/m/list/../A/./x.flac")), PathBuf::from("/m/A/x.flac"));
        assert_eq!(lexical_normalize(Path::new("a/../../b")), PathBuf::from("b"));
    }
}
