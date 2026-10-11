//! Instant search (spec §5.3): FTS5 prefix queries grouped into tracks, albums and artists.
use protocol::SearchResults;
use rusqlite::params;

use crate::rows::{self, ALBUM_COLS, ALBUM_FROM, ARTIST_COLS, TRACK_COLS};
use crate::{Library, Result};

/// An FTS5 expression that prefix-matches every word of `text` (AND), optionally inside a column
/// set such as `"album album_artist"`. Words are runs of alphanumeric characters, so quotes and FTS
/// operators in user input can never reach the query.
pub fn fts_query(text: &str, columns: Option<&str>) -> Option<String> {
    let tokens: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.to_lowercase()))
        .collect();
    if tokens.is_empty() { return None; }
    Some(match columns {
        Some(cols) => tokens.iter().map(|t| format!("{{{cols}}} : {t}")).collect::<Vec<_>>().join(" AND "),
        None => tokens.join(" AND "),
    })
}

impl Library {
    pub fn search(&self, text: &str, limit: u32) -> Result<SearchResults> {
        let text = text.trim();
        let limit = limit.clamp(1, 100) as i64;
        let Some(all) = fts_query(text, None).filter(|_| text.chars().count() >= 2) else { return Ok(SearchResults::default()) };
        let by_album = fts_query(text, Some("album album_artist")).expect("same tokens as `all`");
        let starts = format!("{}%", text.replace(['%', '_', '\\'], ""));
        let conn = self.conn();
        let tracks = conn
            .prepare(&format!(
                "SELECT {TRACK_COLS} FROM tracks_fts f JOIN tracks t ON t.id = f.rowid
                 WHERE tracks_fts MATCH ?1 AND t.missing_since IS NULL
                 ORDER BY bm25(tracks_fts, 10.0, 5.0, 3.0, 3.0, 1.0), t.id LIMIT ?2"
            ))?
            .query_map(params![all, limit], rows::track)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let albums = conn
            .prepare(&format!(
                "SELECT {ALBUM_COLS} FROM {ALBUM_FROM}
                 WHERE a.track_count > 0 AND a.id IN (
                   SELECT t.album_id FROM tracks_fts f JOIN tracks t ON t.id = f.rowid
                   WHERE tracks_fts MATCH ?1 AND t.missing_since IS NULL)
                 ORDER BY a.title LIKE ?3 DESC, a.title COLLATE NOCASE, a.id LIMIT ?2"
            ))?
            .query_map(params![by_album, limit, starts], rows::album)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let artists = conn
            .prepare(&format!(
                "SELECT {ARTIST_COLS} FROM artists_fts f JOIN artists ar ON ar.id = f.rowid
                 WHERE artists_fts MATCH ?1 AND (ar.track_count > 0 OR ar.album_count > 0)
                 ORDER BY bm25(artists_fts), ar.track_count DESC, ar.id LIMIT ?2"
            ))?
            .query_map(params![all, limit], rows::artist)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(SearchResults { tracks, albums, artists })
    }
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn builds_prefix_and_queries() {
        assert_eq!(fts_query("Hello, wor", None).as_deref(), Some("\"hello\"* AND \"wor\"*"));
        assert_eq!(fts_query("ab", Some("album album_artist")).as_deref(), Some("{album album_artist} : \"ab\"*"));
        assert_eq!(fts_query("\"*- {}", None), None);
        assert_eq!(fts_query("\" OR *", None).as_deref(), Some("\"or\"*"), "operators are just quoted words");
        assert_eq!(fts_query("AC/DC", None).as_deref(), Some("\"ac\"* AND \"dc\"*"));
    }
}
