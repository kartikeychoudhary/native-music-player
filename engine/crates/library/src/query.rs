//! List and detail queries (spec §4.3 `library.*`). ORDER BY clauses come only from fixed strings
//! selected by `SortKey`; user values are always bound parameters.
use std::collections::HashMap;

use protocol::*;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OptionalExtension};

use crate::rows::{self, ALBUM_COLS, ALBUM_FROM, ARTIST_COLS, GENRE_COLS, TRACK_COLS};
use crate::{LibError, Library, Result};

pub const DEFAULT_LIMIT: u32 = 100;
pub const MAX_LIMIT: u32 = 1000;

/// WHERE clauses with their bound values, in order.
#[derive(Default)]
struct Filter { clauses: Vec<String>, values: Vec<Value> }

impl Filter {
    fn new(base: &str) -> Self { Filter { clauses: vec![base.to_string()], values: vec![] } }
    fn add(&mut self, clause: &str, values: impl IntoIterator<Item = Value>) {
        self.clauses.push(clause.to_string());
        self.values.extend(values);
    }
    fn sql(&self) -> String { self.clauses.join(" AND ") }
}

fn like(text: &str) -> Value {
    let escaped = text.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    Value::Text(format!("%{escaped}%"))
}

fn text_of(f: &ListFilter) -> Option<&str> { f.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) }

fn bounds(q: &ListQuery) -> (i64, i64) {
    (q.offset.unwrap_or(0) as i64, q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as i64)
}

fn dir(q: &ListQuery) -> &'static str { if q.desc.unwrap_or(false) { "DESC" } else { "ASC" } }

