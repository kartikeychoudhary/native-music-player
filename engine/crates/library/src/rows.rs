//! Shared SQL column lists and row mappers, so every query returns identical protocol rows.
use protocol::{AlbumRow, ArtistRow, GenreRow, TrackRow};
use rusqlite::Row;

use crate::codec;

pub const TRACK_COLS: &str = "t.id, t.path, t.title, t.artist,
  (SELECT ta.artist_id FROM track_artists ta WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1),
  t.album, t.album_id, t.album_artist, t.track_no, t.disc_no, t.year, t.genre, t.duration_ms, t.codec, t.container,
  t.bitrate_kbps, t.sample_rate, t.bit_depth, t.channels, t.artwork_id, t.missing_since IS NOT NULL, t.unplayable,
  t.play_count, t.added_at, t.last_played_at";

pub fn track(r: &Row) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?, path: r.get(1)?, title: r.get(2)?, artist: r.get(3)?, artist_id: r.get(4)?, album: r.get(5)?,
        album_id: r.get(6)?, album_artist: r.get(7)?, track_no: r.get(8)?, disc_no: r.get(9)?, year: r.get(10)?,
        genre: r.get(11)?, duration_ms: r.get::<_, Option<i64>>(12)?.map(|d| d.max(0) as u64),
        codec: r.get::<_, Option<String>>(13)?.as_deref().and_then(codec::parse), container: r.get(14)?,
        bitrate_kbps: r.get(15)?, sample_rate: r.get(16)?, bit_depth: r.get(17)?, channels: r.get(18)?,
        artwork_id: r.get(19)?, missing: r.get(20)?, unplayable: r.get(21)?, play_count: r.get(22)?,
        added_at: r.get(23)?, last_played_at: r.get(24)?,
    })
}

pub const ALBUM_FROM: &str = "albums a LEFT JOIN artists ar ON ar.id = a.artist_id";

pub const ALBUM_COLS: &str = "a.id, a.title,
  COALESCE((SELECT group_concat(x.name, ', ' ORDER BY aa.position) FROM album_artists aa JOIN artists x ON x.id = aa.artist_id
            WHERE aa.album_id = a.id), ''),
  a.artist_id, a.year, a.track_count, a.duration_ms, a.artwork_id, a.added_at, a.codec, a.max_sample_rate, a.max_bit_depth";

pub fn album(r: &Row) -> rusqlite::Result<AlbumRow> {
    Ok(AlbumRow {
        id: r.get(0)?, title: r.get(1)?, artist: r.get(2)?, artist_id: r.get(3)?, year: r.get(4)?, track_count: r.get(5)?,
        duration_ms: r.get::<_, i64>(6)?.max(0) as u64, artwork_id: r.get(7)?, added_at: r.get(8)?,
        codec: r.get::<_, Option<String>>(9)?.as_deref().and_then(codec::parse), max_sample_rate: r.get(10)?, max_bit_depth: r.get(11)?,
    })
}

pub const ARTIST_COLS: &str = "ar.id, ar.name, ar.album_count, ar.track_count, ar.artwork_id";

pub fn artist(r: &Row) -> rusqlite::Result<ArtistRow> {
    Ok(ArtistRow { id: r.get(0)?, name: r.get(1)?, album_count: r.get(2)?, track_count: r.get(3)?, artwork_id: r.get(4)? })
}

pub const GENRE_COLS: &str = "g.id, g.name, g.album_count, g.track_count";

pub fn genre(r: &Row) -> rusqlite::Result<GenreRow> {
    Ok(GenreRow { id: r.get(0)?, name: r.get(1)?, album_count: r.get(2)?, track_count: r.get(3)? })
}
