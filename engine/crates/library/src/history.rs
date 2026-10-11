//! Play history (spec §3.9 auto views) and per-track playability marks (spec §6).
use protocol::{RecentKind, RecentResult};
use rusqlite::{params, TransactionBehavior};

use crate::rows::{self, ALBUM_COLS, ALBUM_FROM, TRACK_COLS};
use crate::{LibError, Library, Result};

impl Library {
    pub fn record_play(&self, track_id: i64, at: i64) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = tx.execute("UPDATE tracks SET play_count = play_count + 1, last_played_at = ?2 WHERE id = ?1", params![track_id, at])?;
        if n == 0 { return Err(LibError::NotFound(format!("track {track_id}"))); }
        tx.execute("INSERT INTO play_history(track_id, played_at) VALUES (?1, ?2)", params![track_id, at])?;
        tx.commit()?;
        Ok(())
    }

    pub fn recent(&self, kind: RecentKind, limit: u32) -> Result<RecentResult> {
        let limit = limit.clamp(1, 200) as i64;
        let conn = self.conn();
        let mut out = RecentResult::default();
        match kind {
            RecentKind::Added => {
                out.albums = conn
                    .prepare(&format!("SELECT {ALBUM_COLS} FROM {ALBUM_FROM} WHERE a.track_count > 0 ORDER BY a.added_at DESC, a.id DESC LIMIT ?1"))?
                    .query_map([limit], rows::album)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
            RecentKind::Played => {
                out.tracks = conn
                    .prepare(&format!(
                        "SELECT {TRACK_COLS} FROM tracks t
                         JOIN (SELECT track_id, MAX(played_at) AS at FROM play_history GROUP BY track_id) h ON h.track_id = t.id
                         WHERE t.missing_since IS NULL ORDER BY h.at DESC, t.id LIMIT ?1"
                    ))?
                    .query_map([limit], rows::track)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
            RecentKind::MostPlayed => {
                out.tracks = conn
                    .prepare(&format!(
                        "SELECT {TRACK_COLS} FROM tracks t WHERE t.play_count > 0 AND t.missing_since IS NULL
                         ORDER BY t.play_count DESC, t.last_played_at DESC, t.id LIMIT ?1"
                    ))?
                    .query_map([limit], rows::track)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
        }
        Ok(out)
    }

    /// Records (or clears, with `None`) why the file at `path` can't be played. Returns whether a
    /// library track has that path (files opened from outside the library have none).
    pub fn set_unplayable(&self, path: &str, reason: Option<&str>) -> Result<bool> {
        Ok(self.conn().execute("UPDATE tracks SET unplayable = ?2 WHERE path = ?1", params![path, reason])? > 0)
    }
}