fn page<T>(conn: &Connection, select: &str, from: &str, f: &Filter, order: &str, q: &ListQuery, map: fn(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Page<T>> {
    let (offset, limit) = bounds(q);
    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {from} WHERE {}", f.sql()), params_from_iter(f.values.iter()), |r| r.get(0))?;
    let mut values = f.values.clone();
    values.extend([Value::Integer(limit), Value::Integer(offset)]);
    let items = conn
        .prepare(&format!("SELECT {select} FROM {from} WHERE {} ORDER BY {order} LIMIT ? OFFSET ?", f.sql()))?
        .query_map(params_from_iter(values.iter()), map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Page { items, total: total as u64 })
}

const TRACK_NUMBER_ORDER: &str = "COALESCE(t.disc_no, 1), t.track_no IS NULL, t.track_no, t.title COLLATE NOCASE";

fn track_order(q: &ListQuery, by_album: bool) -> String {
    let d = dir(q);
    match q.sort.unwrap_or(if by_album { SortKey::TrackNumber } else { SortKey::Title }) {
        SortKey::Artist => format!("t.artist COLLATE NOCASE {d}, t.album COLLATE NOCASE, {TRACK_NUMBER_ORDER}"),
        SortKey::Album => format!("t.album COLLATE NOCASE {d}, {TRACK_NUMBER_ORDER}"),
        SortKey::Year => format!("t.year IS NULL, t.year {d}, t.album COLLATE NOCASE, {TRACK_NUMBER_ORDER}"),
        SortKey::AddedAt => format!("t.added_at {d}, t.id"),
        SortKey::Duration => format!("t.duration_ms {d}, t.id"),
        SortKey::PlayCount => format!("t.play_count {d}, t.last_played_at {d}, t.id"),
        SortKey::TrackNumber => format!("{TRACK_NUMBER_ORDER}, t.id"),
        SortKey::Title | SortKey::Name => format!("t.title COLLATE NOCASE {d}, t.id"),
    }
}

impl Library {
    pub fn albums(&self, q: &ListQuery) -> Result<Page<AlbumRow>> {
        let f = q.filter.clone().unwrap_or_default();
        let mut w = Filter::new("a.track_count > 0");
        if let Some(t) = text_of(&f) { w.add("(a.title LIKE ? ESCAPE '\\' OR ar.name LIKE ? ESCAPE '\\')", [like(t), like(t)]); }
        if let Some(id) = f.artist_id {
            w.add(
                "(a.artist_id = ? OR EXISTS (SELECT 1 FROM album_artists aa WHERE aa.album_id = a.id AND aa.artist_id = ?)
                  OR EXISTS (SELECT 1 FROM tracks t JOIN track_artists ta ON ta.track_id = t.id
                             WHERE t.album_id = a.id AND ta.artist_id = ? AND t.missing_since IS NULL))",
                [Value::Integer(id), Value::Integer(id), Value::Integer(id)],
            );
        }
        if let Some(id) = f.genre_id {
            w.add("EXISTS (SELECT 1 FROM tracks t JOIN track_genres tg ON tg.track_id = t.id WHERE t.album_id = a.id AND tg.genre_id = ? AND t.missing_since IS NULL)", [Value::Integer(id)]);
        }
        if let Some(id) = f.folder_id {
            w.add("EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id AND t.folder_id = ?)", [Value::Integer(id)]);
        }
        let d = dir(q);
        let order = match q.sort.unwrap_or(SortKey::Title) {
            SortKey::Artist => format!("ar.sort_name {d}, a.year IS NULL, a.year, a.title COLLATE NOCASE, a.id"),
            SortKey::Year => format!("a.year IS NULL, a.year {d}, a.title COLLATE NOCASE, a.id"),
            SortKey::AddedAt => format!("a.added_at {d}, a.id"),
            SortKey::Duration => format!("a.duration_ms {d}, a.id"),
            _ => format!("a.title COLLATE NOCASE {d}, a.id"),
        };
        page(&self.conn(), ALBUM_COLS, ALBUM_FROM, &w, &order, q, rows::album)
    }

    pub fn artists(&self, q: &ListQuery) -> Result<Page<ArtistRow>> {
        let f = q.filter.clone().unwrap_or_default();
        let mut w = Filter::new("(ar.track_count > 0 OR ar.album_count > 0)");
        if let Some(t) = text_of(&f) { w.add("ar.name LIKE ? ESCAPE '\\'", [like(t)]); }
        if let Some(id) = f.genre_id {
            w.add(
                "EXISTS (SELECT 1 FROM track_artists ta JOIN track_genres tg ON tg.track_id = ta.track_id JOIN tracks t ON t.id = ta.track_id
                         WHERE ta.artist_id = ar.id AND tg.genre_id = ? AND t.missing_since IS NULL)",
                [Value::Integer(id)],
            );
        }
        let d = dir(q);
        let order = match q.sort {
            Some(SortKey::PlayCount) | Some(SortKey::TrackNumber) => format!("ar.track_count {d}, ar.sort_name"),
            _ => format!("ar.sort_name {d}, ar.id"),
        };
        page(&self.conn(), ARTIST_COLS, "artists ar", &w, &order, q, rows::artist)
    }

    pub fn genres(&self, q: &ListQuery) -> Result<Page<GenreRow>> {
        let f = q.filter.clone().unwrap_or_default();
        let mut w = Filter::new("g.track_count > 0");
        if let Some(t) = text_of(&f) { w.add("g.name LIKE ? ESCAPE '\\'", [like(t)]); }
        let order = format!("g.name COLLATE NOCASE {}, g.id", dir(q));
        page(&self.conn(), GENRE_COLS, "genres g", &w, &order, q, rows::genre)
    }

    pub fn tracks(&self, q: &ListQuery) -> Result<Page<TrackRow>> {
        let f = q.filter.clone().unwrap_or_default();
        let mut w = Filter::new("t.missing_since IS NULL");
        if let Some(t) = text_of(&f) {
            w.add("(t.title LIKE ? ESCAPE '\\' OR t.artist LIKE ? ESCAPE '\\' OR t.album LIKE ? ESCAPE '\\')", [like(t), like(t), like(t)]);
        }
        if let Some(id) = f.album_id { w.add("t.album_id = ?", [Value::Integer(id)]); }
        // IN (subquery) lets SQLite start from the small artist/genre index instead of scanning tracks.
        if let Some(id) = f.artist_id { w.add("t.id IN (SELECT track_id FROM track_artists WHERE artist_id = ?)", [Value::Integer(id)]); }
        if let Some(id) = f.genre_id { w.add("t.id IN (SELECT track_id FROM track_genres WHERE genre_id = ?)", [Value::Integer(id)]); }
        if let Some(id) = f.folder_id { w.add("t.folder_id = ?", [Value::Integer(id)]); }
        let order = track_order(q, f.album_id.is_some());
        page(&self.conn(), TRACK_COLS, "tracks t", &w, &order, q, rows::track)
    }

    fn named(conn: &Connection, sql: &str, id: i64) -> Result<Vec<NamedRef>> {
        Ok(conn.prepare(sql)?.query_map([id], |r| Ok(NamedRef { id: r.get(0)?, name: r.get(1)? }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn album(&self, id: i64) -> Result<AlbumDetail> {
        let conn = self.conn();
        let album = conn
            .query_row(&format!("SELECT {ALBUM_COLS} FROM {ALBUM_FROM} WHERE a.id = ?1"), [id], rows::album)
            .optional()?
            .ok_or_else(|| LibError::NotFound(format!("album {id}")))?;
        let tracks = conn
            .prepare(&format!("SELECT {TRACK_COLS} FROM tracks t WHERE t.album_id = ?1 AND t.missing_since IS NULL ORDER BY {TRACK_NUMBER_ORDER}, t.id"))?
            .query_map([id], rows::track)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let genres = Self::named(&conn,
            "SELECT DISTINCT g.id, g.name FROM genres g JOIN track_genres tg ON tg.genre_id = g.id JOIN tracks t ON t.id = tg.track_id
             WHERE t.album_id = ?1 AND t.missing_since IS NULL ORDER BY g.name COLLATE NOCASE", id)?;
        Ok(AlbumDetail { album, tracks, genres })
    }

    pub fn artist(&self, id: i64) -> Result<ArtistDetail> {
        let artist = self.conn()
            .query_row(&format!("SELECT {ARTIST_COLS} FROM artists ar WHERE ar.id = ?1"), [id], rows::artist)
            .optional()?
            .ok_or_else(|| LibError::NotFound(format!("artist {id}")))?;
        let albums = self.albums(&ListQuery {
            filter: Some(ListFilter { artist_id: Some(id), ..Default::default() }),
            sort: Some(SortKey::Year), desc: Some(true), offset: None, limit: Some(MAX_LIMIT),
        })?.items;
        let top_tracks = self.tracks(&ListQuery {
            filter: Some(ListFilter { artist_id: Some(id), ..Default::default() }),
            sort: Some(SortKey::PlayCount), desc: Some(true), offset: None, limit: Some(10),
        })?.items;
        Ok(ArtistDetail { artist, albums, top_tracks })
    }

    pub fn genre(&self, id: i64) -> Result<GenreRow> {
        self.conn()
            .query_row(&format!("SELECT {GENRE_COLS} FROM genres g WHERE g.id = ?1"), [id], rows::genre)
            .optional()?
            .ok_or_else(|| LibError::NotFound(format!("genre {id}")))
    }

    pub fn track(&self, id: i64) -> Result<TrackDetail> {
        let conn = self.conn();
        let row = conn
            .query_row(
                &format!("SELECT {TRACK_COLS}, t.composer, t.track_total, t.disc_total, t.rg_track_gain, t.rg_track_peak,
                                 t.rg_album_gain, t.rg_album_peak, t.mb_recording_id, t.mb_release_id, t.mb_artist_id,
                                 t.has_lyrics, t.size, t.mtime
                          FROM tracks t WHERE t.id = ?1"),
                [id],
                |r| {
                    Ok(TrackDetail {
                        track: rows::track(r)?,
                        artists: vec![],
                        genres: vec![],
                        composer: r.get(25)?,
                        track_total: r.get(26)?,
                        disc_total: r.get(27)?,
                        replay_gain: ReplayGainInfo { track_gain: r.get(28)?, track_peak: r.get(29)?, album_gain: r.get(30)?, album_peak: r.get(31)? },
                        musicbrainz: MusicBrainzIds { recording: r.get(32)?, release: r.get(33)?, artist: r.get(34)? },
                        has_lyrics: r.get(35)?,
                        size_bytes: r.get::<_, i64>(36)?.max(0) as u64,
                        mtime: r.get(37)?,
                        gapless: None,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| LibError::NotFound(format!("track {id}")))?;
        let artists = Self::named(&conn, "SELECT ar.id, ar.name FROM track_artists ta JOIN artists ar ON ar.id = ta.artist_id WHERE ta.track_id = ?1 ORDER BY ta.position", id)?;
        let genres = Self::named(&conn, "SELECT g.id, g.name FROM track_genres tg JOIN genres g ON g.id = tg.genre_id WHERE tg.track_id = ?1 ORDER BY g.name COLLATE NOCASE", id)?;
        Ok(TrackDetail { artists, genres, ..row })
    }

    /// Rows for `ids` in the given order (duplicates allowed). Any unknown id is an error.
    pub fn tracks_by_ids(&self, ids: &[i64]) -> Result<Vec<TrackRow>> {
        let conn = self.conn();
        let mut found: HashMap<i64, TrackRow> = HashMap::new();
        let mut unique: Vec<i64> = ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        for chunk in unique.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut stmt = conn.prepare(&format!("SELECT {TRACK_COLS} FROM tracks t WHERE t.id IN ({marks})"))?;
            for row in stmt.query_map(params_from_iter(chunk.iter()), rows::track)? {
                let row = row?;
                found.insert(row.id, row);
            }
        }
        ids.iter().map(|id| found.get(id).cloned().ok_or_else(|| LibError::NotFound(format!("track {id}")))).collect()
    }

    pub fn track_id_for_path(&self, path: &str) -> Option<i64> {
        self.conn().query_row("SELECT id FROM tracks WHERE path = ?1", [path], |r| r.get(0)).optional().ok().flatten()
    }
}
