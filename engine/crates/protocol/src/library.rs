use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::{Codec, GaplessSummary};

/// One page of a list query plus the total number of matching rows.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Page<T> { pub items: Vec<T>, pub total: u64 }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub track_count: u64,
    pub last_scan_at: Option<i64>,
    /// False while the folder's root can't be read (e.g. an unplugged drive).
    pub available: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackRow {
    pub id: i64,
    pub path: String,
    pub title: String,
    /// Display string: native multi-value artists joined with ", ".
    pub artist: String,
    /// First credited artist (for "Go to artist").
    pub artist_id: Option<i64>,
    pub album: String,
    pub album_id: Option<i64>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub duration_ms: Option<u64>,
    pub codec: Option<Codec>,
    pub container: String,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u16>,
    pub artwork_id: Option<i64>,
    pub missing: bool,
    /// Error name (e.g. "UNSUPPORTED_FORMAT") when the file failed to play or scan.
    pub unplayable: Option<String>,
    pub play_count: u32,
    pub added_at: i64,
    pub last_played_at: Option<i64>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NamedRef { pub id: i64, pub name: String }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReplayGainInfo {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MusicBrainzIds { pub recording: Option<String>, pub release: Option<String>, pub artist: Option<String> }

/// Everything the track-info dialog shows.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackDetail {
    pub track: TrackRow,
    pub artists: Vec<NamedRef>,
    pub genres: Vec<NamedRef>,
    pub composer: Option<String>,
    pub track_total: Option<u32>,
    pub disc_total: Option<u32>,
    pub replay_gain: ReplayGainInfo,
    pub musicbrainz: MusicBrainzIds,
    pub has_lyrics: bool,
    pub size_bytes: u64,
    pub mtime: i64,
    /// Encoder delay/padding, probed from the file when the dialog asks (None if unknown or unreadable).
    pub gapless: Option<GaplessSummary>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlbumRow {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<i64>,
    pub year: Option<i32>,
    pub track_count: u32,
    pub duration_ms: u64,
    pub artwork_id: Option<i64>,
    pub added_at: i64,
    /// Most common codec on the album.
    pub codec: Option<Codec>,
    pub max_sample_rate: Option<u32>,
    pub max_bit_depth: Option<u16>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlbumDetail { pub album: AlbumRow, pub tracks: Vec<TrackRow>, pub genres: Vec<NamedRef> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ArtistRow { pub id: i64, pub name: String, pub album_count: u32, pub track_count: u32, pub artwork_id: Option<i64> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ArtistDetail { pub artist: ArtistRow, pub albums: Vec<AlbumRow>, pub top_tracks: Vec<TrackRow> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenreRow { pub id: i64, pub name: String, pub album_count: u32, pub track_count: u32 }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SortKey { Title, Name, Artist, Album, Year, AddedAt, Duration, PlayCount, TrackNumber }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ListFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub artist_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub album_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub genre_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub folder_id: Option<i64>,
}

/// `{ filter?, sort?, desc?, offset?, limit? }` (spec §4.3). Defaults live in the library crate.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub filter: Option<ListFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub sort: Option<SortKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub desc: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub offset: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchResults { pub tracks: Vec<TrackRow>, pub albums: Vec<AlbumRow>, pub artists: Vec<ArtistRow> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Palette { pub dominant: String, pub vibrant: String, pub muted: String }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ArtworkInfo { pub path: String, pub palette: Option<Palette> }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScanPhase { Walking, Reading, Finishing, Done }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanProgress {
    pub folder_id: i64,
    pub phase: ScanPhase,
    pub processed: u64,
    pub total: u64,
    pub added: u64,
    pub updated: u64,
    pub missing: u64,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LibraryScope { Tracks, Folders, Playlists, History }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryChanged { pub scopes: Vec<LibraryScope> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Playlist { pub id: i64, pub name: String, pub track_count: u32, pub duration_ms: u64, pub updated_at: i64 }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistItem { pub item_id: i64, pub track: TrackRow }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistDetail { pub playlist: Playlist, pub items: Vec<PlaylistItem> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportResult { pub playlist: Playlist, pub skipped: u32 }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecentKind { Added, Played, MostPlayed }

/// `added` fills `albums`; `played` and `mostPlayed` fill `tracks`.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentResult { pub albums: Vec<AlbumRow>, pub tracks: Vec<TrackRow> }
