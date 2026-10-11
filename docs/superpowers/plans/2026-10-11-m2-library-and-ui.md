# Aurora M2 — Library + Full UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the music library to Aurora and replace the M1 shell with the full UI:
- **Engine:** a new `library` crate (scanner, watcher, SQLite + FTS5, artwork + palette, playlists + M3U8), a real queue (track IDs, add/insert/move/remove, shuffle, repeat), play history, session restore and OS media keys.
- **App:** the Studio UI with routed views (Home, Albums, Artists, Genres, Playlists, Search, Queue, Settings), context menus, a track-info dialog and the complete player bar.

**Architecture:**
- **Library crate:** owns `library.db` (WAL) and the `artwork/` cache. It knows nothing about audio decoding; the engine injects a fallback probe for formats `lofty` can't tag-read (DSF/DFF).
- **`aurora-engine` with `--data-dir`:** the binary opens the library and runs scans and the watcher on a library service thread. It persists `session.json` every 5 s and pushes now-playing to the OS through `souvlaki`.
- **Player queue:** moves into a pure, unit-tested `Queue` model. The player re-resolves every queue position it holds by entry ID after each edit. Edits that invalidate gapless pre-fed audio resync with a same-position seek.
- **Electron app:** main serves artwork through an `aurora-art://` protocol and keeps `settings.json`. The renderer moves to TanStack Router (hash history) with virtualized lists.

**Tech Stack:**
- Existing: Rust 1.99.0 (edition 2024).
- New Rust crates: rusqlite 0.40.2 (`bundled`, SQLite 3.53 with FTS5), lofty 0.25.4, jwalk 0.9.0, rayon 1.10, notify 8.2.0 + notify-debouncer-full 0.7.0, image 0.25.10, webp 0.3.1, blake3 1.8.7, souvlaki 0.8.3, tempfile 3.27.0.
- New npm packages: @tanstack/react-router 1.170.42, @tanstack/react-virtual 3.14.14, @dnd-kit/core 6.3.1, @dnd-kit/sortable 10.0.0, @dnd-kit/utilities 3.2.2.
- Everything from M1 is unchanged: Electron 44.7.0, React 19.3, Zustand 5, Vitest 5, Playwright 1.64.

**Spec:** `docs/superpowers/specs/2026-10-10-aurora-music-player-design.md`. This plan implements **Milestone 2** from §9; executors read both. M1 rulings and deferred findings are in `docs/superpowers/m1-execution-notes.md`.

## Global Constraints

### Crates, protocol and output
- **New crate:** `engine/crates/library` (publish = false), as in spec §3.1.
  - `aurora-engine` depends on it.
  - `library` must **not** depend on `engine-core`. The engine passes a `Fallback` probe closure in instead.
- **Protocol version:** bump to `PROTOCOL_VERSION = 2`. `PlayerState`, `TrackSummary` and `QueueItem` gain fields, so app and engine must match.
- **Protocol types:** every new wire type lives in `protocol` with `ts-rs` and is regenerated into `shared/protocol/`. Generated files are committed, and the CI staleness gate applies.
- **Output thread rule:** unchanged (no allocation, locks, I/O or logging in output callbacks).
- **stdout:** only `rpc/writer.rs` writes to it. Logs go to stderr.

### Engine persistence and library behaviour
- **Persistence requires `aurora-engine rpc --data-dir <dir>`.** Without the flag the engine behaves exactly as in M1:
  - no library;
  - no session;
  - `library.*`/`playlists.*`/`queue.set|add|insertNext` return `DB_ERROR` (-32008) with the message `library unavailable: engine started without --data-dir`.
- **Files in the data dir:**
  - `library.db` (SQLite, WAL);
  - `artwork/` (WebP cache);
  - `session.json`;
  - `folders.json`, a mirror of the folder list, used only to rescan after DB-corruption recovery.
- **Electron settings:** `settings.json` in Electron's `userData` dir (main process only).
- **DB rules:**
  - `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`.
  - Migrations are versioned through `PRAGMA user_version`.
  - A DB whose `user_version` is newer than the code is **refused, never deleted**.
  - Corruption (open fails or `quick_check` ≠ `ok`) moves the DB to `library.db.corrupt-<unix-secs>`, recreates it and rescans the folders from `folders.json` (spec §6).
- **Missing files:**
  - Marked with `missing_since` (unix seconds).
  - Hidden from library views.
  - Purged after **30 days** by a full scan (spec §3.9).
  - Playlist items of purged tracks disappear by `ON DELETE CASCADE`.
- **Watcher:** `notify-debouncer-full` with a **2 s** debounce (spec §3.9).
- **Artwork:**
  - Embedded front cover first, then `cover|folder|front.(jpg|jpeg|png|webp)` (case-insensitive, in that priority) in the track's folder.
  - Stored once per content hash (blake3, first 32 hex chars) at sizes **64, 300 and 1000 px** as lossy WebP (quality 80). Images are never upscaled.
  - A palette (`dominant`, `vibrant`, `muted` as `#rrggbb`) is stored in the `artwork` row.
- **Multi-value tags:** artists and genres are split **only** on the tag's native multi-value fields. There's no splitting on `&`, `,`, `feat.` (spec §3.9). The display string joins with `, `.
- **Album grouping key:** `lowercase(album artist, else first track artist) + U+001F + lowercase(album title)`. Track order is `disc_no` (missing = 1), then `track_no` (missing last), then title.
- **Performance targets** (spec §1.4), asserted by the Task 10 benchmark:
  - 100k-track first scan under 120 s on an SSD;
  - paging and search queries under 50 ms.
- **Play history:**
  - A track counts as played after **50 % of its duration or 4 minutes** of actual listening, whichever comes first.
  - Seeking doesn't count as listening.
  - It's counted once per play.

### App and UI
- **Studio tokens:** unchanged from M1 (`app/src/renderer/styles/tokens.css`).
  - Fonts are Inter (UI) and JetBrains Mono (formats, rates, timestamps, counts). Icons come from Lucide.
  - Trade-dress guardrails: no `#1DB954`-family green, no Circular/Gotham-like fonts, no circular green play button, no edge-to-edge `#121212`/`#181818`.
- **Renderer security:**
  - `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`.
  - CSP `img-src` gains exactly `aurora-art:`. No other CSP change.
- **Test-only environment variables** (`AURORA_TEST_*`, `AURORA_DATA_DIR`) are honoured only when `!app.isPackaged`.
- **UI copy:** one module, `app/src/renderer/lib/strings.ts` (spec §1 i18n assumption), for all user-visible text added in M2.

### Scope
- **Explicitly deferred to M3** (spec §9):
  - lyrics parsing and the `lyrics.get` method;
  - expanded Now Playing;
  - EQ/crossfade/ReplayGain UI and DSP;
  - the spectrum visualizer;
  - the mini-player window;
  - the tray;
  - the player bar's lyrics and mini-player buttons and the Ctrl/Cmd+L / Ctrl/Cmd+M shortcuts;
  - the accent-tint setting.
- **Kept in M2:**
  - the palette is extracted and the album-detail header uses its muted colour (spec §5.5 dynamic colour);
  - DSF/DFF files are indexed with file-name titles and probe-derived properties (tag reading for DSD containers is a known M2 limitation, recorded in the README).
- **Methods outside the spec table:** M2 keeps `queue.setPaths` from M1 for files opened or dropped from outside the library.

## Review Focus

These are inputs the spec implies but no feature test naturally exercises. Each line is pinned by the named test.

1. **A queue edit while the next track is already buffered for gapless playback** (the last ~2 s of a track) changes what plays next, with no stale audio from the old next track. Tests:
   - Task 12 `insert_next_during_prefeed_replaces_buffered_track`;
   - Task 12 `removing_prefed_next_resyncs`.
2. **A library folder on an unplugged drive:** every track is marked missing, nothing is deleted, playlists keep their items, and a rescan after remount clears `missing_since`. Test: Task 5 `unmounted_root_marks_missing_and_remount_restores`.
3. **Corrupt or newer-schema `library.db` at startup:**
   - a corrupt DB is moved aside and its folders are rescanned from `folders.json`;
   - a DB from a newer Aurora is refused and left untouched.

   Tests: Task 2 `corrupt_db_is_moved_aside_and_folders_restored`, `newer_schema_is_refused_and_untouched`.
4. **M3U8 files from other players:**
   - relative paths, `..` segments, backslashes, non-ASCII names and `file://` URLs resolve to library tracks;
   - unknown entries are skipped and counted, not fatal.

   Test: Task 8 `import_resolves_relative_unicode_and_file_urls`.
5. **A damaged session and removed files:**
   - a truncated `session.json` is ignored (the engine starts with an empty queue);
   - a restored queue whose current file was deleted reports `FILE_MISSING` and moves on when played.

   Tests: Task 14 `truncated_session_is_ignored`; Task 16 `restored_queue_skips_missing_file`.

---

## File Structure

```
engine/
  Cargo.toml                                  + library member, new workspace deps
  crates/protocol/src/library.rs              NEW: library/playlist/scan wire types, Page<T>
  crates/protocol/src/types.rs                RepeatMode; TrackSummary/PlayerState/QueueItem fields
  crates/protocol/src/rpc.rs                  queue/library/playlist params
  crates/protocol/src/lib.rs                  PROTOCOL_VERSION = 2, re-export library
  crates/library/                             NEW crate
    src/lib.rs                                Library facade (open, folders, re-exports)
    src/error.rs                              LibError → ErrorCode
    src/db.rs                                 open/configure/migrate/recover, folders.json mirror
    src/schema/001_init.sql                   schema v1
    src/tags.rs                               lofty → TrackTags/AudioProps
    src/artwork.rs                            hash, resize, WebP cache, folder cover lookup
    src/palette.rs                            k-means palette (pure)
    src/scan.rs                               walk → read → upsert, missing/purge, stats refresh
    src/codec.rs                              Codec ↔ DB text (wire names)
    src/rows.rs                               SQL column lists and Row → protocol mappers
    src/query.rs                              list/detail queries, tracks_by_ids
    src/search.rs                             FTS5 query building and grouped search
    src/history.rs                            play history, recent/most played, unplayable marks
    src/playlists.rs                          playlist CRUD + M3U8 import/export
    src/watch.rs                              debounced watcher
    tests/common/mod.rs                       fixture copying + tag writing helpers
    tests/{db,tags,artwork,scan,query,search,playlists,watch,bench_100k}.rs
  crates/engine-core/src/player/queue.rs      NEW: Queue model (entries, shuffle, repeat)
  crates/engine-core/src/player/mod.rs        queue commands, id remap, Played, Restore
  crates/engine-core/tests/queue_player.rs    NEW: player-level queue edit tests
  crates/aurora-engine/src/main.rs            `rpc --data-dir`
  crates/aurora-engine/src/rpc/mod.rs         wiring: library service, session, osmedia
  crates/aurora-engine/src/rpc/dispatch.rs    queue.* methods; routes library/playlists
  crates/aurora-engine/src/rpc/library_service.rs   NEW: scan worker + watcher + notifications
  crates/aurora-engine/src/rpc/library_methods.rs   NEW: library.* / playlists.* dispatch
  crates/aurora-engine/src/rpc/session.rs     NEW: session.json snapshot/restore
  crates/aurora-engine/src/osmedia/mod.rs     NEW: souvlaki thread + pure event mapping
  crates/aurora-engine/src/osmedia/win_window.rs    NEW: hidden HWND + message pump (Windows)
  crates/aurora-engine/tests/common/mod.rs    NEW: shared Engine harness (moved from rpc.rs)
  crates/aurora-engine/tests/library_rpc.rs   NEW: end-to-end library/queue/session tests
app/
  package.json                                + productName, router, virtual, dnd-kit
  vitest.config.ts                            + test/setup.ts
  src/shared/api.ts                           Methods/Events/allowlist (compile-time exhaustive), AuroraApi
  src/shared/settings.ts                      NEW: Settings type + mergeSettings validator
  src/main/index.ts                           --data-dir, aurora-art protocol, settings, IPC, graceful quit
  src/main/engineArgs.ts                      NEW: engine argv + data-dir resolution
  src/main/artProtocol.ts                     NEW: aurora-art:// URL parsing + handler
  src/main/settings.ts                        NEW: settings.json store, window-bounds check
  src/preload/index.ts                        new bridge calls
  src/renderer/index.html                     CSP img-src aurora-art:
  src/renderer/main.tsx                       stores + RouterProvider bootstrap, last-view restore
  src/renderer/router.tsx                     NEW: route tree (hash history)
  src/renderer/Layout.tsx (+ .module.css)     NEW: replaces App.tsx — titlebar, sidebar, outlet, bar, hosts
  src/renderer/lib/
    strings.ts  art.ts  nav.ts                NEW: UI copy, artwork URLs, sidebar entries
    useEngineQuery.ts  usePaged.ts  useGo.ts  NEW: data fetching, sparse paging, navigation
    format.ts  shortcuts.ts                   + library formats; + Ctrl/Cmd+F, Ctrl/Cmd+,, ↑/↓
  src/renderer/stores/
    player.ts                                 − panel; + playTracks/playNext/addToQueue/setShuffle/cycleRepeat
    library.ts  ui.ts                         NEW: folders/scan/playlists/versions; sidebar/toasts/menus/dialogs
  src/renderer/components/
    Sidebar.tsx (replaces IconRail.tsx)       rail ↔ 220 px sidebar + playlists section
    Artwork  ContextMenu  Toasts  Dialog  PromptDialog  ConfirmDialog  (ui.module.css)
    TrackTable  AlbumGrid (VirtualGrid, AlbumCard)  SortableList  trackMenu.ts  TrackInfoDialog
    PlayerBar  Transport  SeekBar             completed player bar (spec §5.4)
  src/renderer/views/ (+ views.module.css)
    HomeView  AlbumsView  AlbumView  ArtistsView  ArtistView  GenresView  GenreView
    PlaylistsView  PlaylistView  SearchView  QueueView  SignalView  SettingsView  NotFoundView
  test/setup.ts  test/helpers.tsx             NEW: jsdom layout stubs; fakeApi, renderAt, renderInRouter, fixtures
  e2e/fixture.ts                              NEW: launch helper with isolated data dir
  e2e/smoke.spec.ts                           M1 test, isolated + try/finally
  e2e/library.spec.ts                         NEW: add folder → scan → play → search → queue → restart
```

### Task 1: Protocol v2 — library, playlist, queue and scan types

**Files:**
- Create: `engine/crates/protocol/src/library.rs`
- Modify: `engine/crates/protocol/src/lib.rs`, `engine/crates/protocol/src/types.rs`, `engine/crates/protocol/src/rpc.rs`
- Modify (compile fixes only): `engine/crates/engine-core/src/player/mod.rs` (`publish`, `emit_queue`), `engine/crates/aurora-engine/tests/rpc.rs` (protocol version 1 → 2)
- Modify (type fixes only): `app/src/renderer/stores/player.ts` (`initialState`), TrackSummary literals in `app/test/*.test.ts(x)`
- Test: `engine/crates/protocol/tests/export.rs`
- Regenerated: `shared/protocol/*.ts`

**Interfaces:**
- Produces (Rust, `protocol::*`; every struct is `#[serde(rename_all = "camelCase")]` and `#[ts(export)]`):
  - **New in `types.rs`:** `RepeatMode { Off, All, One }`.
  - **New fields:**
    - `TrackSummary` gains `track_id: Option<i64>`, `artist: Option<String>`, `album: Option<String>`, `artwork_id: Option<i64>`.
    - `PlayerState` gains `queue_item_id: Option<u32>`, `shuffle: bool`, `repeat: RepeatMode`.
    - `QueueItem` gains `track_id`, `artist`, `album`, `artwork_id`, `duration_ms: Option<u64>`.
  - **New types in `library.rs`:**
    - Paging and rows: `Page<T>`, `Folder`, `TrackRow`, `TrackDetail`, `NamedRef`, `ReplayGainInfo`, `MusicBrainzIds`, `AlbumRow`, `AlbumDetail`, `ArtistRow`, `ArtistDetail`, `GenreRow`.
    - Queries: `SortKey`, `ListFilter`, `ListQuery`, `SearchResults`.
    - Artwork: `Palette`, `ArtworkInfo`.
    - Scan and change events: `ScanPhase`, `ScanProgress`, `LibraryScope`, `LibraryChanged`.
    - Playlists and recents: `Playlist`, `PlaylistItem`, `PlaylistDetail`, `ImportResult`, `RecentKind`, `RecentResult`.
  - **New params in `rpc.rs`:** `QueueSetParams`, `TrackIdsParams`, `QueueMoveParams`, `QueueRemoveParams`, `ShuffleParams`, `RepeatParams`, `IdParams`, `PathParams`, `RescanParams`, `ArtworkParams`, `SearchParams`, `RecentParams`, `NameParams`, `RenameParams`, `PlaylistAddParams`, `PlaylistRemoveParams`, `PlaylistReorderParams`, `ExportParams`.
  - **Version:** `PROTOCOL_VERSION = 2`.
- Produces (TS): the same names under `@protocol/<Name>`. `Page` is generic: `Page<T>`. All IDs and counts are `number` (`TS_RS_LARGE_INT=number`).

- [ ] **Step 1: Write the failing tests**

Append to `engine/crates/protocol/tests/export.rs`:

```rust
#[test]
fn protocol_version_is_two() {
    assert_eq!(PROTOCOL_VERSION, 2);
}

#[test]
fn list_query_accepts_empty_object() {
    let q: ListQuery = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(q.filter.is_none() && q.sort.is_none() && q.offset.is_none() && q.limit.is_none() && q.desc.is_none());
    let q: ListQuery = serde_json::from_value(serde_json::json!({ "filter": { "artistId": 7 }, "sort": "addedAt", "desc": true, "offset": 100, "limit": 50 })).unwrap();
    assert_eq!(q.filter.unwrap().artist_id, Some(7));
    assert_eq!(q.sort, Some(SortKey::AddedAt));
}

#[test]
fn library_rows_serialize_camel_case() {
    let t = TrackRow {
        id: 1, path: "/m/a.flac".into(), title: "A".into(), artist: "X, Y".into(), artist_id: Some(3), album: "Al".into(), album_id: Some(4),
        album_artist: None, track_no: Some(1), disc_no: None, year: Some(1999), genre: None, duration_ms: Some(1000), codec: Some(Codec::Flac),
        container: "flac".into(), bitrate_kbps: None, sample_rate: Some(96000), bit_depth: Some(24), channels: Some(2), artwork_id: None,
        missing: false, unplayable: None, play_count: 0, added_at: 5, last_played_at: None,
    };
    let v = serde_json::to_value(&t).unwrap();
    assert_eq!(v["artistId"], 3);
    assert_eq!(v["trackNo"], 1);
    assert_eq!(v["codec"], "flac");
    assert_eq!(v["addedAt"], 5);
    let p = Page { items: vec![t], total: 1 };
    assert_eq!(serde_json::to_value(&p).unwrap()["total"], 1);
}

#[test]
fn repeat_and_scan_wire_names() {
    assert_eq!(serde_json::to_string(&RepeatMode::One).unwrap(), "\"one\"");
    assert_eq!(serde_json::to_string(&ScanPhase::Walking).unwrap(), "\"walking\"");
    assert_eq!(serde_json::to_string(&LibraryScope::Playlists).unwrap(), "\"playlists\"");
    assert_eq!(serde_json::to_string(&RecentKind::MostPlayed).unwrap(), "\"mostPlayed\"");
    let s = PlayerState::default();
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["repeat"], "off");
    assert_eq!(v["shuffle"], false);
    assert!(v["queueItemId"].is_null());
}

#[test]
fn queue_params_defaults() {
    let p: QueueSetParams = serde_json::from_value(serde_json::json!({ "trackIds": [3, 4] })).unwrap();
    assert_eq!(p.track_ids, vec![3, 4]);
    assert_eq!(p.start_index, None);
    let p: PlaylistAddParams = serde_json::from_value(serde_json::json!({ "id": 1, "trackIds": [2] })).unwrap();
    assert_eq!(p.index, None);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p protocol`
Expected: compile errors (`ListQuery`, `TrackRow`, `RepeatMode`, … not found).

- [ ] **Step 3: Add the types**

Create `engine/crates/protocol/src/library.rs`:

```rust
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
```

In `engine/crates/protocol/src/lib.rs`:

```rust
//! Wire types shared by the engine and the Electron app. No logic lives here.
pub mod errors;
pub mod library;
pub mod rpc;
pub mod types;

pub use errors::ErrorCode;
pub use library::*;
pub use types::*;

pub const PROTOCOL_VERSION: u32 = 2;
```

In `engine/crates/protocol/src/types.rs`:
- Add `RepeatMode`.
- Replace `TrackSummary`, `PlayerState` and `QueueItem` with the versions below. Everything else stays.

```rust
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum RepeatMode { #[default] Off, All, One }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackSummary {
    pub path: String,
    pub title: String,
    pub duration_ms: Option<u64>,
    pub track_id: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_id: Option<i64>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub queue_index: Option<u32>,
    pub queue_item_id: Option<u32>,
    pub track: Option<TrackSummary>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub mode: OutputMode,
    pub device_id: Option<String>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueItem {
    pub id: u32,
    pub path: String,
    pub title: String,
    pub track_id: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_id: Option<i64>,
    pub duration_ms: Option<u64>,
}
```

Append to `engine/crates/protocol/src/rpc.rs` (add `RepeatMode` and `RecentKind` to the imports: `use crate::types::{OutputMode, RepeatMode}; use crate::library::RecentKind;`):

```rust
#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueSetParams {
    pub track_ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub start_index: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackIdsParams { pub track_ids: Vec<i64> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueMoveParams { pub from: u32, pub to: u32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueRemoveParams { pub item_ids: Vec<u32> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShuffleParams { pub shuffle: bool }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeatParams { pub repeat: RepeatMode }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IdParams { pub id: i64 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PathParams { pub path: String }

#[derive(Serialize, Deserialize, TS, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RescanParams { #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub folder_id: Option<i64> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ArtworkParams { pub id: i64, pub size: u32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchParams { pub text: String, #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub limit: Option<u32> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentParams { pub kind: RecentKind, #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub limit: Option<u32> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NameParams { pub name: String }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RenameParams { pub id: i64, pub name: String }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistAddParams {
    pub id: i64,
    pub track_ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub index: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistRemoveParams { pub id: i64, pub item_ids: Vec<i64> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistReorderParams { pub id: i64, pub from: u32, pub to: u32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportParams { pub id: i64, pub path: String }
```

- [ ] **Step 4: Fix the compile sites**

In `engine/crates/engine-core/src/player/mod.rs`, update `publish` and `emit_queue` so the new fields compile. Task 12 replaces these bodies.

```rust
    fn publish(&mut self, emit: bool) {
        self.last_tick = Instant::now();
        let entry = self.queue.get(self.cur);
        let duration_ms = self.markers.front().filter(|m| m.idx == self.cur).and_then(|m| m.duration_ms);
        let state = PlayerState {
            status: self.status,
            queue_index: entry.map(|_| self.cur as u32),
            queue_item_id: entry.map(|e| e.id),
            track: entry.map(|e| TrackSummary { path: e.path.to_string_lossy().into_owned(), title: e.title.clone(), duration_ms, track_id: None, artist: None, album: None, artwork_id: None }),
            position_ms: self.position_ms(),
            duration_ms,
            volume: self.volume,
            mode: self.mode,
            device_id: self.device.clone(),
            shuffle: false,
            repeat: RepeatMode::Off,
        };
        *self.shared_state.lock().unwrap() = state.clone();
        if emit { let _ = self.events.send(PlayerEvent::State(state)); }
    }

    fn emit_queue(&self) {
        let items = self.queue.iter().map(|e| QueueItem { id: e.id, path: e.path.to_string_lossy().into_owned(), title: e.title.clone(), track_id: None, artist: None, album: None, artwork_id: None, duration_ms: None }).collect();
        let _ = self.events.send(PlayerEvent::Queue(QueueChanged { items, current: (!self.queue.is_empty()).then_some(self.cur as u32) }));
    }
```

In `engine/crates/aurora-engine/tests/rpc.rs` `handshake_comes_first`, change `assert_eq!(first["params"]["protocolVersion"], 1);` to `2`.

- [ ] **Step 5: Run the engine tests and regenerate the TS types**

Run: `cargo test --manifest-path engine/Cargo.toml --workspace`
Expected: all pass. `shared/protocol/` gains `Page.ts`, `TrackRow.ts`, `ListQuery.ts`, `RepeatMode.ts`, … and `ProtocolVersion.ts` now says `2`.

Check: `git status --porcelain shared/protocol` lists the new and changed files. `git add` them all.

- [ ] **Step 6: Fix the app types**

In `app/src/renderer/stores/player.ts`:

```ts
const initialState: PlayerState = { status: 'stopped', queueIndex: null, queueItemId: null, track: null, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null, shuffle: false, repeat: 'off' }
```

Find the TrackSummary literals in the app tests (`grep -rn "durationMs:" app/test`). Add `trackId: null, artist: null, album: null, artworkId: null` to each `track: { path, title, durationMs }` literal.

Run: `npm --prefix app run typecheck && npm --prefix app test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add engine/crates/protocol engine/crates/engine-core/src/player/mod.rs engine/crates/aurora-engine/tests/rpc.rs shared/protocol app/src/renderer/stores/player.ts app/test
git commit -m "feat(protocol): v2 types for library, playlists, queue editing and scan events"
```

### Task 2: `library` crate — database, schema, migrations, corruption recovery, folders

**Files:**
- Modify: `engine/Cargo.toml` (member + workspace deps)
- Create: `engine/crates/library/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/db.rs`, `src/artwork.rs` (paths only; Task 4 adds the rest), `src/schema/001_init.sql`
- Test: `engine/crates/library/tests/db.rs`

**Interfaces:**
- Consumes: `protocol::{Folder, ErrorCode}` (Task 1).
- Produces:
  - **`library::Library`:**
    - Construction: `open(dir: &Path) -> Result<(Library, OpenReport), LibError>`.
    - Paths: `dir()`, `db_path()`, `artwork_dir()`.
    - Raw access: `#[doc(hidden)] pub fn conn(&self) -> MutexGuard<'_, Connection>`.
    - Folders: `folders() -> Result<Vec<Folder>>`, `folder(id) -> Result<Folder>`, `add_folder(&Path) -> Result<Folder>`, `remove_folder(id) -> Result<()>`.
  - **`library::OpenReport { recovered_from: Option<PathBuf>, restored_folders: Vec<i64> }`.**
  - **`library::LibError`:** variants `Db`, `Io`, `Corrupt`, `NewerSchema{found, supported}`, `NotFound(String)`, `InvalidArgument(String)`, `FileMissing(String)`, `Cancelled`. It has `code() -> protocol::ErrorCode`.
  - **`library::db`:**
    - Connection: `connect(&Path) -> Result<Connection>` (configured, not migrated), `migrate(&mut Connection)`, `SCHEMA_VERSION`.
    - Maintenance: `refresh_stats(&Connection)`, `cleanup_orphans(&Connection) -> Result<Vec<String>>`, which returns the removed artwork hashes.
    - Folder mirror: `read_folder_mirror(dir)`, `write_folder_mirror(dir, &[String])`.
  - **`library::artwork`:** `SIZES: [u32; 3] = [64, 300, 1000]`, `path_for(dir, hash, size) -> PathBuf`, `remove_files(dir, hash)`.
  - **`library::unix_now() -> i64`.**
  - **The schema in `001_init.sql`.** Later tasks rely on these exact table and column names.

- [ ] **Step 1: Add the crate skeleton**

In `engine/Cargo.toml`:
- Add `"crates/library"` to `members`.
- Add to `[workspace.dependencies]`:

```toml
library = { path = "crates/library" }
rusqlite = { version = "0.40.2", features = ["bundled"] }
lofty = "0.25.4"
jwalk = "0.9.0"
rayon = "1.10"
notify = "8.2.0"
notify-debouncer-full = "0.7.0"
image = { version = "0.25.10", default-features = false, features = ["jpeg", "png", "webp"] }
webp = "0.3.1"
blake3 = "1.8.7"
tempfile = "3.27.0"
```

Create `engine/crates/library/Cargo.toml`:

```toml
[package]
name = "library"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[dependencies]
protocol.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
rusqlite.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Create `engine/crates/library/src/schema/001_init.sql`:

```sql
CREATE TABLE folders (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  added_at INTEGER NOT NULL,
  last_scan_at INTEGER
);
CREATE TABLE artwork (
  id INTEGER PRIMARY KEY,
  hash TEXT NOT NULL UNIQUE,
  dominant TEXT, vibrant TEXT, muted TEXT
);
CREATE TABLE artists (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE,
  sort_name TEXT NOT NULL,
  album_count INTEGER NOT NULL DEFAULT 0,
  track_count INTEGER NOT NULL DEFAULT 0,
  artwork_id INTEGER REFERENCES artwork(id) ON DELETE SET NULL
);
CREATE TABLE albums (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL,
  artist_id INTEGER REFERENCES artists(id) ON DELETE SET NULL,
  group_key TEXT NOT NULL UNIQUE,
  year INTEGER,
  artwork_id INTEGER REFERENCES artwork(id) ON DELETE SET NULL,
  added_at INTEGER NOT NULL,
  track_count INTEGER NOT NULL DEFAULT 0,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  codec TEXT,
  max_sample_rate INTEGER,
  max_bit_depth INTEGER
);
CREATE INDEX albums_title ON albums(title COLLATE NOCASE);
CREATE INDEX albums_added ON albums(added_at);
CREATE INDEX albums_artist ON albums(artist_id);
CREATE TABLE genres (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE,
  album_count INTEGER NOT NULL DEFAULT 0,
  track_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE tracks (
  id INTEGER PRIMARY KEY,
  folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  path TEXT NOT NULL UNIQUE,
  mtime INTEGER NOT NULL,
  size INTEGER NOT NULL,
  title TEXT NOT NULL,
  artist TEXT NOT NULL,
  album_id INTEGER REFERENCES albums(id) ON DELETE SET NULL,
  album TEXT NOT NULL,
  album_artist TEXT,
  composer TEXT,
  track_no INTEGER, track_total INTEGER, disc_no INTEGER, disc_total INTEGER,
  year INTEGER,
  genre TEXT,
  duration_ms INTEGER,
  codec TEXT,
  container TEXT NOT NULL,
  bitrate_kbps INTEGER, sample_rate INTEGER, bit_depth INTEGER, channels INTEGER,
  rg_track_gain REAL, rg_track_peak REAL, rg_album_gain REAL, rg_album_peak REAL,
  mb_recording_id TEXT, mb_release_id TEXT, mb_artist_id TEXT,
  has_lyrics INTEGER NOT NULL DEFAULT 0,
  artwork_id INTEGER REFERENCES artwork(id) ON DELETE SET NULL,
  added_at INTEGER NOT NULL,
  missing_since INTEGER,
  unplayable TEXT,
  play_count INTEGER NOT NULL DEFAULT 0,
  last_played_at INTEGER
);
CREATE INDEX tracks_folder ON tracks(folder_id);
CREATE INDEX tracks_album ON tracks(album_id, disc_no, track_no);
CREATE INDEX tracks_added ON tracks(added_at);
CREATE INDEX tracks_played ON tracks(play_count);
CREATE INDEX tracks_title ON tracks(title COLLATE NOCASE);
CREATE TABLE track_artists (
  track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  PRIMARY KEY (track_id, artist_id)
) WITHOUT ROWID;
CREATE INDEX track_artists_artist ON track_artists(artist_id);
CREATE TABLE album_artists (
  album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
  artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  PRIMARY KEY (album_id, artist_id)
) WITHOUT ROWID;
CREATE INDEX album_artists_artist ON album_artists(artist_id);
CREATE TABLE track_genres (
  track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  genre_id INTEGER NOT NULL REFERENCES genres(id) ON DELETE CASCADE,
  PRIMARY KEY (track_id, genre_id)
) WITHOUT ROWID;
CREATE INDEX track_genres_genre ON track_genres(genre_id);
CREATE TABLE playlists (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE playlist_items (
  id INTEGER PRIMARY KEY,
  playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  position INTEGER NOT NULL
);
CREATE INDEX playlist_items_order ON playlist_items(playlist_id, position);
CREATE INDEX playlist_items_track ON playlist_items(track_id);
CREATE TABLE play_history (
  id INTEGER PRIMARY KEY,
  track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  played_at INTEGER NOT NULL
);
CREATE INDEX play_history_track ON play_history(track_id, played_at);
CREATE INDEX play_history_time ON play_history(played_at);
CREATE VIRTUAL TABLE tracks_fts USING fts5(
  title, artist, album, album_artist, composer,
  tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3'
);
CREATE TRIGGER tracks_fts_delete AFTER DELETE ON tracks BEGIN
  DELETE FROM tracks_fts WHERE rowid = old.id;
END;
CREATE VIRTUAL TABLE artists_fts USING fts5(
  name, tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3'
);
CREATE TRIGGER artists_fts_delete AFTER DELETE ON artists BEGIN
  DELETE FROM artists_fts WHERE rowid = old.id;
END;
```

- [ ] **Step 2: Write the failing tests**

Create `engine/crates/library/tests/db.rs`:

```rust
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
    let conn = lib.conn();
    for table in ["tracks", "albums", "artists", "artwork", "tracks_fts"] {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{table} emptied");
    }
    assert!(!art_file.exists(), "orphaned artwork files are deleted");
    assert!(lib.folders().unwrap().is_empty());
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test db`
Expected: compile errors (`Library`, `db`, … unresolved).

- [ ] **Step 4: Implement**

`engine/crates/library/src/error.rs`:

```rust
use protocol::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum LibError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("library database is corrupt")]
    Corrupt,
    #[error("library database is from a newer Aurora (schema {found}, this build supports {supported})")]
    NewerSchema { found: i64, supported: i64 },
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    InvalidArgument(String),
    #[error("file not found: {0}")]
    FileMissing(String),
    #[error("scan cancelled")]
    Cancelled,
}

impl LibError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::TrackNotFound,
            Self::InvalidArgument(_) => ErrorCode::InvalidArgument,
            Self::FileMissing(_) => ErrorCode::FileMissing,
            _ => ErrorCode::DbError,
        }
    }

    /// True for errors that mean the database file itself is damaged (not, e.g., locked).
    pub fn is_corruption(&self) -> bool {
        match self {
            Self::Corrupt => true,
            Self::Db(rusqlite::Error::SqliteFailure(f, _)) => matches!(f.code, rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase),
            _ => false,
        }
    }
}

pub type Result<T, E = LibError> = std::result::Result<T, E>;
```

`engine/crates/library/src/db.rs`:

```rust
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{LibError, Result};

const MIGRATIONS: &[&str] = &[include_str!("schema/001_init.sql")];
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
```

`engine/crates/library/src/artwork.rs` (Task 4 extends this file):

```rust
use std::path::{Path, PathBuf};

/// Cached sizes, in pixels (longest edge).
pub const SIZES: [u32; 3] = [64, 300, 1000];

pub fn path_for(dir: &Path, hash: &str, size: u32) -> PathBuf {
    dir.join(&hash[..2]).join(format!("{hash}-{size}.webp"))
}

pub fn remove_files(dir: &Path, hash: &str) {
    for size in SIZES {
        let _ = std::fs::remove_file(path_for(dir, hash, size));
    }
}
```

`engine/crates/library/src/lib.rs`:

```rust
//! Aurora's music library: scanning, tags, artwork, SQLite storage and queries (spec §3.9).
pub mod artwork;
pub mod db;
pub mod error;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use protocol::Folder;
use rusqlite::{params, Connection, OptionalExtension};

pub use error::{LibError, Result};

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub struct OpenReport {
    /// Where a damaged database was moved, when one was.
    pub recovered_from: Option<PathBuf>,
    /// Folders re-added from `folders.json` after recovery; the caller rescans them.
    pub restored_folders: Vec<i64>,
}

pub struct Library {
    conn: Mutex<Connection>,
    dir: PathBuf,
}

/// Canonical, user-facing form of a folder path: absolute, symlinks resolved, and on Windows without
/// the `\\?\` verbatim prefix that `canonicalize` adds (so paths match what the OS and the UI show).
pub fn normalize_root(p: &Path) -> std::io::Result<PathBuf> {
    let c = std::fs::canonicalize(p)?;
    #[cfg(windows)]
    {
        let s = c.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") { return Ok(PathBuf::from(format!(r"\\{rest}"))); }
        if let Some(rest) = s.strip_prefix(r"\\?\") { return Ok(PathBuf::from(rest)); }
    }
    Ok(c)
}

impl Library {
    pub fn open(dir: &Path) -> Result<(Self, OpenReport)> {
        std::fs::create_dir_all(dir.join("artwork"))?;
        let opened = db::open(&dir.join("library.db"))?;
        let lib = Library { conn: Mutex::new(opened.conn), dir: dir.to_path_buf() };
        let mut restored_folders = vec![];
        if opened.recovered_from.is_some() {
            for path in db::read_folder_mirror(dir) {
                match lib.insert_folder(&path) {
                    Ok(f) => restored_folders.push(f.id),
                    Err(e) => tracing::warn!(path, error = %e, "couldn't restore folder after recovery"),
                }
            }
        }
        Ok((lib, OpenReport { recovered_from: opened.recovered_from, restored_folders }))
    }

    pub fn dir(&self) -> &Path { &self.dir }
    pub fn db_path(&self) -> PathBuf { self.dir.join("library.db") }
    pub fn artwork_dir(&self) -> PathBuf { self.dir.join("artwork") }

    /// The shared connection. Hidden: for this crate's modules, tests and the scan worker setup.
    #[doc(hidden)]
    pub fn conn(&self) -> MutexGuard<'_, Connection> { self.conn.lock().unwrap_or_else(|p| p.into_inner()) }

    fn folder_row(conn: &Connection, id: i64) -> Result<Option<Folder>> {
        Ok(conn
            .query_row(
                "SELECT f.id, f.path, f.last_scan_at,
                        (SELECT COUNT(*) FROM tracks t WHERE t.folder_id = f.id AND t.missing_since IS NULL)
                 FROM folders f WHERE f.id = ?1",
                [id],
                |r| {
                    let path: String = r.get(1)?;
                    Ok(Folder { id: r.get(0)?, available: Path::new(&path).is_dir(), path, last_scan_at: r.get(2)?, track_count: r.get::<_, i64>(3)? as u64 })
                },
            )
            .optional()?)
    }

    pub fn folder(&self, id: i64) -> Result<Folder> {
        Self::folder_row(&self.conn(), id)?.ok_or_else(|| LibError::NotFound(format!("folder {id}")))
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let conn = self.conn();
        let ids = conn.prepare("SELECT id FROM folders ORDER BY path")?.query_map([], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter().filter_map(|id| Self::folder_row(&conn, id).transpose()).collect()
    }

    fn insert_folder(&self, path: &str) -> Result<Folder> {
        let id = {
            let conn = self.conn();
            conn.execute("INSERT INTO folders(path, added_at) VALUES (?1, ?2)", params![path, unix_now()])?;
            conn.last_insert_rowid()
        };
        self.sync_mirror()?;
        self.folder(id)
    }

    fn sync_mirror(&self) -> Result<()> {
        let paths = self.conn().prepare("SELECT path FROM folders ORDER BY path")?.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        db::write_folder_mirror(&self.dir, &paths)
    }

    /// Adds a music folder. Rejects paths that don't exist and folders that overlap an existing one
    /// (equal, inside, or containing it), since a file must belong to exactly one folder.
    pub fn add_folder(&self, path: &Path) -> Result<Folder> {
        let root = normalize_root(path).map_err(|_| LibError::InvalidArgument(format!("folder not found: {}", path.display())))?;
        if !root.is_dir() {
            return Err(LibError::InvalidArgument(format!("not a folder: {}", root.display())));
        }
        for f in self.folders()? {
            let existing = Path::new(&f.path);
            if root.starts_with(existing) || existing.starts_with(&root) {
                return Err(LibError::InvalidArgument(format!("{} overlaps the library folder {}", root.display(), f.path)));
            }
        }
        self.insert_folder(&root.to_string_lossy())
    }

    /// Removes a folder and everything scanned from it (tracks cascade; orphans and their artwork go too).
    pub fn remove_folder(&self, id: i64) -> Result<()> {
        let hashes = {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            if tx.execute("DELETE FROM folders WHERE id = ?1", [id])? == 0 {
                return Err(LibError::NotFound(format!("folder {id}")));
            }
            let hashes = db::cleanup_orphans(&tx)?;
            db::refresh_stats(&tx)?;
            tx.commit()?;
            hashes
        };
        let art = self.artwork_dir();
        for h in hashes { artwork::remove_files(&art, &h); }
        self.sync_mirror()
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test db`
Expected: 8 passed. (On the first build, rusqlite compiles the bundled SQLite. That takes a minute.)

- [ ] **Step 6: Commit**

```bash
git add engine/Cargo.toml engine/Cargo.lock engine/crates/library
git commit -m "feat(library): SQLite schema, migrations, corruption recovery and folders"
```

### Task 3: Tag reading (`lofty`) and library test helpers

**Files:**
- Modify: `engine/crates/library/Cargo.toml` (add `lofty`), `engine/crates/library/src/lib.rs` (`pub mod tags;`)
- Create: `engine/crates/library/src/tags.rs`, `engine/crates/library/tests/common/mod.rs`
- Test: `engine/crates/library/tests/tags.rs`, unit tests in `tags.rs`

**Interfaces:**
- Consumes: `protocol::{Codec, ReplayGainInfo}`.
- Produces:
  - **`library::tags::AudioProps`:** `{ duration_ms: Option<u64>, codec: Option<Codec>, container: String, bitrate_kbps: Option<u32>, sample_rate: Option<u32>, bit_depth: Option<u16>, channels: Option<u16> }`. It derives Clone, Debug, Default, PartialEq.
  - **`library::tags::TrackTags`:**
    - Text: `title`, `artists: Vec<String>`, `album`, `album_artists: Vec<String>`, `composer`.
    - Numbering and date: `track_no`, `track_total`, `disc_no`, `disc_total`, `year: Option<i32>`.
    - Classification: `genres: Vec<String>`.
    - Extras: `replay_gain: ReplayGainInfo`, `mb_recording_id`, `mb_release_id`, `mb_artist_id`, `has_lyrics: bool`.
    - Audio and art: `props: AudioProps`, `picture: Option<Vec<u8>>`.
  - **Functions:** `library::tags::read(&Path) -> Result<TrackTags, TagError>` and `TrackTags::from_props(props, path)`. The second is a file-name-titled record used when only a probe could read the file.
  - **`library::tags::{AUDIO_EXTENSIONS, is_audio(&Path) -> bool, container_of(&Path) -> String}`.**
  - **Parsing helpers:** `library::tags::{parse_gain(&str) -> Option<f32>, parse_year(&str) -> Option<i32>}`.
  - **Test helpers in `tests/common/mod.rs`**, used by Tasks 5–10:
    - `fixture(name) -> PathBuf`;
    - `Tags<'a>` builder struct;
    - `tagged(src_fixture, dest, &Tags) -> PathBuf`;
    - `png(w, h, rgb) -> Vec<u8>`, which arrives in Task 4.

- [ ] **Step 1: Add the dependency and the test helpers**

Add `lofty.workspace = true` to `[dependencies]` in `engine/crates/library/Cargo.toml`. Also add `lofty.workspace = true` under `[dev-dependencies]`, since the helpers write tags.

Create `engine/crates/library/tests/common/mod.rs`:

```rust
#![allow(dead_code)]
use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{ItemKey, ItemValue, Tag, TagItem};

/// Generated by `fixtures/generate.sh` (run `npm run fixtures` once).
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/out").join(name)
}

/// Tag values that `tagged` writes. Unset fields are left out of the file.
#[derive(Default, Clone)]
pub struct Tags<'a> {
    pub title: Option<&'a str>,
    pub artists: &'a [&'a str],
    pub album: Option<&'a str>,
    pub album_artist: Option<&'a str>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub date: Option<&'a str>,
    pub genres: &'a [&'a str],
    pub extra: &'a [(ItemKey, &'a str)],
    pub pictures: &'a [(PictureType, &'a [u8])],
}

/// Copies the fixture `src` to `dest` (creating parent folders) and replaces its tags with `t`.
pub fn tagged(src: &str, dest: &Path, t: &Tags) -> PathBuf {
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::copy(fixture(src), dest).unwrap_or_else(|e| panic!("copy {src}: {e} (run fixtures/generate.sh)"));
    let tf = lofty::read_from_path(dest).unwrap();
    let mut tag = Tag::new(tf.primary_tag_type());
    if let Some(v) = t.title { tag.set_title(v.into()); }
    for a in t.artists { tag.push(TagItem::new(ItemKey::TrackArtist, ItemValue::Text((*a).into()))); }
    if let Some(v) = t.album { tag.set_album(v.into()); }
    if let Some(v) = t.album_artist { tag.insert_text(ItemKey::AlbumArtist, v.into()); }
    if let Some(n) = t.track { tag.set_track(n); }
    if let Some(n) = t.disc { tag.set_disk(n); }
    if let Some(d) = t.date { tag.insert_text(ItemKey::RecordingDate, d.into()); }
    for g in t.genres { tag.push(TagItem::new(ItemKey::Genre, ItemValue::Text((*g).into()))); }
    for (k, v) in t.extra { tag.insert_text(*k, (*v).into()); }
    for (kind, data) in t.pictures {
        tag.push_picture(Picture::unchecked(data.to_vec()).pic_type(*kind).mime_type(MimeType::Png).build());
    }
    tag.save_to_path(dest, WriteOptions::default()).unwrap();
    dest.to_path_buf()
}
```

- [ ] **Step 2: Write the failing tests**

Create `engine/crates/library/tests/tags.rs`:

```rust
mod common;
use common::{tagged, Tags};
use library::tags;
use lofty::picture::PictureType;
use lofty::tag::ItemKey;
use protocol::Codec;

#[test]
fn reads_basic_fields_and_properties() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac24_96k.flac", &dir.path().join("a.flac"), &Tags {
        title: Some("Opening"), artists: &["Alpha"], album: Some("First Light"), album_artist: Some("Alpha"),
        track: Some(3), disc: Some(1), date: Some("1999-05-01"), genres: &["Ambient"], ..Default::default()
    });
    let t = tags::read(&p).unwrap();
    assert_eq!(t.title.as_deref(), Some("Opening"));
    assert_eq!(t.artists, vec!["Alpha"]);
    assert_eq!(t.album.as_deref(), Some("First Light"));
    assert_eq!(t.album_artists, vec!["Alpha"]);
    assert_eq!((t.track_no, t.disc_no, t.year), (Some(3), Some(1), Some(1999)));
    assert_eq!(t.genres, vec!["Ambient"]);
    assert_eq!(t.props.codec, Some(Codec::Flac));
    assert_eq!(t.props.container, "flac");
    assert_eq!(t.props.sample_rate, Some(96000));
    assert_eq!(t.props.bit_depth, Some(24));
    assert_eq!(t.props.channels, Some(2));
    assert!(t.props.duration_ms.unwrap() > 500);
}

#[test]
fn multi_value_artists_and_genres_use_native_fields_only() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("m.flac"), &Tags {
        artists: &["Alpha", "Beta & Gamma"], genres: &["Jazz", "Funk"], ..Default::default()
    });
    let t = tags::read(&p).unwrap();
    assert_eq!(t.artists, vec!["Alpha", "Beta & Gamma"], "no splitting on '&'");
    assert_eq!(t.genres, vec!["Jazz", "Funk"]);
}

#[test]
fn front_cover_wins_over_other_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("p.flac"), &Tags {
        pictures: &[(PictureType::Other, b"other"), (PictureType::CoverFront, b"front")], ..Default::default()
    });
    assert_eq!(tags::read(&p).unwrap().picture.as_deref(), Some(&b"front"[..]));
}

#[test]
fn replaygain_musicbrainz_and_composer() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("r.flac"), &Tags {
        extra: &[
            (ItemKey::ReplayGainTrackGain, "-6.50 dB"), (ItemKey::ReplayGainTrackPeak, "0.988"),
            (ItemKey::MusicBrainzRecordingId, "rec-1"), (ItemKey::Composer, "C. Writer"), (ItemKey::Lyrics, "la la"),
        ],
        ..Default::default()
    });
    let t = tags::read(&p).unwrap();
    assert_eq!(t.replay_gain.track_gain, Some(-6.5));
    assert_eq!(t.replay_gain.track_peak, Some(0.988));
    assert_eq!(t.mb_recording_id.as_deref(), Some("rec-1"));
    assert_eq!(t.composer.as_deref(), Some("C. Writer"));
    assert!(t.has_lyrics);
}

#[test]
fn untagged_file_still_has_properties() {
    let t = tags::read(&common::fixture("pcm16_44k.wav")).unwrap();
    assert!(t.title.is_none() && t.artists.is_empty());
    assert_eq!(t.props.codec, Some(Codec::Pcm));
    assert_eq!(t.props.container, "wav");
}

#[test]
fn mp4_codec_distinguishes_alac_from_aac() {
    assert_eq!(tags::read(&common::fixture("alac16_44k.m4a")).unwrap().props.codec, Some(Codec::Alac));
    assert_eq!(tags::read(&common::fixture("aac_lc_256.m4a")).unwrap().props.codec, Some(Codec::AacLc));
    assert_eq!(tags::read(&common::fixture("mp3_320.mp3")).unwrap().props.codec, Some(Codec::Mp3));
    assert_eq!(tags::read(&common::fixture("opus_160.opus")).unwrap().props.codec, Some(Codec::Opus));
}

#[test]
fn sidecar_lrc_sets_has_lyrics() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("song.flac"), &Tags::default());
    std::fs::write(dir.path().join("song.lrc"), "[00:01.00]hi").unwrap();
    assert!(tags::read(&p).unwrap().has_lyrics);
}

#[test]
fn empty_and_dsd_files_are_errors_for_lofty() {
    assert!(tags::read(&common::fixture("empty.mp3")).is_err());
    assert!(tags::read(&common::fixture("dsd64.dsf")).is_err(), "DSD tags are read through the fallback probe");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test tags`
Expected: compile error (`library::tags` not found).

- [ ] **Step 4: Implement `tags.rs`**

```rust
//! Tag and audio-property reading with `lofty` (spec §3.9 metadata list).
use std::path::Path;

use lofty::file::FileType;
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};
use protocol::{Codec, ReplayGainInfo};

/// File extensions the scanner indexes (matches the app's `AUDIO_EXTENSIONS`, minus the "alac" pseudo-extension).
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "m4b", "mp4", "aac", "flac", "wav", "aif", "aiff", "aifc", "ogg", "oga", "opus", "dsf", "dff"];

pub fn container_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub fn is_audio(path: &Path) -> bool {
    AUDIO_EXTENSIONS.contains(&container_of(path).as_str())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioProps {
    pub duration_ms: Option<u64>,
    pub codec: Option<Codec>,
    pub container: String,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u16>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    /// Native multi-value artist fields, in tag order. Never split heuristically.
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub composer: Option<String>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_no: Option<u32>,
    pub disc_total: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub replay_gain: ReplayGainInfo,
    pub mb_recording_id: Option<String>,
    pub mb_release_id: Option<String>,
    pub mb_artist_id: Option<String>,
    pub has_lyrics: bool,
    pub props: AudioProps,
    /// Embedded front cover (or the first picture when none is marked front).
    pub picture: Option<Vec<u8>>,
}

impl TrackTags {
    /// A record for a file `lofty` can't read but the decoder can (DSF/DFF): properties only.
    pub fn from_props(props: AudioProps, path: &Path) -> Self {
        TrackTags { props, has_lyrics: path.with_extension("lrc").is_file(), ..Default::default() }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("can't read tags: {0}")]
pub struct TagError(pub String);

/// Library-level codec from the container type. AAC is reported as AAC-LC here; playback's signal
/// path reports HE-AAC precisely. FLAC-in-MP4 (rare) is labelled ALAC because both carry a bit depth.
fn codec_for(ft: FileType, bit_depth: Option<u8>) -> Option<Codec> {
    Some(match ft {
        FileType::Mpeg => Codec::Mp3,
        FileType::Aac => Codec::AacLc,
        FileType::Mp4 if bit_depth.is_some() => Codec::Alac,
        FileType::Mp4 => Codec::AacLc,
        FileType::Flac => Codec::Flac,
        FileType::Wav | FileType::Aiff => Codec::Pcm,
        FileType::Vorbis => Codec::Vorbis,
        FileType::Opus => Codec::Opus,
        _ => return None,
    })
}

/// "-6.50 dB" → -6.5; "0.988" → 0.988.
pub fn parse_gain(s: &str) -> Option<f32> {
    let t = s.trim();
    let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).unwrap_or(t).trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The first four-digit run in a date string: "1999-05-01" → 1999, "05/1999" → 1999.
pub fn parse_year(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(3))
        .find(|&i| b[i..i + 4].iter().all(u8::is_ascii_digit) && b.get(i + 4).is_none_or(|c| !c.is_ascii_digit()))
        .and_then(|i| s[i..i + 4].parse().ok())
        .filter(|y| (1000..=9999).contains(y))
}

fn texts(tag: &Tag, key: ItemKey) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for v in tag.get_strings(key).map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o == v) { out.push(v.to_string()); }
    }
    out
}

fn text(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn fill(out: &mut TrackTags, tag: &Tag) {
    out.title = text(tag, ItemKey::TrackTitle);
    // An explicit multi-value ARTISTS field (Picard writes one) is more precise than ARTIST.
    out.artists = texts(tag, ItemKey::TrackArtists);
    if out.artists.is_empty() { out.artists = texts(tag, ItemKey::TrackArtist); }
    out.album = text(tag, ItemKey::AlbumTitle);
    out.album_artists = texts(tag, ItemKey::AlbumArtist);
    let composers = texts(tag, ItemKey::Composer);
    out.composer = (!composers.is_empty()).then(|| composers.join(", "));
    out.track_no = tag.track();
    out.track_total = tag.track_total();
    out.disc_no = tag.disk();
    out.disc_total = tag.disk_total();
    out.year = tag.date().map(|d| d.year as i32).or_else(|| {
        [ItemKey::RecordingDate, ItemKey::Year, ItemKey::OriginalReleaseDate].into_iter().find_map(|k| tag.get_string(k).and_then(parse_year))
    });
    out.genres = texts(tag, ItemKey::Genre);
    out.replay_gain = ReplayGainInfo {
        track_gain: text(tag, ItemKey::ReplayGainTrackGain).and_then(|s| parse_gain(&s)),
        track_peak: text(tag, ItemKey::ReplayGainTrackPeak).and_then(|s| parse_gain(&s)),
        album_gain: text(tag, ItemKey::ReplayGainAlbumGain).and_then(|s| parse_gain(&s)),
        album_peak: text(tag, ItemKey::ReplayGainAlbumPeak).and_then(|s| parse_gain(&s)),
    };
    out.mb_recording_id = text(tag, ItemKey::MusicBrainzRecordingId);
    out.mb_release_id = text(tag, ItemKey::MusicBrainzReleaseId);
    out.mb_artist_id = text(tag, ItemKey::MusicBrainzArtistId);
    out.has_lyrics |= text(tag, ItemKey::Lyrics).is_some() || text(tag, ItemKey::UnsyncLyrics).is_some();
    let pics = tag.pictures();
    out.picture = pics.iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pics.first()).map(|p| p.data().to_vec());
}

pub fn read(path: &Path) -> Result<TrackTags, TagError> {
    let tf = lofty::read_from_path(path).map_err(|e| TagError(e.to_string()))?;
    let p = tf.properties();
    let props = AudioProps {
        duration_ms: Some(p.duration().as_millis() as u64).filter(|d| *d > 0),
        codec: codec_for(tf.file_type(), p.bit_depth()),
        container: container_of(path),
        bitrate_kbps: p.audio_bitrate().or(p.overall_bitrate()).filter(|b| *b > 0),
        sample_rate: p.sample_rate().filter(|r| *r > 0),
        bit_depth: p.bit_depth().map(u16::from),
        channels: p.channels().map(u16::from),
    };
    let mut out = TrackTags::from_props(props, path);
    if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) { fill(&mut out, tag); }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_parsing() {
        assert_eq!(parse_gain("-6.50 dB"), Some(-6.5));
        assert_eq!(parse_gain("+1.2 db"), Some(1.2));
        assert_eq!(parse_gain("0.988"), Some(0.988));
        assert_eq!(parse_gain("loud"), None);
        assert_eq!(parse_gain("NaN"), None);
    }

    #[test]
    fn year_parsing() {
        assert_eq!(parse_year("1999-05-01"), Some(1999));
        assert_eq!(parse_year("05/2003"), Some(2003));
        assert_eq!(parse_year("12345"), None);
        assert_eq!(parse_year("99"), None);
    }

    #[test]
    fn audio_extension_filter() {
        assert!(is_audio(Path::new("/a/B.FLAC")));
        assert!(is_audio(Path::new("x.dsf")));
        assert!(!is_audio(Path::new("cover.jpg")));
        assert!(!is_audio(Path::new("noext")));
    }
}
```

Add `pub mod tags;` to `src/lib.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library`
Expected: all `tags` tests (8 integration + 3 unit) and the Task 2 tests pass.

If `untagged_file_still_has_properties` sees a title, `fixtures/generate.sh` wrote one. Drop the `title.is_none()` part of the assertion and keep the property checks.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/library engine/Cargo.lock
git commit -m "feat(library): tag and property reading with native multi-value fields"
```

### Task 4: Artwork cache (WebP sizes, content hash) and palette extraction

**Files:**
- Modify: `engine/crates/library/Cargo.toml` (add `image`, `webp`, `blake3`; `image` also as a dev-dependency), `engine/crates/library/src/artwork.rs`, `engine/crates/library/src/lib.rs` (`pub mod palette;`), `engine/crates/library/tests/common/mod.rs` (add `png`)
- Create: `engine/crates/library/src/palette.rs`
- Test: `engine/crates/library/tests/artwork.rs`, unit tests in `palette.rs`

**Interfaces:**
- Consumes: `artwork::{SIZES, path_for}` (Task 2), `protocol::Palette`.
- Produces:
  - **`library::artwork`:**
    - `hash(&[u8]) -> String`: blake3, first 32 hex characters.
    - `nearest_size(u32) -> u32`.
    - `store(dir, &[u8]) -> Result<(String, Palette), ArtError>`.
    - `cover_rank(file_name: &str) -> Option<usize>`: 0 = cover, 1 = folder, 2 = front.
    - `ArtError`.
  - **`library::palette::extract(rgb: &[u8]) -> Palette`.**
  - **Test helper:** `common::png(w, h, [r, g, b]) -> Vec<u8>`.

- [ ] **Step 1: Add the dependencies and the PNG helper**

In `engine/crates/library/Cargo.toml`:
- `[dependencies]`: add `image.workspace = true`, `webp.workspace = true`, `blake3.workspace = true`.
- `[dev-dependencies]`: add `image.workspace = true`.

Append to `tests/common/mod.rs`:

```rust
/// A solid-colour PNG of `w`×`h`.
pub fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}
```

- [ ] **Step 2: Write the failing tests**

Create `engine/crates/library/tests/artwork.rs`:

```rust
mod common;
use library::artwork;

fn dims(path: &std::path::Path) -> (u32, u32) {
    let img = image::open(path).unwrap();
    (img.width(), img.height())
}

#[test]
fn store_writes_three_downscaled_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = common::png(1500, 1000, [200, 50, 50]);
    let (hash, palette) = artwork::store(dir.path(), &bytes).unwrap();
    assert_eq!(hash.len(), 32);
    assert_eq!(hash, artwork::hash(&bytes));
    assert_eq!(dims(&artwork::path_for(dir.path(), &hash, 1000)), (1000, 667));
    assert_eq!(dims(&artwork::path_for(dir.path(), &hash, 300)), (300, 200));
    assert_eq!(dims(&artwork::path_for(dir.path(), &hash, 64)), (64, 43));
    assert_eq!(palette.dominant, "#c83232");
}

#[test]
fn small_images_are_not_upscaled() {
    let dir = tempfile::tempdir().unwrap();
    let (hash, _) = artwork::store(dir.path(), &common::png(50, 40, [1, 2, 3])).unwrap();
    for size in artwork::SIZES {
        assert_eq!(dims(&artwork::path_for(dir.path(), &hash, size)), (50, 40));
    }
}

#[test]
fn storing_twice_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = common::png(400, 400, [9, 9, 9]);
    let (a, _) = artwork::store(dir.path(), &bytes).unwrap();
    let modified = std::fs::metadata(artwork::path_for(dir.path(), &a, 300)).unwrap().modified().unwrap();
    let (b, _) = artwork::store(dir.path(), &bytes).unwrap();
    assert_eq!(a, b);
    assert_eq!(std::fs::metadata(artwork::path_for(dir.path(), &a, 300)).unwrap().modified().unwrap(), modified, "existing files are kept");
}

#[test]
fn garbage_bytes_are_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(artwork::store(dir.path(), b"not an image").is_err());
}

#[test]
fn cover_file_priority() {
    assert_eq!(artwork::cover_rank("Cover.JPG"), Some(0));
    assert_eq!(artwork::cover_rank("folder.png"), Some(1));
    assert_eq!(artwork::cover_rank("front.webp"), Some(2));
    assert_eq!(artwork::cover_rank("front.jpeg"), Some(2));
    assert_eq!(artwork::cover_rank("back.jpg"), None);
    assert_eq!(artwork::cover_rank("cover.gif"), None);
    assert_eq!(artwork::cover_rank("cover"), None);
}

#[test]
fn nearest_cached_size() {
    assert_eq!(artwork::nearest_size(10), 64);
    assert_eq!(artwork::nearest_size(64), 64);
    assert_eq!(artwork::nearest_size(65), 300);
    assert_eq!(artwork::nearest_size(5000), 1000);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test artwork`
Expected: compile errors (`artwork::store`, `hash`, … not found).

- [ ] **Step 4: Implement**

Create `engine/crates/library/src/palette.rs`:

```rust
//! Album-art palette: dominant, vibrant and muted colours (spec §3.9, used by §5.5 dynamic colour).
use protocol::Palette;

const K: usize = 5;
const ITERATIONS: usize = 10;

fn luma(p: [f32; 3]) -> f32 { 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2] }

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 { (0..3).map(|i| (a[i] - b[i]).powi(2)).sum() }

fn nearest(centers: &[[f32; 3]], p: [f32; 3]) -> usize {
    (0..centers.len()).min_by(|&a, &b| dist2(centers[a], p).total_cmp(&dist2(centers[b], p))).unwrap_or(0)
}

/// HSL saturation and lightness, both in 0..=1.
fn sat_light(p: [f32; 3]) -> (f32, f32) {
    let (r, g, b) = (p[0] / 255.0, p[1] / 255.0, p[2] / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6) };
    (s.min(1.0), l)
}

fn vibrancy(p: [f32; 3]) -> f32 { let (s, l) = sat_light(p); s * (1.0 - (l - 0.5).abs() * 2.0) }

/// Lower is more "muted": low saturation and a slightly dark lightness (it blends into `--bg`).
fn muted_score(p: [f32; 3]) -> f32 { let (s, l) = sat_light(p); s + (l - 0.4).abs() }

fn hex(p: [f32; 3]) -> String {
    let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(p[0]), c(p[1]), c(p[2]))
}

/// Palette of packed RGB bytes via k-means (k = 5, 10 iterations, seeded deterministically from
/// pixels spread evenly through luma order). Clusters under 5 % of pixels can't be vibrant/muted.
pub fn extract(rgb: &[u8]) -> Palette {
    let px: Vec<[f32; 3]> = rgb.chunks_exact(3).map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]).collect();
    if px.is_empty() {
        return Palette { dominant: "#000000".into(), vibrant: "#000000".into(), muted: "#000000".into() };
    }
    let k = K.min(px.len());
    let mut order: Vec<usize> = (0..px.len()).collect();
    order.sort_by(|&a, &b| luma(px[a]).total_cmp(&luma(px[b])));
    let mut centers: Vec<[f32; 3]> = (0..k).map(|i| px[order[(2 * i + 1) * order.len() / (2 * k)]]).collect();
    let mut counts = vec![0usize; k];
    for _ in 0..ITERATIONS {
        let mut sums = vec![[0f32; 3]; k];
        counts.iter_mut().for_each(|c| *c = 0);
        for &p in &px {
            let i = nearest(&centers, p);
            counts[i] += 1;
            for c in 0..3 { sums[i][c] += p[c]; }
        }
        for i in 0..k {
            if counts[i] > 0 { for c in 0..3 { centers[i][c] = sums[i][c] / counts[i] as f32; } }
        }
    }
    let clusters: Vec<([f32; 3], usize)> = centers.into_iter().zip(counts).filter(|(_, n)| *n > 0).collect();
    let dominant = clusters.iter().max_by_key(|(_, n)| *n).map(|c| c.0).unwrap_or([0.0; 3]);
    let significant: Vec<&([f32; 3], usize)> = clusters.iter().filter(|(_, n)| n * 20 >= px.len()).collect();
    let vibrant = significant.iter().max_by(|a, b| vibrancy(a.0).total_cmp(&vibrancy(b.0))).map_or(dominant, |c| c.0);
    let muted = significant.iter().min_by(|a, b| muted_score(a.0).total_cmp(&muted_score(b.0))).map_or(dominant, |c| c.0);
    Palette { dominant: hex(dominant), vibrant: hex(vibrant), muted: hex(muted) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(parts: &[([u8; 3], usize)]) -> Vec<u8> {
        parts.iter().flat_map(|(c, n)| std::iter::repeat_n(*c, *n)).flatten().collect()
    }

    #[test]
    fn solid_colour_fills_every_role() {
        let p = extract(&image(&[([200, 50, 50], 100)]));
        assert_eq!((p.dominant.as_str(), p.vibrant.as_str(), p.muted.as_str()), ("#c83232", "#c83232", "#c83232"));
    }

    #[test]
    fn grey_dominant_red_vibrant() {
        let p = extract(&image(&[([128, 128, 128], 700), ([220, 30, 30], 300)]));
        assert_eq!(p.dominant, "#808080");
        assert_eq!(p.vibrant, "#dc1e1e");
        assert_eq!(p.muted, "#808080");
    }

    #[test]
    fn tiny_specks_never_win_vibrant() {
        let p = extract(&image(&[([60, 60, 70], 990), ([255, 0, 255], 10)]));
        assert_ne!(p.vibrant, "#ff00ff");
    }

    #[test]
    fn empty_input_is_black() {
        assert_eq!(extract(&[]).dominant, "#000000");
    }
}
```

Replace `engine/crates/library/src/artwork.rs` with:

```rust
//! Artwork cache: each distinct image is resized once to 64/300/1000 px lossy WebP under
//! `artwork/<hash[..2]>/<hash>-<size>.webp` (spec §3.9).
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::imageops::FilterType;
use protocol::Palette;

/// Cached sizes, in pixels (longest edge).
pub const SIZES: [u32; 3] = [64, 300, 1000];
const WEBP_QUALITY: f32 = 80.0;
const COVER_STEMS: [&str; 3] = ["cover", "folder", "front"];
const COVER_EXTS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

#[derive(Debug, thiserror::Error)]
#[error("artwork: {0}")]
pub struct ArtError(pub String);

pub fn path_for(dir: &Path, hash: &str, size: u32) -> PathBuf {
    dir.join(&hash[..2]).join(format!("{hash}-{size}.webp"))
}

pub fn remove_files(dir: &Path, hash: &str) {
    for size in SIZES {
        let _ = std::fs::remove_file(path_for(dir, hash, size));
    }
}

/// Content hash of the original image bytes: blake3, first 32 hex characters.
pub fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..32].to_string()
}

/// The smallest cached size that's at least `size`, or the largest.
pub fn nearest_size(size: u32) -> u32 {
    SIZES.into_iter().find(|s| *s >= size).unwrap_or(SIZES[SIZES.len() - 1])
}

/// Folder-cover priority of a file name (0 = cover, 1 = folder, 2 = front), case-insensitive.
pub fn cover_rank(file_name: &str) -> Option<usize> {
    let lower = file_name.to_ascii_lowercase();
    let (stem, ext) = lower.rsplit_once('.')?;
    if !COVER_EXTS.contains(&ext) { return None; }
    COVER_STEMS.iter().position(|s| *s == stem)
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(path.parent().expect("cache paths have a parent"))?;
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })
}

/// Decodes `bytes`, writes any missing cache sizes (never upscaling) and returns the hash and palette.
/// Safe to call concurrently for the same image: each writer renames its own temp file into place.
pub fn store(dir: &Path, bytes: &[u8]) -> Result<(String, Palette), ArtError> {
    let h = hash(bytes);
    let img = image::load_from_memory(bytes).map_err(|e| ArtError(e.to_string()))?;
    for size in SIZES {
        let path = path_for(dir, &h, size);
        if path.exists() { continue; }
        let scaled = if img.width().max(img.height()) > size { img.resize(size, size, FilterType::Lanczos3) } else { img.clone() };
        let rgb = scaled.to_rgb8();
        let webp = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height()).encode(WEBP_QUALITY);
        write_atomic(&path, &webp).map_err(|e| ArtError(format!("{}: {e}", path.display())))?;
    }
    let thumb = img.thumbnail(64, 64).to_rgb8();
    Ok((h, crate::palette::extract(thumb.as_raw())))
}
```

Add `pub mod palette;` to `src/lib.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library`
Expected: all pass (6 artwork integration tests, 4 palette unit tests, earlier tests).

Two tests might be off by one:
- **Resize dimensions:** `image`'s `resize` rounds, so the 1500×1000 → 300 px case gives `(300, 200)`, and the 64 px case may give `(64, 43)` or `(64, 42)`. If yours differs by one pixel, assert on the actual `image` 0.25.10 output, but keep the longest edge exact.
- **Palette hex:** k-means can land a centre one unit away. If `grey_dominant_red_vibrant` reports e.g. `#7f7f7f`, check that the clusters separate and update the expected hex to the deterministic output.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/library engine/Cargo.lock
git commit -m "feat(library): WebP artwork cache by content hash and palette extraction"
```

### Task 5: Scanner — walk, skip unchanged, parallel tag reads, upsert, missing/purge

**Files:**
- Modify: `engine/crates/library/Cargo.toml` (add `jwalk`, `rayon`), `engine/crates/library/src/lib.rs` (`pub mod codec; pub mod scan;`, `Library::scanner`)
- Create: `engine/crates/library/src/codec.rs`, `engine/crates/library/src/scan.rs`
- Test: `engine/crates/library/tests/scan.rs`

**Interfaces:**
- Consumes:
  - From Task 3: `tags::{read, is_audio, container_of, AudioProps, TrackTags}`.
  - From Task 4: `artwork::{store, hash, cover_rank, remove_files}`.
  - From Task 2: `db::{connect, cleanup_orphans, refresh_stats}`.
  - From Task 1: `protocol::{ScanProgress, ScanPhase, Codec}`.
- Produces:
  - **`library::scan` types:**
    - `Fallback = Arc<dyn Fn(&Path) -> Option<AudioProps> + Send + Sync>`.
    - `ScanOptions { fallback: Option<Fallback>, now: Option<i64> }` (Clone, Default).
    - `ScanSummary { added, updated, unchanged, missing, restored, purged: u64 }`.
    - `MISSING_RETENTION_SECS`.
  - **`library::scan::Scanner`:** `scan(&mut self, folder_id: i64, roots: &[PathBuf], full: bool, progress: &mut dyn FnMut(&ScanProgress), cancel: &AtomicBool) -> Result<ScanSummary>`.
    - Empty `roots` means the folder root.
    - `full` enables the 30-day purge and stamps `last_scan_at`.
  - **`Library::scanner(&self, opts: ScanOptions) -> Result<Scanner>`.** The scanner has its own connection to the same DB.
  - **`library::codec::{name(Codec) -> &'static str, parse(&str) -> Option<Codec>}`.** The DB stores codecs as their wire names.
  - **FTS columns:** `tracks_fts.album_artist` holds the album's grouping artist (the album artist, else the first track artist), so album search finds albums whose tracks lack an album-artist tag. Each newly created artist also gets an `artists_fts` row.
  - **Unknown values are stored as empty strings:** `tracks.artist`/`album`, and the album group key built from them. The UI renders `""` as "Unknown artist" / "Unknown album".

- [ ] **Step 1: Write the failing tests**

Create `engine/crates/library/tests/scan.rs`:

```rust
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

struct Fx { _data: tempfile::TempDir, music: tempfile::TempDir, lib: Library, folder: i64 }

fn setup() -> Fx {
    let data = tempfile::tempdir().unwrap();
    let music = tempfile::tempdir().unwrap();
    let (lib, _) = Library::open(data.path()).unwrap();
    let folder = lib.add_folder(music.path()).unwrap().id;
    Fx { _data: data, music, lib, folder }
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
    let s = scan_with(&fx, &[p.clone()], false, ScanOptions::default());
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test scan`
Expected: compile errors (`library::scan` not found).

- [ ] **Step 3: Implement**

Add `jwalk.workspace = true` and `rayon.workspace = true` to `[dependencies]`.

Create `engine/crates/library/src/codec.rs`:

```rust
//! Codecs are stored in the DB as their wire names (protocol `Codec`, kebab-case).
use protocol::Codec;

pub fn name(c: Codec) -> &'static str {
    match c {
        Codec::Mp3 => "mp3", Codec::AacLc => "aac-lc", Codec::HeAac => "he-aac", Codec::HeAacV2 => "he-aac-v2",
        Codec::Alac => "alac", Codec::Flac => "flac", Codec::Pcm => "pcm", Codec::Vorbis => "vorbis",
        Codec::Opus => "opus", Codec::Dsd => "dsd",
    }
}

pub fn parse(s: &str) -> Option<Codec> {
    Some(match s {
        "mp3" => Codec::Mp3, "aac-lc" => Codec::AacLc, "he-aac" => Codec::HeAac, "he-aac-v2" => Codec::HeAacV2,
        "alac" => Codec::Alac, "flac" => Codec::Flac, "pcm" => Codec::Pcm, "vorbis" => Codec::Vorbis,
        "opus" => Codec::Opus, "dsd" => Codec::Dsd,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_match_the_wire_format() {
        for c in [protocol::Codec::Mp3, protocol::Codec::HeAacV2, protocol::Codec::Dsd] {
            assert_eq!(serde_json::to_value(c).unwrap(), super::name(c));
            assert_eq!(super::parse(super::name(c)), Some(c));
        }
    }
}
```

Create `engine/crates/library/src/scan.rs`:

```rust
//! Library scanner (spec §3.9): walk the folder in parallel, skip files whose (mtime, size) are
//! unchanged, read tags in parallel, store new artwork once, upsert in transactions of `BATCH`
//! files, then mark vanished files missing and (on full scans) purge ones missing for 30 days.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use protocol::{Palette, ScanPhase, ScanProgress};
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::tags::{self, AudioProps, TrackTags};
use crate::{artwork, codec, db, LibError, Result};

pub type Fallback = Arc<dyn Fn(&Path) -> Option<AudioProps> + Send + Sync>;

pub const MISSING_RETENTION_SECS: i64 = 30 * 24 * 3600;
const BATCH: usize = 500;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Clone, Default)]
pub struct ScanOptions {
    /// Reads properties of files `lofty` can't (DSF/DFF). The engine passes its decoder probe.
    pub fallback: Option<Fallback>,
    /// Clock override (unix seconds) for tests.
    pub now: Option<i64>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScanSummary { pub added: u64, pub updated: u64, pub unchanged: u64, pub missing: u64, pub restored: u64, pub purged: u64 }

pub struct Scanner { conn: Connection, art_dir: PathBuf, opts: ScanOptions }

impl Scanner {
    pub(crate) fn new(conn: Connection, art_dir: PathBuf, opts: ScanOptions) -> Self { Scanner { conn, art_dir, opts } }

    /// Scans `roots` (files or folders inside folder `folder_id`; empty = the folder root).
    /// Tracks under `roots` that weren't found are marked missing. `full` also purges tracks missing
    /// for longer than 30 days and records the scan time on the folder.
    pub fn scan(&mut self, folder_id: i64, roots: &[PathBuf], full: bool, progress: &mut dyn FnMut(&ScanProgress), cancel: &AtomicBool) -> Result<ScanSummary> {
        scan(&mut self.conn, &self.art_dir, folder_id, roots, full, &self.opts, progress, cancel)
    }
}

struct Candidate { path: PathBuf, key: String, mtime: i64, size: i64 }
struct Existing { id: i64, mtime: i64, size: i64, missing: bool }
enum ArtSource { Embedded(Vec<u8>), File(PathBuf) }
struct ReadFile { cand: Candidate, tags: TrackTags, unplayable: Option<&'static str>, art: Option<ArtSource> }

/// Folder → (rank, path) of its best `cover|folder|front` image.
type Covers = HashMap<PathBuf, (usize, PathBuf)>;

fn file_meta(p: &Path) -> Option<(i64, i64)> {
    let m = std::fs::metadata(p).ok()?;
    let mtime = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
    Some((mtime, m.len() as i64))
}

fn note_cover(covers: &mut Covers, path: PathBuf, name: &str) {
    if let Some(rank) = artwork::cover_rank(name) {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        if covers.get(&dir).is_none_or(|(r, _)| rank < *r) { covers.insert(dir, (rank, path)); }
    }
}

fn candidate(path: PathBuf) -> Option<Candidate> {
    let (mtime, size) = file_meta(&path)?;
    Some(Candidate { key: path.to_string_lossy().into_owned(), path, mtime, size })
}

fn walk(root: &Path, files: &mut Vec<Candidate>, covers: &mut Covers) {
    if root.is_file() {
        if tags::is_audio(root) { files.extend(candidate(root.to_path_buf())); }
        if let Some(Ok(entries)) = root.parent().map(std::fs::read_dir) {
            for e in entries.flatten() { note_cover(covers, e.path(), &e.file_name().to_string_lossy()); }
        }
        return;
    }
    if !root.is_dir() { return; }
    for entry in jwalk::WalkDir::new(root).skip_hidden(true).follow_links(false).into_iter().flatten() {
        if !entry.file_type().is_file() { continue; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if tags::is_audio(&path) { files.extend(candidate(path)); } else { note_cover(covers, path, &name); }
    }
}

fn load_existing(conn: &Connection, folder_id: i64, roots: &[PathBuf]) -> Result<HashMap<String, Existing>> {
    let mut stmt = conn.prepare("SELECT id, path, mtime, size, missing_since IS NOT NULL FROM tracks WHERE folder_id = ?1")?;
    let rows = stmt.query_map([folder_id], |r| Ok((r.get::<_, String>(1)?, Existing { id: r.get(0)?, mtime: r.get(2)?, size: r.get(3)?, missing: r.get(4)? })))?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, e) = row?;
        if roots.iter().any(|root| Path::new(&path).starts_with(root)) { out.insert(path, e); }
    }
    Ok(out)
}

fn read_one(cand: Candidate, covers: &Covers, fallback: Option<&Fallback>) -> ReadFile {
    let (mut tags, unplayable) = match tags::read(&cand.path) {
        Ok(t) => (t, None),
        Err(e) => match fallback.and_then(|f| f(&cand.path)) {
            Some(props) => (TrackTags::from_props(props, &cand.path), None),
            None => {
                tracing::debug!(path = %cand.path.display(), error = %e, "unreadable audio file");
                let props = AudioProps { container: tags::container_of(&cand.path), ..Default::default() };
                (TrackTags::from_props(props, &cand.path), Some("UNSUPPORTED_FORMAT"))
            }
        },
    };
    let art = match tags.picture.take() {
        Some(bytes) => Some(ArtSource::Embedded(bytes)),
        None => cand.path.parent().and_then(|d| covers.get(d)).map(|(_, p)| ArtSource::File(p.clone())),
    };
    ReadFile { cand, tags, unplayable, art }
}

/// Hashes each file's artwork (reading each folder cover once), stores images the DB hasn't seen
/// (in parallel) and returns each file's artwork hash plus the new (hash, palette) rows.
fn prepare_art(conn: &Connection, art_dir: &Path, reads: &mut [ReadFile]) -> Result<(Vec<Option<String>>, Vec<(String, Palette)>)> {
    let mut bytes_by_hash: HashMap<String, Vec<u8>> = HashMap::new();
    let mut file_hashes: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut per_file = Vec::with_capacity(reads.len());
    for r in reads.iter_mut() {
        let h = match r.art.take() {
            Some(ArtSource::Embedded(b)) => {
                let h = artwork::hash(&b);
                bytes_by_hash.entry(h.clone()).or_insert(b);
                Some(h)
            }
            Some(ArtSource::File(p)) => file_hashes
                .entry(p.clone())
                .or_insert_with(|| {
                    std::fs::read(&p).ok().map(|b| {
                        let h = artwork::hash(&b);
                        bytes_by_hash.entry(h.clone()).or_insert(b);
                        h
                    })
                })
                .clone(),
            None => None,
        };
        per_file.push(h);
    }
    let mut known = conn.prepare_cached("SELECT 1 FROM artwork WHERE hash = ?1")?;
    let mut fresh = vec![];
    for h in bytes_by_hash.keys() {
        if !known.exists([h])? { fresh.push(h.clone()); }
    }
    let stored: Vec<(String, Palette)> = fresh
        .par_iter()
        .filter_map(|h| match artwork::store(art_dir, &bytes_by_hash[h]) {
            Ok((_, palette)) => Some((h.clone(), palette)),
            Err(e) => { tracing::warn!(error = %e, "skipping unreadable artwork"); None }
        })
        .collect();
    let failed: HashSet<&String> = fresh.iter().filter(|h| !stored.iter().any(|(s, _)| s == *h)).collect();
    let per_file = per_file.into_iter().map(|h| h.filter(|h| !failed.contains(h))).collect();
    Ok((per_file, stored))
}

fn sort_name(name: &str) -> String {
    let lower = name.to_lowercase();
    lower.strip_prefix("the ").map(str::to_string).unwrap_or(lower)
}

fn artist_id(tx: &Transaction, name: &str) -> Result<i64> {
    let created = tx.prepare_cached("INSERT INTO artists(name, sort_name) VALUES (?1, ?2) ON CONFLICT(name) DO NOTHING")?.execute(params![name, sort_name(name)])? == 1;
    let id: i64 = tx.prepare_cached("SELECT id FROM artists WHERE name = ?1")?.query_row([name], |r| r.get(0))?;
    if created {
        // artists_fts rows are deleted by a trigger when cleanup_orphans removes the artist.
        tx.prepare_cached("INSERT INTO artists_fts(rowid, name) VALUES (?1, ?2)")?.execute(params![id, name])?;
    }
    Ok(id)
}

fn genre_id(tx: &Transaction, name: &str) -> Result<i64> {
    tx.prepare_cached("INSERT INTO genres(name) VALUES (?1) ON CONFLICT(name) DO NOTHING")?.execute([name])?;
    Ok(tx.prepare_cached("SELECT id FROM genres WHERE name = ?1")?.query_row([name], |r| r.get(0))?)
}

fn stem(p: &Path) -> String { p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() }

fn upsert(tx: &Transaction, folder_id: i64, r: &ReadFile, art_hash: Option<&str>, now: i64) -> Result<()> {
    let t = &r.tags;
    let title = t.title.clone().unwrap_or_else(|| stem(&r.cand.path));
    let album_title = t.album.clone().unwrap_or_default();
    let album_artist_names: Vec<String> =
        if !t.album_artists.is_empty() { t.album_artists.clone() } else { t.artists.first().cloned().into_iter().collect() };
    let group_key = format!("{}\u{1f}{}", album_artist_names.join(", ").to_lowercase(), album_title.to_lowercase());
    let album_artist_ids = album_artist_names.iter().map(|n| artist_id(tx, n)).collect::<Result<Vec<_>>>()?;
    let art_id: Option<i64> = match art_hash {
        Some(h) => tx.prepare_cached("SELECT id FROM artwork WHERE hash = ?1")?.query_row([h], |r| r.get(0)).optional()?,
        None => None,
    };
    let album_id: i64 = tx.prepare_cached(
        "INSERT INTO albums(title, artist_id, group_key, year, artwork_id, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(group_key) DO UPDATE SET year = COALESCE(albums.year, excluded.year),
                                              artwork_id = COALESCE(albums.artwork_id, excluded.artwork_id)
         RETURNING id",
    )?.query_row(params![album_title, album_artist_ids.first(), group_key, t.year, art_id, now], |r| r.get(0))?;
    for (pos, id) in album_artist_ids.iter().enumerate() {
        tx.prepare_cached("INSERT OR IGNORE INTO album_artists(album_id, artist_id, position) VALUES (?1, ?2, ?3)")?.execute(params![album_id, id, pos as i64])?;
    }
    let p = &t.props;
    let rg = &t.replay_gain;
    let album_artist = (!t.album_artists.is_empty()).then(|| t.album_artists.join(", "));
    let genre = (!t.genres.is_empty()).then(|| t.genres.join(", "));
    let track_id: i64 = tx.prepare_cached(
        "INSERT INTO tracks(folder_id, path, mtime, size, title, artist, album_id, album, album_artist, composer,
                            track_no, track_total, disc_no, disc_total, year, genre, duration_ms, codec, container,
                            bitrate_kbps, sample_rate, bit_depth, channels, rg_track_gain, rg_track_peak, rg_album_gain,
                            rg_album_peak, mb_recording_id, mb_release_id, mb_artist_id, has_lyrics, artwork_id, added_at, unplayable)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
                 ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34)
         ON CONFLICT(path) DO UPDATE SET
           folder_id = excluded.folder_id, mtime = excluded.mtime, size = excluded.size, title = excluded.title,
           artist = excluded.artist, album_id = excluded.album_id, album = excluded.album, album_artist = excluded.album_artist,
           composer = excluded.composer, track_no = excluded.track_no, track_total = excluded.track_total,
           disc_no = excluded.disc_no, disc_total = excluded.disc_total, year = excluded.year, genre = excluded.genre,
           duration_ms = excluded.duration_ms, codec = excluded.codec, container = excluded.container,
           bitrate_kbps = excluded.bitrate_kbps, sample_rate = excluded.sample_rate, bit_depth = excluded.bit_depth,
           channels = excluded.channels, rg_track_gain = excluded.rg_track_gain, rg_track_peak = excluded.rg_track_peak,
           rg_album_gain = excluded.rg_album_gain, rg_album_peak = excluded.rg_album_peak,
           mb_recording_id = excluded.mb_recording_id, mb_release_id = excluded.mb_release_id,
           mb_artist_id = excluded.mb_artist_id, has_lyrics = excluded.has_lyrics, artwork_id = excluded.artwork_id,
           unplayable = excluded.unplayable, missing_since = NULL
         RETURNING id",
    )?.query_row(
        params![
            folder_id, r.cand.key, r.cand.mtime, r.cand.size, title, t.artists.join(", "), album_id, album_title, album_artist,
            t.composer, t.track_no, t.track_total, t.disc_no, t.disc_total, t.year, genre, p.duration_ms.map(|d| d as i64),
            p.codec.map(codec::name), p.container, p.bitrate_kbps, p.sample_rate, p.bit_depth, p.channels,
            rg.track_gain, rg.track_peak, rg.album_gain, rg.album_peak, t.mb_recording_id, t.mb_release_id, t.mb_artist_id,
            t.has_lyrics, art_id, now, r.unplayable,
        ],
        |row| row.get(0),
    )?;
    tx.prepare_cached("DELETE FROM track_artists WHERE track_id = ?1")?.execute([track_id])?;
    for (pos, name) in t.artists.iter().enumerate() {
        let id = artist_id(tx, name)?;
        tx.prepare_cached("INSERT OR IGNORE INTO track_artists(track_id, artist_id, position) VALUES (?1, ?2, ?3)")?.execute(params![track_id, id, pos as i64])?;
    }
    tx.prepare_cached("DELETE FROM track_genres WHERE track_id = ?1")?.execute([track_id])?;
    for name in &t.genres {
        let id = genre_id(tx, name)?;
        tx.prepare_cached("INSERT OR IGNORE INTO track_genres(track_id, genre_id) VALUES (?1, ?2)")?.execute(params![track_id, id])?;
    }
    tx.prepare_cached("DELETE FROM tracks_fts WHERE rowid = ?1")?.execute([track_id])?;
    tx.prepare_cached("INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, composer) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?
        .execute(params![track_id, title, t.artists.join(", "), album_title, album_artist_names.join(", "), t.composer])?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan(
    conn: &mut Connection, art_dir: &Path, folder_id: i64, roots: &[PathBuf], full: bool, opts: &ScanOptions,
    progress: &mut dyn FnMut(&ScanProgress), cancel: &AtomicBool,
) -> Result<ScanSummary> {
    let now = opts.now.unwrap_or_else(crate::unix_now);
    let mut prog = ScanProgress { folder_id, phase: ScanPhase::Walking, processed: 0, total: 0, added: 0, updated: 0, missing: 0 };
    progress(&prog);
    let folder_root: String = conn
        .query_row("SELECT path FROM folders WHERE id = ?1", [folder_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| LibError::NotFound(format!("folder {folder_id}")))?;
    let roots: Vec<PathBuf> = if roots.is_empty() { vec![PathBuf::from(&folder_root)] } else { roots.to_vec() };

    let mut files = vec![];
    let mut covers = Covers::new();
    for root in &roots { walk(root, &mut files, &mut covers); }
    files.retain(|c| c.path.starts_with(&folder_root));
    files.sort_by(|a, b| a.key.cmp(&b.key));
    files.dedup_by(|a, b| a.key == b.key);

    let existing = load_existing(conn, folder_id, &roots)?;
    let mut summary = ScanSummary::default();
    let mut seen = HashSet::new();
    let mut restore = vec![];
    let mut to_read = vec![];
    for c in files {
        seen.insert(c.key.clone());
        match existing.get(&c.key) {
            Some(e) if e.mtime == c.mtime && e.size == c.size => {
                summary.unchanged += 1;
                if e.missing { restore.push(e.id); }
            }
            _ => to_read.push(c),
        }
    }

    prog.phase = ScanPhase::Reading;
    prog.total = to_read.len() as u64;
    progress(&prog);
    let mut last = Instant::now();
    let mut rest = to_read;
    while !rest.is_empty() {
        if cancel.load(Ordering::Relaxed) { return Err(LibError::Cancelled); }
        let batch: Vec<Candidate> = rest.drain(..BATCH.min(rest.len())).collect();
        let n = batch.len() as u64;
        let mut reads: Vec<ReadFile> = batch.into_par_iter().map(|c| read_one(c, &covers, opts.fallback.as_ref())).collect();
        let (hashes, new_art) = prepare_art(conn, art_dir, &mut reads)?;
        let tx = conn.transaction()?;
        for (h, pal) in &new_art {
            tx.prepare_cached("INSERT OR IGNORE INTO artwork(hash, dominant, vibrant, muted) VALUES (?1, ?2, ?3, ?4)")?
                .execute(params![h, pal.dominant, pal.vibrant, pal.muted])?;
        }
        for (r, h) in reads.iter().zip(&hashes) {
            upsert(&tx, folder_id, r, h.as_deref(), now)?;
            if existing.contains_key(&r.cand.key) { summary.updated += 1 } else { summary.added += 1 }
        }
        tx.commit()?;
        prog.processed += n;
        prog.added = summary.added;
        prog.updated = summary.updated;
        if last.elapsed() >= PROGRESS_EVERY { progress(&prog); last = Instant::now(); }
    }

    prog.phase = ScanPhase::Finishing;
    progress(&prog);
    let tx = conn.transaction()?;
    for id in restore {
        tx.execute("UPDATE tracks SET missing_since = NULL WHERE id = ?1", [id])?;
        summary.restored += 1;
    }
    for (path, e) in &existing {
        if !seen.contains(path) && !e.missing {
            tx.execute("UPDATE tracks SET missing_since = ?1 WHERE id = ?2", params![now, e.id])?;
            summary.missing += 1;
        }
    }
    if full {
        summary.purged = tx.execute(
            "DELETE FROM tracks WHERE folder_id = ?1 AND missing_since IS NOT NULL AND missing_since <= ?2",
            params![folder_id, now - MISSING_RETENTION_SECS],
        )? as u64;
        tx.execute("UPDATE folders SET last_scan_at = ?1 WHERE id = ?2", params![now, folder_id])?;
    }
    let orphaned_art = db::cleanup_orphans(&tx)?;
    db::refresh_stats(&tx)?;
    tx.commit()?;
    // Keeps planner statistics current so the paging/search queries stay under the 50 ms target.
    conn.execute_batch("PRAGMA optimize")?;
    for h in orphaned_art { artwork::remove_files(art_dir, &h); }

    prog.phase = ScanPhase::Done;
    prog.missing = summary.missing;
    progress(&prog);
    Ok(summary)
}
```

In `src/lib.rs`:
- Add `pub mod codec; pub mod scan;`.
- Add this method to `impl Library`:

```rust
    /// A scanner with its own connection, for a worker thread (WAL lets it write while queries read).
    pub fn scanner(&self, opts: scan::ScanOptions) -> Result<scan::Scanner> {
        Ok(scan::Scanner::new(db::connect(&self.db_path())?, self.artwork_dir(), opts))
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library`
Expected: all pass, including the 11 `scan` tests and the `codec` unit test.

- [ ] **Step 5: Run clippy on the crate**

Run: `cargo clippy --manifest-path engine/Cargo.toml -p library --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/library engine/Cargo.lock
git commit -m "feat(library): parallel scanner with skip-unchanged, artwork dedup and 30-day missing retention"
```

### Task 6: Library queries — paging, sorting, filtering, album/artist/track detail

**Files:**
- Create: `engine/crates/library/src/rows.rs`, `engine/crates/library/src/query.rs`
- Modify: `engine/crates/library/src/lib.rs` (`mod rows; pub mod query;`), `engine/crates/library/tests/common/mod.rs` (add `Scanned`, `scanned`, `demo_tree`)
- Test: `engine/crates/library/tests/query.rs`

**Interfaces:**
- Consumes:
  - Schema from Task 2.
  - `Library::scanner` from Task 5.
  - From Task 1: `protocol::{ListQuery, ListFilter, SortKey, Page, AlbumRow, AlbumDetail, ArtistRow, ArtistDetail, GenreRow, TrackRow, TrackDetail, NamedRef, ReplayGainInfo, MusicBrainzIds}`.
- Produces (methods on `Library`; all return `library::Result`):
  - **Lists:** `albums(&ListQuery) -> Page<AlbumRow>`, `artists(&ListQuery) -> Page<ArtistRow>`, `genres(&ListQuery) -> Page<GenreRow>`, `tracks(&ListQuery) -> Page<TrackRow>`.
  - **Details:** `album(id) -> AlbumDetail`, `artist(id) -> ArtistDetail`, `genre(id) -> GenreRow`, `track(id) -> TrackDetail`.
    - `track()` returns `gapless: None` and `mtime` in **unix milliseconds**.
  - **Lookups:**
    - `tracks_by_ids(&[i64]) -> Vec<TrackRow>`: order is preserved, and an unknown id gives `LibError::NotFound("track <id>")`.
    - `track_id_for_path(&str) -> Option<i64>`.
  - **Defaults:** `limit` is 100 and is clamped to 1..=1000. `offset` is 0.
  - **Default sorts:**
    - albums: title;
    - artists and genres: name;
    - tracks: track number when filtered by album, otherwise title.
  - **Visibility:** missing tracks, and albums/artists/genres with no present tracks, are excluded from lists. Detail lookups by id still return them, flagged `missing`.
  - **`library::rows` (crate-internal):**
    - `TRACK_COLS` with `track(&Row)`;
    - `ALBUM_COLS` and `ALBUM_FROM` with `album(&Row)`;
    - `ARTIST_COLS` with `artist(&Row)`;
    - `GENRE_COLS` with `genre(&Row)`.

    Tasks 7 and 8 reuse them.
  - **Test helpers:** `common::{Scanned { data, music, lib, folder }, scanned(build: impl FnOnce(&Path)) -> Scanned, demo_tree(&Path)}`.

- [ ] **Step 1: Add the shared test library**

Append to `engine/crates/library/tests/common/mod.rs`:

```rust
pub struct Scanned { pub data: tempfile::TempDir, pub music: tempfile::TempDir, pub lib: library::Library, pub folder: i64 }

impl Scanned {
    pub fn root(&self) -> PathBuf { PathBuf::from(&self.lib.folder(self.folder).unwrap().path) }
    pub fn rescan(&self) {
        self.lib.scanner(Default::default()).unwrap().scan(self.folder, &[], true, &mut |_| {}, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    }
    pub fn id_of(&self, title: &str) -> i64 {
        self.lib.conn().query_row("SELECT id FROM tracks WHERE title = ?1", [title], |r| r.get(0)).unwrap()
    }
}

/// A library in a temp dir with one folder, populated by `build(root)` and scanned once.
pub fn scanned(build: impl FnOnce(&Path)) -> Scanned {
    let data = tempfile::tempdir().unwrap();
    let music = tempfile::tempdir().unwrap();
    let (lib, _) = library::Library::open(data.path()).unwrap();
    let folder = lib.add_folder(music.path()).unwrap().id;
    let s = Scanned { data, music, lib, folder };
    build(&s.root());
    s.rescan();
    s
}

/// Four albums, three artists, three genres:
/// Alpha/First Light (2001, Ambient): 1 Dawn, 2 Noon (Alpha + Beta; album artist Alpha), disc 2 track 1 Encore
/// Alpha/Zenith (1999, Rock): 1 Peak
/// Beta/Bridges (2010, Ambient): 1 Span
/// The Gamma/Night (no year, Jazz, MP3): 1 Dusk
pub fn demo_tree(root: &Path) {
    let t = |src: &str, rel: &str, tags: Tags| { tagged(src, &root.join(rel), &tags); };
    t("flac16_44k.flac", "Alpha/First Light/01.flac", Tags { title: Some("Dawn"), artists: &["Alpha"], album: Some("First Light"), track: Some(1), disc: Some(1), date: Some("2001"), genres: &["Ambient"], ..Default::default() });
    t("flac16_44k.flac", "Alpha/First Light/02.flac", Tags { title: Some("Noon"), artists: &["Alpha", "Beta"], album_artist: Some("Alpha"), album: Some("First Light"), track: Some(2), disc: Some(1), genres: &["Ambient"], ..Default::default() });
    t("flac24_96k.flac", "Alpha/First Light/2-01.flac", Tags { title: Some("Encore"), artists: &["Alpha"], album: Some("First Light"), track: Some(1), disc: Some(2), genres: &["Ambient"], ..Default::default() });
    t("flac16_44k.flac", "Alpha/Zenith/01.flac", Tags { title: Some("Peak"), artists: &["Alpha"], album: Some("Zenith"), track: Some(1), date: Some("1999"), genres: &["Rock"], ..Default::default() });
    t("flac16_44k.flac", "Beta/Bridges/01.flac", Tags { title: Some("Span"), artists: &["Beta"], album: Some("Bridges"), track: Some(1), date: Some("2010"), genres: &["Ambient"], ..Default::default() });
    t("mp3_320.mp3", "The Gamma/Night/01.mp3", Tags { title: Some("Dusk"), artists: &["The Gamma"], album: Some("Night"), track: Some(1), genres: &["Jazz"], ..Default::default() });
}
```

- [ ] **Step 2: Write the failing tests**

Create `engine/crates/library/tests/query.rs`:

```rust
mod common;
use common::{demo_tree, scanned};
use protocol::{ListFilter, ListQuery, SortKey};

fn q(sort: Option<SortKey>, desc: bool, offset: u32, limit: u32, filter: ListFilter) -> ListQuery {
    ListQuery { filter: Some(filter), sort, desc: Some(desc), offset: Some(offset), limit: Some(limit) }
}
fn titles<T>(items: &[T], f: impl Fn(&T) -> &str) -> Vec<String> { items.iter().map(|i| f(i).to_string()).collect() }

#[test]
fn albums_page_by_title_by_default() {
    let s = scanned(demo_tree);
    let p = s.lib.albums(&ListQuery { limit: Some(2), ..Default::default() }).unwrap();
    assert_eq!(p.total, 4);
    assert_eq!(titles(&p.items, |a| &a.title), ["Bridges", "First Light"]);
    let p = s.lib.albums(&ListQuery { offset: Some(2), limit: Some(2), ..Default::default() }).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Night", "Zenith"]);
}

#[test]
fn albums_sort_by_artist_uses_sort_name_then_year() {
    let s = scanned(demo_tree);
    let p = s.lib.albums(&q(Some(SortKey::Artist), false, 0, 10, Default::default())).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Zenith", "First Light", "Bridges", "Night"], "'The Gamma' sorts as 'gamma'");
    assert_eq!(p.items[1].artist, "Alpha");
}

#[test]
fn albums_sort_by_year_desc_puts_unknown_last() {
    let s = scanned(demo_tree);
    let p = s.lib.albums(&q(Some(SortKey::Year), true, 0, 10, Default::default())).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Bridges", "First Light", "Zenith", "Night"]);
}

#[test]
fn albums_filter_by_artist_includes_appearances_and_by_genre() {
    let s = scanned(demo_tree);
    let beta: i64 = s.lib.conn().query_row("SELECT id FROM artists WHERE name = 'Beta'", [], |r| r.get(0)).unwrap();
    let p = s.lib.albums(&q(None, false, 0, 10, ListFilter { artist_id: Some(beta), ..Default::default() })).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Bridges", "First Light"]);
    let ambient: i64 = s.lib.conn().query_row("SELECT id FROM genres WHERE name = 'Ambient'", [], |r| r.get(0)).unwrap();
    let p = s.lib.albums(&q(None, false, 0, 10, ListFilter { genre_id: Some(ambient), ..Default::default() })).unwrap();
    assert_eq!(p.total, 2);
}

#[test]
fn album_detail_orders_by_disc_then_track() {
    let s = scanned(demo_tree);
    let id = s.lib.albums(&ListQuery::default()).unwrap().items.into_iter().find(|a| a.title == "First Light").unwrap().id;
    let d = s.lib.album(id).unwrap();
    assert_eq!(titles(&d.tracks, |t| &t.title), ["Dawn", "Noon", "Encore"]);
    assert_eq!(d.album.track_count, 3);
    assert_eq!(d.album.year, Some(2001));
    assert_eq!(d.album.max_sample_rate, Some(96000));
    assert_eq!(d.album.max_bit_depth, Some(24));
    assert_eq!(d.album.codec, Some(protocol::Codec::Flac));
    assert_eq!(titles(&d.genres, |g| &g.name), ["Ambient"]);
}

#[test]
fn artist_detail_lists_albums_newest_first_and_top_tracks_by_plays() {
    let s = scanned(demo_tree);
    let alpha: i64 = s.lib.conn().query_row("SELECT id FROM artists WHERE name = 'Alpha'", [], |r| r.get(0)).unwrap();
    s.lib.conn().execute("UPDATE tracks SET play_count = 5 WHERE title = 'Peak'", []).unwrap();
    let d = s.lib.artist(alpha).unwrap();
    assert_eq!(d.artist.name, "Alpha");
    assert_eq!(d.artist.album_count, 2);
    assert_eq!(titles(&d.albums, |a| &a.title), ["First Light", "Zenith"]);
    assert_eq!(d.top_tracks[0].title, "Peak");
    assert_eq!(d.top_tracks.len(), 4);
}

#[test]
fn tracks_filter_sort_and_text() {
    let s = scanned(demo_tree);
    let all = s.lib.tracks(&q(Some(SortKey::Title), false, 0, 100, Default::default())).unwrap();
    assert_eq!(titles(&all.items, |t| &t.title), ["Dawn", "Dusk", "Encore", "Noon", "Peak", "Span"]);
    let p = s.lib.tracks(&q(None, false, 0, 100, ListFilter { text: Some("da".into()), ..Default::default() })).unwrap();
    assert_eq!(titles(&p.items, |t| &t.title), ["Dawn"]);
    let p = s.lib.tracks(&q(Some(SortKey::Duration), true, 0, 1, Default::default())).unwrap();
    assert_eq!(p.total, 6);
    assert_eq!(p.items.len(), 1);
    let noon = &all.items[3];
    assert_eq!(noon.artist, "Alpha, Beta");
    assert!(noon.artist_id.is_some() && noon.album_id.is_some());
}

#[test]
fn tracks_by_ids_preserves_order_and_rejects_unknown() {
    let s = scanned(demo_tree);
    let (a, b) = (s.id_of("Span"), s.id_of("Dawn"));
    let rows = s.lib.tracks_by_ids(&[a, b, a]).unwrap();
    assert_eq!(titles(&rows, |t| &t.title), ["Span", "Dawn", "Span"]);
    assert!(matches!(s.lib.tracks_by_ids(&[a, 999_999]), Err(library::LibError::NotFound(_))));
    assert_eq!(s.lib.track_id_for_path(&rows[0].path), Some(a));
}

#[test]
fn track_detail_has_credits_and_file_facts() {
    let s = scanned(demo_tree);
    let d = s.lib.track(s.id_of("Noon")).unwrap();
    assert_eq!(titles(&d.artists, |a| &a.name), ["Alpha", "Beta"]);
    assert_eq!(titles(&d.genres, |g| &g.name), ["Ambient"]);
    assert!(d.size_bytes > 0);
    assert!(d.mtime > 1_000_000_000_000, "milliseconds");
    assert!(d.gapless.is_none());
}

#[test]
fn missing_tracks_are_hidden_from_lists_but_resolvable() {
    let s = scanned(demo_tree);
    let dusk = s.id_of("Dusk");
    std::fs::remove_file(s.root().join("The Gamma/Night/01.mp3")).unwrap();
    s.rescan();
    assert_eq!(s.lib.albums(&ListQuery::default()).unwrap().total, 3);
    assert_eq!(s.lib.tracks(&ListQuery::default()).unwrap().total, 5);
    assert_eq!(s.lib.artists(&ListQuery::default()).unwrap().total, 2);
    assert!(s.lib.track(dusk).unwrap().track.missing);
}

#[test]
fn artist_and_genre_lists() {
    let s = scanned(demo_tree);
    let a = s.lib.artists(&ListQuery::default()).unwrap();
    assert_eq!(titles(&a.items, |x| &x.name), ["Alpha", "Beta", "The Gamma"]);
    let g = s.lib.genres(&ListQuery::default()).unwrap();
    assert_eq!(titles(&g.items, |x| &x.name), ["Ambient", "Jazz", "Rock"]);
    assert_eq!((g.items[0].album_count, g.items[0].track_count), (2, 4));
    assert_eq!(s.lib.genre(g.items[0].id).unwrap().name, "Ambient");
}

#[test]
fn oversized_limits_are_clamped_not_errors() {
    let s = scanned(demo_tree);
    assert_eq!(s.lib.tracks(&ListQuery { limit: Some(50_000), ..Default::default() }).unwrap().items.len(), 6);
    assert_eq!(s.lib.tracks(&ListQuery { limit: Some(0), ..Default::default() }).unwrap().items.len(), 1);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test query`
Expected: compile errors (`albums`, `tracks`, … not found on `Library`).

- [ ] **Step 4: Implement**

Create `engine/crates/library/src/rows.rs`:

```rust
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
```

Create `engine/crates/library/src/query.rs`:

```rust
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
            w.add("EXISTS (SELECT 1 FROM track_artists ta JOIN track_genres tg ON tg.track_id = ta.track_id WHERE ta.artist_id = ar.id AND tg.genre_id = ?)", [Value::Integer(id)]);
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
```

In `src/lib.rs`, add `pub mod query; mod rows;`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library`
Expected: all pass (12 `query` tests plus everything earlier).

- [ ] **Step 6: Commit**

```bash
git add engine/crates/library
git commit -m "feat(library): paged, sorted, filtered list queries and detail lookups"
```

### Task 7: Search (FTS5), play history, recent/most played, unplayable marks

**Files:**
- Create: `engine/crates/library/src/search.rs`, `engine/crates/library/src/history.rs`
- Modify: `engine/crates/library/src/lib.rs` (`pub mod search; pub mod history;`)
- Test: `engine/crates/library/tests/search.rs`, unit tests in `search.rs`

**Interfaces:**
- Consumes:
  - From Task 2: the `tracks_fts` and `artists_fts` schema.
  - From Task 6: `rows::*`.
  - From Task 1: `protocol::{SearchResults, RecentKind, RecentResult}`.
- Produces:
  - **`library::search::fts_query(text, columns: Option<&str>) -> Option<String>`:** prefix-matches every word with AND. Words are split on non-alphanumeric characters, so the result is injection-safe.
  - **`Library::search(&self, text: &str, limit: u32) -> Result<SearchResults>`:**
    - `limit` is clamped to 1..=100.
    - A query shorter than 2 characters after trimming returns empty results.
    - Tracks are ranked by bm25 with weights title 10, artist 5, album 3, album artist 3, composer 1.
    - Albums match on the album and album-artist columns.
    - Artists come from `artists_fts`.
    - Missing tracks are excluded.
  - **`Library::record_play(&self, track_id: i64, at: i64) -> Result<()>`:** increments `play_count`, sets `last_played_at` and adds a `play_history` row. An unknown id gives `NotFound`.
  - **`Library::recent(&self, kind: RecentKind, limit: u32) -> Result<RecentResult>`:** `limit` is clamped to 1..=200.
  - **`Library::set_unplayable(&self, path: &str, reason: Option<&str>) -> Result<bool>`:** returns true when a track row matched.

- [ ] **Step 1: Write the failing tests**

Create `engine/crates/library/tests/search.rs`:

```rust
mod common;
use common::{demo_tree, scanned, tagged, Tags};
use protocol::RecentKind;

fn names<T>(items: &[T], f: impl Fn(&T) -> &str) -> Vec<String> { items.iter().map(|i| f(i).to_string()).collect() }

#[test]
fn prefix_search_groups_tracks_albums_artists() {
    let s = scanned(demo_tree);
    let r = s.lib.search("dus", 20).unwrap();
    assert_eq!(names(&r.tracks, |t| &t.title), ["Dusk"]);
    let r = s.lib.search("alp", 20).unwrap();
    assert_eq!(names(&r.artists, |a| &a.name), ["Alpha"]);
    assert_eq!(names(&r.albums, |a| &a.title), ["First Light", "Zenith"], "Zenith has no album-artist tag but groups under Alpha");
    assert_eq!(r.tracks.len(), 4);
}

#[test]
fn title_matches_rank_above_artist_matches() {
    let s = scanned(|root| {
        tagged("flac16_44k.flac", &root.join("a.flac"), &Tags { title: Some("Other"), artists: &["Nova"], album: Some("X"), ..Default::default() });
        tagged("flac16_44k.flac", &root.join("b.flac"), &Tags { title: Some("Nova"), artists: &["Someone"], album: Some("Y"), ..Default::default() });
    });
    let r = s.lib.search("nova", 20).unwrap();
    assert_eq!(names(&r.tracks, |t| &t.title), ["Nova", "Other"]);
}

#[test]
fn diacritics_fold_in_tracks_and_artists() {
    let s = scanned(|root| {
        tagged("flac16_44k.flac", &root.join("c.flac"), &Tags { title: Some("Café Society"), artists: &["Zoë Keating"], album: Some("Into the Trees"), ..Default::default() });
    });
    let r = s.lib.search("cafe", 20).unwrap();
    assert_eq!(r.tracks.len(), 1);
    let r = s.lib.search("zoe kea", 20).unwrap();
    assert_eq!(names(&r.artists, |a| &a.name), ["Zoë Keating"]);
}

#[test]
fn hostile_short_and_empty_queries_return_nothing_without_errors() {
    let s = scanned(demo_tree);
    for q in ["", " ", "a", "\"*", "\" OR 1=1 --", "NEAR(", "{title}:x"] {
        let r = s.lib.search(q, 20).unwrap_or_else(|e| panic!("{q:?}: {e}"));
        if q.trim().chars().count() < 2 { assert!(r.tracks.is_empty() && r.albums.is_empty() && r.artists.is_empty(), "{q:?}"); }
    }
}

#[test]
fn missing_tracks_are_not_found() {
    let s = scanned(demo_tree);
    std::fs::remove_file(s.root().join("The Gamma/Night/01.mp3")).unwrap();
    s.rescan();
    let r = s.lib.search("dusk", 20).unwrap();
    assert!(r.tracks.is_empty() && r.albums.is_empty());
}

#[test]
fn play_history_drives_recent_and_most_played() {
    let s = scanned(demo_tree);
    let (dawn, peak, span) = (s.id_of("Dawn"), s.id_of("Peak"), s.id_of("Span"));
    s.lib.record_play(dawn, 100).unwrap();
    s.lib.record_play(peak, 200).unwrap();
    s.lib.record_play(dawn, 300).unwrap();
    let played = s.lib.recent(RecentKind::Played, 10).unwrap().tracks;
    assert_eq!(names(&played, |t| &t.title), ["Dawn", "Peak"]);
    let most = s.lib.recent(RecentKind::MostPlayed, 10).unwrap().tracks;
    assert_eq!(names(&most, |t| &t.title), ["Dawn", "Peak"]);
    assert_eq!(most[0].play_count, 2);
    assert_eq!(most[0].last_played_at, Some(300));
    assert!(matches!(s.lib.record_play(999_999, 1), Err(library::LibError::NotFound(_))));
    let _ = span;
}

#[test]
fn recently_added_albums_newest_first() {
    let s = scanned(demo_tree);
    s.lib.conn().execute("UPDATE albums SET added_at = CASE title WHEN 'Night' THEN 50 WHEN 'Zenith' THEN 40 ELSE 10 END", []).unwrap();
    let r = s.lib.recent(RecentKind::Added, 2).unwrap();
    assert_eq!(names(&r.albums, |a| &a.title), ["Night", "Zenith"]);
    assert!(r.tracks.is_empty());
}

#[test]
fn unplayable_marks_set_and_clear() {
    let s = scanned(demo_tree);
    let path = s.lib.tracks_by_ids(&[s.id_of("Span")]).unwrap()[0].path.clone();
    assert!(s.lib.set_unplayable(&path, Some("DECODE_FAILED")).unwrap());
    assert_eq!(s.lib.track(s.id_of("Span")).unwrap().track.unplayable.as_deref(), Some("DECODE_FAILED"));
    assert!(s.lib.set_unplayable(&path, None).unwrap());
    assert!(s.lib.track(s.id_of("Span")).unwrap().track.unplayable.is_none());
    assert!(!s.lib.set_unplayable("/not/in/library.flac", Some("X")).unwrap());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test search`
Expected: compile errors (`search`, `record_play`, … not found).

- [ ] **Step 3: Implement**

Create `engine/crates/library/src/search.rs`:

```rust
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
        assert_eq!(fts_query("\" OR *", None), None);
        assert_eq!(fts_query("AC/DC", None).as_deref(), Some("\"ac\"* AND \"dc\"*"));
    }
}
```

Create `engine/crates/library/src/history.rs`:

```rust
//! Play history (spec §3.9 auto views) and per-track playability marks (spec §6).
use protocol::{RecentKind, RecentResult};
use rusqlite::params;

use crate::rows::{self, ALBUM_COLS, ALBUM_FROM, TRACK_COLS};
use crate::{LibError, Library, Result};

impl Library {
    pub fn record_play(&self, track_id: i64, at: i64) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
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
```

In `src/lib.rs`, add `pub mod history; pub mod search;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library`
Expected: all pass (8 `search` tests, 1 unit test, earlier tests).

- [ ] **Step 5: Commit**

```bash
git add engine/crates/library
git commit -m "feat(library): FTS5 grouped search, play history and recent views"
```

### Task 8: Playlists — CRUD, reorder, M3U8 import/export

**Files:**
- Create: `engine/crates/library/src/playlists.rs`
- Modify: `engine/crates/library/src/lib.rs` (`pub mod playlists;`)
- Test: `engine/crates/library/tests/playlists.rs`, unit tests in `playlists.rs`

**Interfaces:**
- Consumes:
  - From Task 2: the `playlists` and `playlist_items` tables.
  - From Task 6: `Library::{tracks_by_ids, track_id_for_path}`.
  - From Task 1: `protocol::{Playlist, PlaylistDetail, PlaylistItem, ImportResult}`.
- Produces (methods on `Library`, returning `library::Result`):
  - **Reading:** `playlists() -> Vec<Playlist>` (sorted by name, case-insensitive) and `playlist(id) -> PlaylistDetail`.
  - **Creating and renaming:** `create_playlist(name) -> Playlist` and `rename_playlist(id, name) -> Playlist`. A name is trimmed, must be 1–200 characters, and otherwise gives `InvalidArgument`.
  - **Deleting:** `delete_playlist(id)`.
  - **Editing items:**
    - `add_to_playlist(id, &[i64], index: Option<u32>) -> Playlist`: `None` appends, and an index past the end appends.
    - `remove_from_playlist(id, &[item_id]) -> Playlist`.
    - `reorder_playlist(id, from, to) -> Playlist`: array-move semantics. An index out of range gives `InvalidArgument`.
  - **M3U8:**
    - `import_m3u(&Path) -> ImportResult`: the playlist is named after the file stem, and unresolvable entries are counted in `skipped`.
    - `export_m3u(id, &Path)`: UTF-8 with `#EXTM3U`, `#PLAYLIST:` and `#EXTINF:` lines. Paths inside the playlist file's folder are written relative to it; others are absolute.
  - **Pure helpers:** `library::playlists::{percent_decode(&str) -> String, lexical_normalize(&Path) -> PathBuf}`.
  - Every mutation bumps `updated_at`. Item positions are always 0..n−1 with no gaps.

- [ ] **Step 1: Write the failing tests**

Create `engine/crates/library/tests/playlists.rs`:

```rust
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
    let positions: Vec<i64> = s.lib.conn().prepare("SELECT position FROM playlist_items WHERE playlist_id = ?1 ORDER BY position").unwrap()
        .query_map([p.id], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test playlists`
Expected: compile errors (`create_playlist`, … not found).

- [ ] **Step 3: Implement**

Create `engine/crates/library/src/playlists.rs`:

```rust
//! User playlists and M3U8 import/export (spec §3.9).
use std::path::{Component, Path, PathBuf};

use protocol::{ImportResult, Playlist, PlaylistDetail, PlaylistItem};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};

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
        if b[i] == b'%' && i + 2 < b.len()
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
            Component::ParentDir => { if matches!(out.components().next_back(), Some(Component::Normal(_))) { out.pop(); } }
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
    Ok(tx.prepare("SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn renumber(tx: &Transaction, id: i64, items: &[i64]) -> Result<()> {
    let mut stmt = tx.prepare_cached("UPDATE playlist_items SET position = ?1 WHERE id = ?2")?;
    for (pos, item) in items.iter().enumerate() { stmt.execute(params![pos as i64, item])?; }
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
        let ids = conn.prepare("SELECT id FROM playlists ORDER BY name COLLATE NOCASE, id")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter().map(|id| summary(&conn, id)).collect()
    }

    pub fn playlist(&self, id: i64) -> Result<PlaylistDetail> {
        let conn = self.conn();
        let playlist = summary(&conn, id)?;
        // (item id, track id) in playlist order; the rows come from tracks_by_ids, which keeps order.
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
        if self.conn().execute("DELETE FROM playlists WHERE id = ?1", [id])? == 0 {
            return Err(LibError::NotFound(format!("playlist {id}")));
        }
        Ok(())
    }

    pub fn add_to_playlist(&self, id: i64, track_ids: &[i64], index: Option<u32>) -> Result<Playlist> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        ensure(&tx, id)?;
        let mut items = item_ids(&tx, id)?;
        let mut added = Vec::with_capacity(track_ids.len());
        for t in track_ids {
            let known: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tracks WHERE id = ?1)", [t], |r| r.get(0))?;
            if !known { return Err(LibError::NotFound(format!("track {t}"))); }
            tx.execute("INSERT INTO playlist_items(playlist_id, track_id, position) VALUES (?1, ?2, 0)", params![id, t])?;
            added.push(tx.last_insert_rowid());
        }
        let at = index.map_or(items.len(), |i| (i as usize).min(items.len()));
        items.splice(at..at, added);
        renumber(&tx, id, &items)?;
        tx.commit()?;
        summary(&conn, id)
    }

    pub fn remove_from_playlist(&self, id: i64, item_ids_to_remove: &[i64]) -> Result<Playlist> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        ensure(&tx, id)?;
        if !item_ids_to_remove.is_empty() {
            let marks = vec!["?"; item_ids_to_remove.len()].join(",");
            let mut values: Vec<i64> = vec![id];
            values.extend_from_slice(item_ids_to_remove);
            tx.execute(&format!("DELETE FROM playlist_items WHERE playlist_id = ? AND id IN ({marks})"), params_from_iter(values.iter()))?;
        }
        let items = item_ids(&tx, id)?;
        renumber(&tx, id, &items)?;
        tx.commit()?;
        summary(&conn, id)
    }

    pub fn reorder_playlist(&self, id: i64, from: u32, to: u32) -> Result<Playlist> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
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
                // file:///C:/x → C:/x on Windows-style paths.
                if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { decoded[1..].to_string() } else { decoded }
            }
            None => entry.to_string(),
        };
        let native = if cfg!(windows) { raw.replace('/', "\\") } else { raw.replace('\\', "/") };
        let p = PathBuf::from(native);
        let abs = lexical_normalize(&if p.is_absolute() { p } else { base.join(p) });
        let key = abs.to_string_lossy().into_owned();
        self.track_id_for_path(&key).or_else(|| {
            if !cfg!(windows) { return None; }
            self.conn().query_row("SELECT id FROM tracks WHERE path = ?1 COLLATE NOCASE", [&key], |r| r.get(0)).optional().ok().flatten()
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
```

In `src/lib.rs`, add `pub mod playlists;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library`
Expected: all pass (6 `playlists` tests, 2 unit tests, earlier tests).

Then run: `cargo clippy --manifest-path engine/Cargo.toml -p library --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/library
git commit -m "feat(library): playlists with dense ordering and M3U8 import/export"
```

### Task 9: Folder watcher and change grouping

**Files:**
- Modify: `engine/crates/library/Cargo.toml` (add `notify`, `notify-debouncer-full`), `engine/crates/library/src/lib.rs` (`pub mod watch;`)
- Create: `engine/crates/library/src/watch.rs`
- Test: `engine/crates/library/tests/watch.rs`, unit tests in `watch.rs`

**Interfaces:**
- Consumes: `LibError` (Task 2).
- Produces:
  - **`library::watch::Watcher`:**
    - `new(on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Watcher>`: the debounce is 2 s.
    - `with_timeout(Duration, on_change)`: for tests.
    - `watch(&mut self, &Path) -> Result<()>` (recursive) and `unwatch(&mut self, &Path) -> Result<()>`.
    - `on_change` runs on the debouncer's thread and receives each batch's paths, de-duplicated. Access events are ignored.
  - **`library::watch::DEBOUNCE`:** `Duration::from_secs(2)`.
  - **`library::watch::group_by_folder(folders: &[(i64, PathBuf)], paths: Vec<PathBuf>) -> Vec<(i64, Vec<PathBuf>)>`:** a pure function.
    - Each path is assigned to the folder that contains it. Paths outside every folder are dropped.
    - Within a folder, a path is dropped when another listed path already covers it (an ancestor).
    - A folder with more than `FULL_SCAN_THRESHOLD = 200` remaining paths gets an empty list, meaning "scan the whole folder".
    - Output is ordered by folder id.

- [ ] **Step 1: Write the failing tests**

Create `engine/crates/library/tests/watch.rs`:

```rust
use std::sync::mpsc;
use std::time::Duration;

use library::watch::Watcher;

#[test]
fn reports_created_files_after_the_debounce() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut w = Watcher::with_timeout(Duration::from_millis(200), move |paths| { let _ = tx.send(paths); }).unwrap();
    w.watch(dir.path()).unwrap();
    std::thread::sleep(Duration::from_millis(300)); // let the OS watch settle (FSEvents/inotify)
    std::fs::create_dir(dir.path().join("Album")).unwrap();
    std::fs::write(dir.path().join("Album").join("new.flac"), b"x").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut seen = vec![];
    while std::time::Instant::now() < deadline {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(500)) { seen.extend(batch); }
        if seen.iter().any(|p: &std::path::PathBuf| p.file_name().is_some_and(|n| n == "new.flac" || n == "Album")) { break; }
    }
    assert!(seen.iter().any(|p| p.file_name().is_some_and(|n| n == "new.flac" || n == "Album")), "saw {seen:?}");
}

#[test]
fn unwatched_folders_are_silent() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut w = Watcher::with_timeout(Duration::from_millis(100), move |paths| { let _ = tx.send(paths); }).unwrap();
    w.watch(dir.path()).unwrap();
    w.unwatch(dir.path()).unwrap();
    std::fs::write(dir.path().join("x.flac"), b"x").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(1500)).is_err());
}
```

Add the unit tests at the bottom of `src/watch.rs` (Step 3).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p library --test watch`
Expected: compile error (`library::watch` not found).

- [ ] **Step 3: Implement**

Add `notify.workspace = true` and `notify-debouncer-full.workspace = true` to `[dependencies]`.

Create `engine/crates/library/src/watch.rs`:

```rust
//! Folder watcher (spec §3.9): debounced filesystem events become partial rescans.
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};

use crate::{LibError, Result};

pub const DEBOUNCE: Duration = Duration::from_secs(2);
/// Above this many changed paths in one folder, a full (still incremental) folder scan is cheaper.
pub const FULL_SCAN_THRESHOLD: usize = 200;

pub struct Watcher { inner: Debouncer<RecommendedWatcher, RecommendedCache> }

fn watch_err(e: impl std::fmt::Display) -> LibError { LibError::Io(std::io::Error::other(format!("folder watcher: {e}"))) }

impl Watcher {
    pub fn new(on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Self> {
        Self::with_timeout(DEBOUNCE, on_change)
    }

    pub fn with_timeout(timeout: Duration, on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Self> {
        let inner = new_debouncer(timeout, None, move |res: DebounceEventResult| match res {
            Ok(events) => {
                let mut paths: Vec<PathBuf> = events
                    .iter()
                    .filter(|e| !matches!(e.kind, EventKind::Access(_)))
                    .flat_map(|e| e.paths.iter().cloned())
                    .collect();
                paths.sort();
                paths.dedup();
                if !paths.is_empty() { on_change(paths); }
            }
            Err(errors) => {
                for e in errors { tracing::warn!(error = %e, "folder watcher error"); }
            }
        })
        .map_err(watch_err)?;
        Ok(Watcher { inner })
    }

    pub fn watch(&mut self, path: &Path) -> Result<()> {
        self.inner.watch(path, RecursiveMode::Recursive).map_err(watch_err)
    }

    pub fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.inner.unwatch(path).map_err(watch_err)
    }
}

/// Assigns changed paths to library folders for partial rescans (see the module docs on `Interfaces`).
pub fn group_by_folder(folders: &[(i64, PathBuf)], paths: Vec<PathBuf>) -> Vec<(i64, Vec<PathBuf>)> {
    let mut sorted: Vec<&(i64, PathBuf)> = folders.iter().collect();
    sorted.sort_by_key(|(id, _)| *id);
    let mut out = vec![];
    for (id, root) in sorted {
        let mut mine: Vec<PathBuf> = paths.iter().filter(|p| p.starts_with(root)).cloned().collect();
        if mine.is_empty() { continue; }
        mine.sort();
        mine.dedup();
        let mut kept: Vec<PathBuf> = vec![];
        for p in mine {
            if !kept.iter().any(|k| p.starts_with(k)) { kept.push(p); }
        }
        if kept.len() > FULL_SCAN_THRESHOLD || kept.iter().any(|p| p == root) { kept.clear(); }
        out.push((*id, kept));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(s: &str) -> PathBuf { PathBuf::from(s) }

    #[test]
    fn groups_by_folder_and_collapses_descendants() {
        let folders = [(2, pb("/m/b")), (1, pb("/m/a"))];
        let got = group_by_folder(&folders, vec![pb("/m/a/x/1.flac"), pb("/m/a/x"), pb("/m/b/2.flac"), pb("/elsewhere/3.flac"), pb("/m/a/y.flac")]);
        assert_eq!(got, vec![(1, vec![pb("/m/a/x"), pb("/m/a/y.flac")]), (2, vec![pb("/m/b/2.flac")])]);
    }

    #[test]
    fn many_changes_or_the_root_itself_mean_full_scan() {
        let folders = [(1, pb("/m/a"))];
        let many: Vec<PathBuf> = (0..=FULL_SCAN_THRESHOLD).map(|i| pb(&format!("/m/a/{i}.flac"))).collect();
        assert_eq!(group_by_folder(&folders, many), vec![(1, vec![])]);
        assert_eq!(group_by_folder(&folders, vec![pb("/m/a")]), vec![(1, vec![])]);
    }

    #[test]
    fn sibling_prefixes_are_not_ancestors() {
        let folders = [(1, pb("/m/a"))];
        assert!(group_by_folder(&folders, vec![pb("/m/ab/x.flac")]).is_empty());
    }
}
```

Add `pub mod watch;` to `src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p library`
Expected: all pass (2 watcher integration tests, 3 unit tests, earlier tests).

The watcher tests touch the real filesystem. If `reports_created_files_after_the_debounce` flakes on a slow machine, don't loosen the assertion: raise the 10 s deadline instead.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/library engine/Cargo.lock
git commit -m "feat(library): debounced folder watcher and change grouping for partial rescans"
```

### Task 10: 100k-track benchmark (spec §1.4, §7)

**Files:**
- Create: `engine/crates/library/tests/bench_100k.rs`
- Modify: `README.md` (how to run it), library code **only** if a target is missed (see Step 4)

**Interfaces:**
- Consumes: `Library::{open, add_folder, scanner, albums, artists, tracks, search, album, artist}` (Tasks 2–7).
- Produces: an `#[ignore]` test that M2's exit checklist runs. It needs no fixtures. The corpus is generated into `CARGO_TARGET_TMPDIR` and reused across runs.

- [ ] **Step 1: Write the benchmark**

Create `engine/crates/library/tests/bench_100k.rs`:

```rust
//! Spec §1.4: first scan of 100k tracks < 2 min on an SSD; paging/search queries < 50 ms.
//! Run: cargo test --release --manifest-path engine/Cargo.toml -p library --test bench_100k -- --ignored --nocapture
//! AURORA_BENCH_TRACKS overrides the corpus size (default 100000).
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use library::Library;
use protocol::{ListFilter, ListQuery, SortKey};
use rayon::prelude::*;

const SYLLABLES: [&str; 20] = ["ka", "ve", "lo", "mi", "ru", "sa", "te", "no", "pi", "da", "ze", "fu", "ri", "go", "ha", "ne", "so", "bi", "ta", "ly"];

/// A deterministic pronounceable word for `n` (distinct for distinct `n`).
fn word(mut n: usize) -> String {
    let mut w = String::new();
    loop {
        w.push_str(SYLLABLES[n % 20]);
        n /= 20;
        if n == 0 { break; }
    }
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn info_chunk(id: &[u8; 4], value: &str) -> Vec<u8> {
    let mut data = value.as_bytes().to_vec();
    data.push(0);
    let mut out = id.to_vec();
    out.extend((data.len() as u32).to_le_bytes());
    if data.len() % 2 == 1 { data.push(0); }
    out.extend(data);
    out
}

/// 10 ms of 44.1 kHz/16-bit stereo silence with a RIFF INFO tag (INAM/IART/IPRD/IGNR/ITRK).
fn tiny_wav(title: &str, artist: &str, album: &str, genre: &str, track: usize) -> Vec<u8> {
    let mut list = b"INFO".to_vec();
    for (id, v) in [(b"INAM", title), (b"IART", artist), (b"IPRD", album), (b"IGNR", genre)] { list.extend(info_chunk(id, v)); }
    list.extend(info_chunk(b"ITRK", &track.to_string()));
    let pcm = vec![0u8; 441 * 4];
    let mut body = b"WAVE".to_vec();
    body.extend(b"fmt ");
    body.extend(16u32.to_le_bytes());
    body.extend(1u16.to_le_bytes()); // PCM
    body.extend(2u16.to_le_bytes()); // channels
    body.extend(44100u32.to_le_bytes());
    body.extend((44100u32 * 4).to_le_bytes());
    body.extend(4u16.to_le_bytes());
    body.extend(16u16.to_le_bytes());
    body.extend(b"LIST");
    body.extend((list.len() as u32).to_le_bytes());
    body.extend(list);
    body.extend(b"data");
    body.extend((pcm.len() as u32).to_le_bytes());
    body.extend(pcm);
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

const PER_ALBUM: usize = 10;
const ALBUMS_PER_ARTIST: usize = 5;
const GENRES: [&str; 12] = ["Ambient", "Jazz", "Rock", "Folk", "Soul", "Funk", "House", "Techno", "Classical", "Blues", "Pop", "Metal"];

fn artist_name(a: usize) -> String { format!("{} {}", word(a + 400), word(a * 7 + 13)) }

fn generate(root: &Path, n: usize) {
    let _ = std::fs::remove_dir_all(root);
    let albums = n.div_ceil(PER_ALBUM);
    (0..albums).into_par_iter().for_each(|al| {
        let a = al / ALBUMS_PER_ARTIST;
        let artist = artist_name(a);
        let album = format!("{} {}", word(al * 3 + 1), word(al + 9000));
        let dir = root.join(&artist).join(&album);
        std::fs::create_dir_all(&dir).unwrap();
        for t in 0..PER_ALBUM {
            let i = al * PER_ALBUM + t;
            if i >= n { break; }
            let title = format!("{} {}", word(i * 31 + 5), word(i + 77));
            std::fs::write(dir.join(format!("{:02}.wav", t + 1)), tiny_wav(&title, &artist, &album, GENRES[al % GENRES.len()], t + 1)).unwrap();
        }
    });
    std::fs::write(root.join(".complete"), b"").unwrap();
}

fn median(mut f: impl FnMut()) -> Duration {
    f(); // warm the page cache and statement cache
    let mut times: Vec<Duration> = (0..5).map(|_| { let t = Instant::now(); f(); t.elapsed() }).collect();
    times.sort();
    times[2]
}

#[test]
#[ignore = "benchmark; run with --release --ignored"]
fn library_100k_meets_targets() {
    let n: usize = std::env::var("AURORA_BENCH_TRACKS").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let corpus = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("bench-corpus-{n}"));
    if !corpus.join(".complete").exists() {
        let t = Instant::now();
        generate(&corpus, n);
        println!("generated {n} files in {:.1} s", t.elapsed().as_secs_f64());
    }
    let data = tempfile::tempdir().unwrap();
    let (lib, _) = Library::open(data.path()).unwrap();
    let folder = lib.add_folder(&corpus).unwrap().id;

    let t = Instant::now();
    let summary = lib.scanner(Default::default()).unwrap().scan(folder, &[], true, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    let scan = t.elapsed();
    println!("first scan: {} tracks in {:.1} s", summary.added, scan.as_secs_f64());
    assert_eq!(summary.added as usize, n);
    assert!(scan < Duration::from_secs(120), "first scan took {scan:?} (target < 120 s)");

    let t = Instant::now();
    let again = lib.scanner(Default::default()).unwrap().scan(folder, &[], true, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    println!("rescan (nothing changed): {:.1} s", t.elapsed().as_secs_f64());
    assert_eq!(again.unchanged as usize, n);

    let some_album = lib.albums(&ListQuery { offset: Some(1234), limit: Some(1), ..Default::default() }).unwrap().items[0].id;
    let some_artist = lib.artists(&ListQuery { offset: Some(321), limit: Some(1), ..Default::default() }).unwrap().items[0].id;
    let artist_prefix: String = artist_name(4217 % (n / PER_ALBUM / ALBUMS_PER_ARTIST).max(1)).chars().take(5).collect();
    let page = |sort, desc, offset| ListQuery { sort: Some(sort), desc: Some(desc), offset: Some(offset), limit: Some(100), filter: None };

    let cases: Vec<(&str, Box<dyn Fn() + '_>)> = vec![
        ("albums by title", Box::new(|| { lib.albums(&page(SortKey::Title, false, 0)).unwrap(); })),
        ("albums by artist, deep page", Box::new(|| { lib.albums(&page(SortKey::Artist, false, 4000)).unwrap(); })),
        ("albums by year desc", Box::new(|| { lib.albums(&page(SortKey::Year, true, 0)).unwrap(); })),
        ("albums recently added", Box::new(|| { lib.albums(&page(SortKey::AddedAt, true, 0)).unwrap(); })),
        ("tracks by title, offset 50k", Box::new(|| { lib.tracks(&page(SortKey::Title, false, (n / 2) as u32)).unwrap(); })),
        ("tracks of one artist", Box::new(|| {
            lib.tracks(&ListQuery { filter: Some(ListFilter { artist_id: Some(some_artist), ..Default::default() }), ..Default::default() }).unwrap();
        })),
        ("albums of one artist", Box::new(|| {
            lib.albums(&ListQuery { filter: Some(ListFilter { artist_id: Some(some_artist), ..Default::default() }), ..Default::default() }).unwrap();
        })),
        ("artists page", Box::new(|| { lib.artists(&page(SortKey::Name, false, 500)).unwrap(); })),
        ("search artist prefix", Box::new(|| { lib.search(&artist_prefix, 20).unwrap(); })),
        ("search two words", Box::new(|| { lib.search("kave lo", 20).unwrap(); })),
        ("album detail", Box::new(|| { lib.album(some_album).unwrap(); })),
        ("artist detail", Box::new(|| { lib.artist(some_artist).unwrap(); })),
    ];
    let mut failures = vec![];
    for (name, f) in &cases {
        let m = median(f);
        println!("{name:<32} {:>7.2} ms", m.as_secs_f64() * 1000.0);
        if m >= Duration::from_millis(50) { failures.push(format!("{name}: {m:?}")); }
    }
    assert!(failures.is_empty(), "queries over 50 ms: {failures:?}");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test --release --manifest-path engine/Cargo.toml -p library --test bench_100k -- --ignored --nocapture`
Expected:
- the corpus is generated once (≈100k small files, about 200 MB);
- the scan prints `first scan: 100000 tracks in <N> s` with N < 120;
- every query line is below 50 ms;
- the test passes.

On Windows, Defender's real-time scanning can slow file creation and the first walk badly. Check that before treating a slow scan as a code problem: exclude the target dir, or confirm with a second clean run.

- [ ] **Step 3: Record the numbers**

Add a "Library benchmark" section to `README.md`. It should give the command above and a table of the numbers from this machine (scan seconds, each query's median ms), with the OS and CPU.

- [ ] **Step 4: If a target is missed, fix the cause, not the threshold**

Never raise the 120 s or 50 ms thresholds. Use these remedies, in this order, and re-run after each:

**Scan too slow:**
1. Run with `--release` (debug builds are 10× slower).
2. Check `BATCH` (500) in `scan.rs`. Fewer, larger transactions cut fsyncs, so try 2000.
3. Confirm `synchronous=NORMAL` and WAL are actually active (Task 2 test).

**A query too slow:** run `EXPLAIN QUERY PLAN` on its SQL in a scratch test, using `lib.conn()`.
1. For a `SCAN tracks` on a filtered track list: add a covering index for that filter's sort. For example, `CREATE INDEX tracks_artist_sort ON tracks(artist COLLATE NOCASE)`.
2. For the album artist filter: rewrite its three-way `OR` as a `UNION` of `album_id` subqueries feeding `a.id IN (…)`.
3. For a deep `OFFSET` page: that's inherent to OFFSET paging, but 50k rows on an index should be about 10 ms. If it isn't, check that the sort uses `tracks_title`.

Add new indexes as a **new migration** (`src/schema/002_*.sql`, appended to `MIGRATIONS`), never by editing `001_init.sql`. Then re-run the full library test suite.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/library README.md
git commit -m "test(library): 100k-track scan and query benchmark against spec targets"
```

### Task 11: Queue model — entries, shuffle, repeat (pure)

**Files:**
- Create: `engine/crates/engine-core/src/player/queue.rs` (with unit tests)
- Modify: `engine/crates/engine-core/src/player/mod.rs` (only `pub mod queue;`; Task 12 wires it in)

**Interfaces:**
- Consumes: `protocol::{QueueItem, RepeatMode}`.
- Produces (`engine_core::player::queue`):
  - **`NewEntry`:** `{ path: PathBuf, title: Option<String>, track_id: Option<i64>, artist: Option<String>, album: Option<String>, artwork_id: Option<i64>, duration_ms: Option<u64> }`. It derives Clone, Debug, PartialEq, Default and has `NewEntry::from_path(impl Into<PathBuf>)`.
  - **`Entry`:** `{ id: u32, path, title: String, track_id, artist, album, artwork_id, duration_ms }`. `Entry::item(&self) -> QueueItem`.
  - **`Queue`:**
    - Construction and access: `new(seed: u64)`, `len()`, `is_empty()`, `get(i) -> Option<&Entry>`, `entries() -> &[Entry]`, `index_of(id) -> Option<usize>`, `shuffle() -> bool`, `repeat() -> RepeatMode`, `items() -> Vec<QueueItem>`.
    - Replacing:
      - `replace(Vec<NewEntry>, start) -> usize` returns the index to start at.
      - `restore(Vec<NewEntry>, shuffle, repeat)` keeps the given order exactly.
    - Editing:
      - `add(Vec<NewEntry>) -> Vec<u32>` and `insert_after(current: Option<usize>, Vec<NewEntry>) -> Vec<u32>`.
      - `move_item(from, to) -> bool` and `remove(&[u32]) -> usize`.
      - `clear()`.
    - Modes: `set_shuffle(on, current: Option<usize>)` and `set_repeat(RepeatMode)`.
    - Navigation: `auto_next(idx) -> Option<usize>`, `user_next(idx) -> Option<usize>`, `user_prev(idx) -> Option<usize>`.
  - **Semantics:**
    - **IDs** are unique for the queue's lifetime: they keep counting across `replace`/`clear`.
    - **Titles** default to the file stem when `title` is `None` or blank.
    - **Shuffle on:** the current entry moves to index 0 and every other entry is shuffled behind it (Fisher–Yates, xorshift64, seeded).
    - **Shuffle off:** the pre-shuffle order returns. Entries added while shuffled keep their insertion point relative to the current entry; removed entries stay removed.
    - **`auto_next`** (natural end of track):
      - `One` gives the same index;
      - otherwise the next index;
      - at the end, `All` wraps to 0 and `Off` gives `None`.
    - **`user_next`** (Next button, or skipping a broken file): the next index; at the end it wraps to 0 when repeat isn't `Off`, else `None`.
    - **`user_prev`:** the previous index; at index 0 it wraps to the last index when repeat isn't `Off`, else stays at 0.

- [ ] **Step 1: Write the failing tests**

Create `engine/crates/engine-core/src/player/queue.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::RepeatMode;

    fn entries(names: &[&str]) -> Vec<NewEntry> { names.iter().map(|n| NewEntry::from_path(format!("/m/{n}.flac"))).collect() }
    fn titles(q: &Queue) -> Vec<String> { q.entries().iter().map(|e| e.title.clone()).collect() }

    #[test]
    fn replace_assigns_fresh_ids_and_stem_titles() {
        let mut q = Queue::new(7);
        assert_eq!(q.replace(entries(&["a", "b"]), 1), 1);
        assert_eq!(titles(&q), ["a", "b"]);
        assert_eq!(q.entries().iter().map(|e| e.id).collect::<Vec<_>>(), [1, 2]);
        q.replace(vec![NewEntry { title: Some("  ".into()), ..NewEntry::from_path("/m/c.flac") }, NewEntry { title: Some("Named".into()), ..NewEntry::from_path("/m/d.flac") }], 9);
        assert_eq!(titles(&q), ["c", "Named"]);
        assert_eq!(q.entries()[0].id, 3, "ids keep counting");
        assert_eq!(q.replace(vec![], 3), 0);
    }

    #[test]
    fn shuffle_keeps_current_first_and_unshuffle_restores_order() {
        let mut q = Queue::new(42);
        let names: Vec<String> = (0..20).map(|i| format!("t{i:02}")).collect();
        q.replace(entries(&names.iter().map(String::as_str).collect::<Vec<_>>()), 0);
        q.set_shuffle(true, Some(5));
        assert!(q.shuffle());
        assert_eq!(q.entries()[0].title, "t05");
        let mut sorted = titles(&q);
        assert_ne!(sorted, names, "order changed");
        sorted.sort();
        assert_eq!(sorted, names, "no entry lost or duplicated");
        q.set_shuffle(false, Some(0));
        assert_eq!(titles(&q), names);
    }

    #[test]
    fn replace_while_shuffled_starts_with_the_chosen_entry() {
        let mut q = Queue::new(3);
        q.set_shuffle(true, None);
        let start = q.replace(entries(&["a", "b", "c", "d"]), 2);
        assert_eq!(start, 0);
        assert_eq!(q.entries()[0].title, "c");
        q.set_shuffle(false, Some(0));
        assert_eq!(titles(&q), ["a", "b", "c", "d"]);
    }

    #[test]
    fn insert_after_and_add_survive_unshuffle() {
        let mut q = Queue::new(9);
        q.replace(entries(&["a", "b", "c", "d"]), 0);
        q.set_shuffle(true, Some(1)); // "b" is current, at index 0
        let ids = q.insert_after(Some(0), entries(&["x"]));
        assert_eq!(q.entries()[1].title, "x");
        assert_eq!(ids.len(), 1);
        q.add(entries(&["z"]));
        assert_eq!(q.entries().last().unwrap().title, "z");
        q.set_shuffle(false, None);
        assert_eq!(titles(&q), ["a", "b", "x", "c", "d", "z"]);
    }

    #[test]
    fn insert_into_empty_queue_and_unshuffled_insert() {
        let mut q = Queue::new(1);
        q.insert_after(None, entries(&["a"]));
        q.insert_after(Some(0), entries(&["b", "c"]));
        q.insert_after(Some(0), entries(&["n"]));
        assert_eq!(titles(&q), ["a", "n", "b", "c"]);
    }

    #[test]
    fn move_and_remove() {
        let mut q = Queue::new(1);
        q.replace(entries(&["a", "b", "c", "d"]), 0);
        assert!(q.move_item(0, 3));
        assert_eq!(titles(&q), ["b", "c", "d", "a"]);
        assert!(!q.move_item(0, 4));
        let b = q.entries()[0].id;
        let d = q.entries()[2].id;
        assert_eq!(q.remove(&[b, d, 999]), 2);
        assert_eq!(titles(&q), ["c", "a"]);
        assert_eq!(q.index_of(b), None);
        q.set_shuffle(true, Some(0));
        q.set_shuffle(false, None);
        assert_eq!(titles(&q), ["c", "a"], "removed entries don't come back on unshuffle");
    }

    #[test]
    fn next_and_prev_follow_repeat_mode() {
        let mut q = Queue::new(1);
        assert_eq!((q.auto_next(0), q.user_next(0), q.user_prev(0)), (None, None, None));
        q.replace(entries(&["a", "b", "c"]), 0);
        assert_eq!((q.auto_next(0), q.auto_next(2), q.user_next(2), q.user_prev(0)), (Some(1), None, None, Some(0)));
        q.set_repeat(RepeatMode::All);
        assert_eq!((q.auto_next(2), q.user_next(2), q.user_prev(0)), (Some(0), Some(0), Some(2)));
        q.set_repeat(RepeatMode::One);
        assert_eq!((q.auto_next(1), q.user_next(1), q.user_next(2)), (Some(1), Some(2), Some(0)));
        assert_eq!(q.repeat(), RepeatMode::One);
    }

    #[test]
    fn restore_keeps_order_and_flags() {
        let mut q = Queue::new(5);
        q.restore(entries(&["c", "a", "b"]), true, RepeatMode::All);
        assert_eq!(titles(&q), ["c", "a", "b"]);
        assert!(q.shuffle());
        assert_eq!(q.repeat(), RepeatMode::All);
        q.set_shuffle(false, None);
        assert_eq!(titles(&q), ["c", "a", "b"], "the restored order is the original order");
    }

    #[test]
    fn items_carry_metadata() {
        let mut q = Queue::new(1);
        q.replace(vec![NewEntry { track_id: Some(9), artist: Some("A".into()), album: Some("Al".into()), artwork_id: Some(4), duration_ms: Some(1000), ..NewEntry::from_path("/m/x.flac") }], 0);
        let it = &q.items()[0];
        assert_eq!((it.track_id, it.artist.as_deref(), it.album.as_deref(), it.artwork_id, it.duration_ms), (Some(9), Some("A"), Some("Al"), Some(4), Some(1000)));
        assert_eq!(it.title, "x");
    }

    #[test]
    fn same_seed_same_shuffle() {
        let names: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let run = || { let mut q = Queue::new(11); q.replace(entries(&refs), 0); q.set_shuffle(true, Some(0)); titles(&q) };
        assert_eq!(run(), run());
    }
}
```

Add `pub mod queue;` to `engine/crates/engine-core/src/player/mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core --lib player::queue`
Expected: compile errors (`Queue`, `NewEntry` not found).

- [ ] **Step 3: Implement the model**

Put this above the test module in `queue.rs`:

```rust
//! The play queue as a pure model (spec §4.3 `queue.*`, §7 "queue/shuffle/repeat state machine").
//! The player stores positions into it and re-resolves them by entry id after every edit.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use protocol::{QueueItem, RepeatMode};

/// A track to enqueue: a path plus whatever library metadata is known.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NewEntry {
    pub path: PathBuf,
    pub title: Option<String>,
    pub track_id: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_id: Option<i64>,
    pub duration_ms: Option<u64>,
}

impl NewEntry {
    pub fn from_path(path: impl Into<PathBuf>) -> Self { NewEntry { path: path.into(), ..Default::default() } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u32,
    pub path: PathBuf,
    pub title: String,
    pub track_id: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_id: Option<i64>,
    pub duration_ms: Option<u64>,
}

impl Entry {
    pub fn item(&self) -> QueueItem {
        QueueItem {
            id: self.id, path: self.path.to_string_lossy().into_owned(), title: self.title.clone(), track_id: self.track_id,
            artist: self.artist.clone(), album: self.album.clone(), artwork_id: self.artwork_id, duration_ms: self.duration_ms,
        }
    }
}

fn stem(p: &Path) -> String { p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() }

pub struct Queue {
    entries: Vec<Entry>,
    next_id: u32,
    shuffle: bool,
    repeat: RepeatMode,
    /// Entry ids in un-shuffled order. Mirrors `entries` while shuffle is off.
    original: Vec<u32>,
    rng: u64,
}

impl Queue {
    pub fn new(seed: u64) -> Self {
        Queue { entries: vec![], next_id: 1, shuffle: false, repeat: RepeatMode::Off, original: vec![], rng: seed | 1 }
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn get(&self, i: usize) -> Option<&Entry> { self.entries.get(i) }
    pub fn entries(&self) -> &[Entry] { &self.entries }
    pub fn index_of(&self, id: u32) -> Option<usize> { self.entries.iter().position(|e| e.id == id) }
    pub fn shuffle(&self) -> bool { self.shuffle }
    pub fn repeat(&self) -> RepeatMode { self.repeat }
    pub fn items(&self) -> Vec<QueueItem> { self.entries.iter().map(Entry::item).collect() }

    fn make(&mut self, items: Vec<NewEntry>) -> Vec<Entry> {
        items
            .into_iter()
            .map(|n| {
                let id = self.next_id;
                self.next_id += 1;
                let title = n.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| stem(&n.path));
                Entry { id, path: n.path, title, track_id: n.track_id, artist: n.artist, album: n.album, artwork_id: n.artwork_id, duration_ms: n.duration_ms }
            })
            .collect()
    }

    fn sync_original(&mut self) {
        if !self.shuffle { self.original = self.entries.iter().map(|e| e.id).collect(); }
    }

    fn rand(&mut self) -> u64 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn shuffle_from(&mut self, start: usize) {
        let n = self.entries.len();
        if n <= start + 1 { return; }
        for i in (start + 1..n).rev() {
            let j = start + (self.rand() % (i - start + 1) as u64) as usize;
            self.entries.swap(i, j);
        }
    }

    /// Replaces the queue; returns the index to start at. While shuffled, the chosen entry moves to
    /// the front and the rest are shuffled behind it.
    pub fn replace(&mut self, items: Vec<NewEntry>, start: usize) -> usize {
        self.entries = self.make(items);
        self.original = self.entries.iter().map(|e| e.id).collect();
        if self.entries.is_empty() { return 0; }
        let start = start.min(self.entries.len() - 1);
        if !self.shuffle { return start; }
        let e = self.entries.remove(start);
        self.entries.insert(0, e);
        self.shuffle_from(1);
        0
    }

    /// Restores a saved queue in exactly the given order; `shuffle` only records the mode.
    pub fn restore(&mut self, items: Vec<NewEntry>, shuffle: bool, repeat: RepeatMode) {
        self.entries = self.make(items);
        self.original = self.entries.iter().map(|e| e.id).collect();
        self.shuffle = shuffle;
        self.repeat = repeat;
    }

    pub fn add(&mut self, items: Vec<NewEntry>) -> Vec<u32> {
        let new = self.make(items);
        let ids: Vec<u32> = new.iter().map(|e| e.id).collect();
        self.entries.extend(new);
        if self.shuffle { self.original.extend(&ids); } else { self.sync_original(); }
        ids
    }

    /// Inserts right after `current` (at the front when there's no current entry).
    pub fn insert_after(&mut self, current: Option<usize>, items: Vec<NewEntry>) -> Vec<u32> {
        let new = self.make(items);
        let ids: Vec<u32> = new.iter().map(|e| e.id).collect();
        let at = current.map_or(0, |c| (c + 1).min(self.entries.len()));
        let anchor = current.and_then(|c| self.entries.get(c)).map(|e| e.id);
        self.entries.splice(at..at, new);
        if self.shuffle {
            let pos = anchor.and_then(|a| self.original.iter().position(|id| *id == a)).map_or(self.original.len(), |p| p + 1);
            self.original.splice(pos..pos, ids.iter().copied());
        } else {
            self.sync_original();
        }
        ids
    }

    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from >= self.entries.len() || to >= self.entries.len() { return false; }
        let e = self.entries.remove(from);
        self.entries.insert(to, e);
        self.sync_original();
        true
    }

    pub fn remove(&mut self, ids: &[u32]) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| !ids.contains(&e.id));
        self.original.retain(|id| !ids.contains(id));
        before - self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.original.clear();
    }

    pub fn set_shuffle(&mut self, on: bool, current: Option<usize>) {
        if on == self.shuffle { return; }
        self.shuffle = on;
        if on {
            self.original = self.entries.iter().map(|e| e.id).collect();
            match current.filter(|c| *c < self.entries.len()) {
                Some(c) => {
                    let e = self.entries.remove(c);
                    self.entries.insert(0, e);
                    self.shuffle_from(1);
                }
                None => self.shuffle_from(0),
            }
        } else {
            let pos: HashMap<u32, usize> = self.original.iter().enumerate().map(|(i, id)| (*id, i)).collect();
            self.entries.sort_by_key(|e| pos.get(&e.id).copied().unwrap_or(usize::MAX));
            self.sync_original();
        }
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) { self.repeat = repeat; }

    /// What plays when `idx` ends by itself.
    pub fn auto_next(&self, idx: usize) -> Option<usize> {
        let n = self.entries.len();
        if n == 0 { return None; }
        match self.repeat {
            RepeatMode::One => Some(idx.min(n - 1)),
            _ if idx + 1 < n => Some(idx + 1),
            RepeatMode::All => Some(0),
            RepeatMode::Off => None,
        }
    }

    /// What the Next button (or skipping an unplayable entry) moves to.
    pub fn user_next(&self, idx: usize) -> Option<usize> {
        let n = self.entries.len();
        if idx + 1 < n { Some(idx + 1) } else if n > 0 && self.repeat != RepeatMode::Off { Some(0) } else { None }
    }

    /// What the Previous button moves to (the player restarts the current track instead after 3 s).
    pub fn user_prev(&self, idx: usize) -> Option<usize> {
        let n = self.entries.len();
        if n == 0 { return None; }
        if idx > 0 { Some(idx.min(n) - 1) } else if self.repeat != RepeatMode::Off { Some(n - 1) } else { Some(0) }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core --lib player::queue`
Expected: 10 passed.

If `shuffle_keeps_current_first_and_unshuffle_restores_order` happens to produce the identity order for seed 42, change the seed. The other assertions in that test are seed-independent.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core/src/player/queue.rs engine/crates/engine-core/src/player/mod.rs
git commit -m "feat(player): pure queue model with shuffle, repeat and stable entry ids"
```

### Task 12: Player integration — queue editing, id remapping, prefeed resync, Played, Restore

**Files:**
- Modify: `engine/crates/engine-core/src/player/mod.rs` (full replacement below)
- Modify (compile fixes): `engine/crates/engine-core/tests/player.rs` (`queue` helper), `engine/crates/aurora-engine/src/main.rs` (`play`), `engine/crates/aurora-engine/src/rpc/dispatch.rs` (`queue.setPaths`), `engine/crates/aurora-engine/src/rpc/mod.rs` (`PlayerEvent::Played` arm)
- Test: `engine/crates/engine-core/tests/queue_player.rs`

**Interfaces:**
- Consumes: `player::queue::{Queue, NewEntry, Entry}` (Task 11), protocol v2 fields (Task 1).
- Produces (`engine_core::player`):
  - **`Command` (changed variant):** `SetQueue { items: Vec<NewEntry>, start: usize }`.
  - **`Command` (new variants):** `Add(Vec<NewEntry>)`, `InsertNext(Vec<NewEntry>)`, `Move { from: usize, to: usize }`, `Remove(Vec<u32>)`, `Clear`, `SetShuffle(bool)`, `SetRepeat(RepeatMode)`, `Restore(Box<Restore>)`.
  - **`Restore`:** `{ items: Vec<NewEntry>, current: usize, position_ms: u64, shuffle: bool, repeat: RepeatMode, volume: f32, mode: OutputMode }`. The player ends up **paused** at `position_ms`; the first Play opens the stream there.
  - **`PlayerEvent::Played { track_id: Option<i64>, path: PathBuf }`:** sent once per play when listening reaches `played_threshold_ms(duration)`, which is min(50 %, 4 min). Seeking forward doesn't count as listening; carried listening survives seeks and mode/device restarts.
  - **Handles:**
    - `PlayerHandle::commander() -> Commander`, a cloneable sender used by OS media keys (Task 15).
    - `Commander::send(Command)`.
  - **Functions:** `pub fn played_threshold_ms(duration_ms: Option<u64>) -> u64`. Re-exports `NewEntry`, `Entry`, `Queue`.
  - **`PlayerState` is now complete:** `queue_item_id`, `shuffle`, `repeat` and track metadata are filled in. `duration_ms` falls back to the entry's library duration when no stream is open (restored sessions).
  - **Queue edits:**
    - They never stop playback unless the playing entry is removed, in which case the entry now at its position plays (or playback stops at the end).
    - Edits that change what follows the audible track resync buffered audio by seeking to the current position.
  - **Seek while paused with no stream** (restored session, device loss) moves the resume point.

- [ ] **Step 1: Write the failing tests**

Create `engine/crates/engine-core/tests/queue_player.rs`:

```rust
mod common;
use common::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use engine_core::output::null::{NullBackend, NullHandle, NullMode};
use engine_core::player::{spawn_player, Command, NewEntry, PlayerConfig, PlayerEvent, PlayerHandle, Restore};
use protocol::*;

const GAP_A: usize = 110_583;
const GAP_B: usize = 6 * 44_100 - GAP_A;
const SHORT: usize = 22_050;

fn setup() -> (PlayerHandle, NullHandle, Receiver<PlayerEvent>) {
    let (backend, h) = NullBackend::new(NullMode::Manual);
    let (tx, rx) = crossbeam_channel::unbounded();
    (spawn_player(Arc::new(backend), PlayerConfig::default(), tx), h, rx)
}

fn items(names: &[&str]) -> Vec<NewEntry> { names.iter().map(|n| NewEntry::from_path(fixture(n))).collect() }

fn play(p: &PlayerHandle, names: &[&str]) {
    p.send(Command::SetQueue { items: items(names), start: 0 });
    p.send(Command::Play(None));
}

fn wait_for(p: &PlayerHandle, cond: impl Fn(&PlayerState) -> bool) {
    let start = Instant::now();
    while !cond(&p.state()) { assert!(start.elapsed() < Duration::from_secs(10), "state: {:?}", p.state()); std::thread::sleep(Duration::from_millis(2)); }
}

fn pull_frames(h: &NullHandle, frames: usize) {
    let mut got = 0;
    let start = Instant::now();
    while got < frames { got += h.pull((frames - got).min(4096)); assert!(start.elapsed() < Duration::from_secs(10)); std::thread::sleep(Duration::from_micros(200)); }
}

/// Pulls until Ended (or `max_frames`), returning the events seen.
fn run(h: &NullHandle, rx: &Receiver<PlayerEvent>, max_frames: usize) -> Vec<PlayerEvent> {
    let start = Instant::now();
    let (mut events, mut pulled) = (vec![], 0);
    loop {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out; events: {events:?}");
        let n = h.pull(4096.min(max_frames - pulled));
        pulled += n;
        events.extend(rx.try_iter());
        if pulled >= max_frames || events.iter().any(|e| matches!(e, PlayerEvent::Ended)) { return events; }
        if n == 0 { std::thread::sleep(Duration::from_millis(1)); }
    }
}

fn track_names(events: &[PlayerEvent]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for e in events {
        if let PlayerEvent::State(s) = e && let Some(t) = &s.track {
            let name = std::path::Path::new(&t.path).file_stem().unwrap().to_string_lossy().into_owned();
            if out.last() != Some(&name) && s.status == PlaybackStatus::Playing { out.push(name); }
        }
    }
    out
}

/// Tracks that started playing after `current` (periodic state ticks may repeat `current` first).
fn upcoming(events: &[PlayerEvent], current: &str) -> Vec<String> {
    track_names(events).into_iter().skip_while(|n| n == current).collect()
}

fn last_queue(rx: &Receiver<PlayerEvent>) -> QueueChanged {
    let start = Instant::now();
    let mut last = None;
    while start.elapsed() < Duration::from_millis(500) {
        for e in rx.try_iter() { if let PlayerEvent::Queue(q) = e { last = Some(q); } }
        std::thread::sleep(Duration::from_millis(5));
    }
    last.expect("a queue.changed event")
}

/// Plays gap_a for 1 s and lets the ring fill past its end, so gap_b is already decoded (prefed).
fn into_prefeed(p: &PlayerHandle, h: &NullHandle, names: &[&str]) {
    play(p, names);
    pull_frames(h, 44_100);
    std::thread::sleep(Duration::from_millis(300));
}

#[test]
fn insert_next_during_prefeed_replaces_buffered_track() {
    let (p, h, rx) = setup();
    into_prefeed(&p, &h, &["gap_a.flac", "gap_b.flac"]);
    p.send(Command::InsertNext(items(&["short_a.flac"])));
    assert_eq!(last_queue(&rx).items.len(), 3);
    h.clear_recorded();
    let events = run(&h, &rx, usize::MAX);
    assert_eq!(h.recorded().len() / 8, (GAP_A - 44_100) + SHORT + GAP_B, "rest of gap_a, the inserted track, then gap_b; nothing stale");
    assert_eq!(upcoming(&events, "gap_a"), ["short_a", "gap_b"]);
}

#[test]
fn removing_prefed_next_resyncs() {
    let (p, h, rx) = setup();
    into_prefeed(&p, &h, &["gap_a.flac", "gap_b.flac", "short_b.flac"]);
    let gap_b_id = p.state().queue_item_id.unwrap() + 1;
    p.send(Command::Remove(vec![gap_b_id]));
    assert_eq!(last_queue(&rx).items.len(), 2);
    h.clear_recorded();
    let events = run(&h, &rx, usize::MAX);
    assert_eq!(h.recorded().len() / 8, (GAP_A - 44_100) + SHORT);
    assert_eq!(upcoming(&events, "gap_a"), ["short_b"]);
}

#[test]
fn moving_upcoming_tracks_drops_the_preopened_decoder() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac", "short_a.flac", "short_b.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Move { from: 2, to: 1 });
    let events = run(&h, &rx, usize::MAX);
    assert_eq!(track_names(&events), ["gap_a", "short_b", "short_a"]);
    assert_eq!(h.recorded().len() / 8, GAP_A + 2 * SHORT);
}

#[test]
fn removing_the_playing_track_starts_the_next() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac", "short_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    let id = p.state().queue_item_id.unwrap();
    p.send(Command::Remove(vec![id]));
    wait_for(&p, |s| s.track.as_ref().is_some_and(|t| t.path.ends_with("short_a.flac")) && s.status == PlaybackStatus::Playing);
    h.clear_recorded();
    run(&h, &rx, usize::MAX);
    assert_eq!(h.recorded().len() / 8, SHORT);
}

#[test]
fn repeat_one_replays_and_next_still_moves_on() {
    let (p, h, rx) = setup();
    p.send(Command::SetRepeat(RepeatMode::One));
    play(&p, &["short_a.flac", "short_b.flac"]);
    let events = run(&h, &rx, 3 * SHORT + 1000);
    assert!(events.iter().all(|e| !matches!(e, PlayerEvent::State(s) if s.queue_index == Some(1))), "stayed on the first track");
    assert_eq!(p.state().repeat, RepeatMode::One);
    p.send(Command::Next);
    wait_for(&p, |s| s.queue_index == Some(1));
}

#[test]
fn repeat_all_wraps_without_ending() {
    let (p, h, rx) = setup();
    p.send(Command::SetRepeat(RepeatMode::All));
    play(&p, &["short_a.flac", "short_b.flac"]);
    let events = run(&h, &rx, 4 * SHORT + 2000);
    assert!(!events.iter().any(|e| matches!(e, PlayerEvent::Ended)));
    let mut seen = vec![];
    for e in &events { if let PlayerEvent::State(s) = e && s.status == PlaybackStatus::Playing && seen.last() != Some(&s.queue_index) { seen.push(s.queue_index); } }
    assert!(seen.windows(4).any(|w| w == [Some(0), Some(1), Some(0), Some(1)]), "{seen:?}");
}

#[test]
fn shuffle_keeps_the_playing_entry_first_and_restores_order() {
    let (p, _h, rx) = setup();
    play(&p, &["short_a.flac", "short_b.flac", "mono16_44k.flac", "flac16_44k.flac", "gap_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    let before = last_queue(&rx);
    let playing = p.state().queue_item_id.unwrap();
    p.send(Command::SetShuffle(true));
    let shuffled = last_queue(&rx);
    assert_eq!(shuffled.items[0].id, playing);
    assert_eq!(shuffled.current, Some(0));
    let mut a: Vec<u32> = before.items.iter().map(|i| i.id).collect();
    let mut b: Vec<u32> = shuffled.items.iter().map(|i| i.id).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    assert!(p.state().shuffle);
    p.send(Command::SetShuffle(false));
    assert_eq!(last_queue(&rx).items.iter().map(|i| i.id).collect::<Vec<_>>(), before.items.iter().map(|i| i.id).collect::<Vec<_>>());
}

#[test]
fn played_fires_once_after_half_the_track() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac"]); // 2.51 s → threshold 1.25 s
    pull_frames(&h, 44_100);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!rx.try_iter().any(|e| matches!(e, PlayerEvent::Played { .. })));
    let events = run(&h, &rx, usize::MAX);
    let played: Vec<_> = events.iter().filter(|e| matches!(e, PlayerEvent::Played { .. })).collect();
    assert_eq!(played.len(), 1);
    assert!(matches!(played[0], PlayerEvent::Played { path, .. } if path.ends_with("gap_a.flac")));
}

#[test]
fn seeking_forward_is_not_listening() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Seek(2_000));
    wait_for(&p, |s| s.position_ms >= 2_000);
    let events = run(&h, &rx, usize::MAX);
    assert!(!events.iter().any(|e| matches!(e, PlayerEvent::Played { .. })), "only ~0.5 s was heard");
}

#[test]
fn restore_starts_paused_at_the_saved_position() {
    let (p, h, _rx) = setup();
    p.send(Command::Restore(Box::new(Restore {
        items: vec![NewEntry { duration_ms: Some(2_507), ..NewEntry::from_path(fixture("gap_a.flac")) }, NewEntry::from_path(fixture("gap_b.flac"))],
        current: 1, position_ms: 1_000, shuffle: false, repeat: RepeatMode::All, volume: 0.5, mode: OutputMode::Processed,
    })));
    wait_for(&p, |s| s.status == PlaybackStatus::Paused);
    let s = p.state();
    assert_eq!((s.queue_index, s.position_ms, s.repeat, s.volume), (Some(1), 1_000, RepeatMode::All, 0.5));
    assert!(h.opened().is_empty(), "no stream until Play");
    p.send(Command::Seek(1_500));
    wait_for(&p, |s| s.position_ms == 1_500);
    p.send(Command::Play(None));
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    pull_frames(&h, 44_100);
    wait_for(&p, |s| s.position_ms >= 2_400 && s.track.as_ref().is_some_and(|t| t.path.ends_with("gap_b.flac")));
}

#[test]
fn adding_to_a_draining_queue_keeps_playing() {
    let (p, h, rx) = setup();
    play(&p, &["short_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    std::thread::sleep(Duration::from_millis(50)); // short_a is fully decoded; the player is draining to the end
    p.send(Command::Add(items(&["short_b.flac"])));
    let events = run(&h, &rx, usize::MAX);
    assert_eq!(h.recorded().len() / 8, 2 * SHORT);
    assert_eq!(track_names(&events), ["short_a", "short_b"]);
}

#[test]
fn clear_stops_and_empties() {
    let (p, _h, rx) = setup();
    play(&p, &["gap_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Clear);
    wait_for(&p, |s| s.status == PlaybackStatus::Stopped && s.track.is_none());
    assert!(last_queue(&rx).items.is_empty());
}

#[test]
fn played_threshold_rule() {
    use engine_core::player::played_threshold_ms;
    assert_eq!(played_threshold_ms(Some(200_000)), 100_000);
    assert_eq!(played_threshold_ms(Some(1_200_000)), 240_000);
    assert_eq!(played_threshold_ms(None), 240_000);
    assert_eq!(played_threshold_ms(Some(0)), 1);
}
```

Update the M1 helper in `engine/crates/engine-core/tests/player.rs`:

```rust
fn queue(p: &PlayerHandle, names: &[&str]) {
    p.send(Command::SetQueue { items: names.iter().map(|n| engine_core::player::NewEntry::from_path(fixture(n))).collect(), start: 0 });
    p.send(Command::Play(None));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core --test queue_player`
Expected: compile errors (`NewEntry`, `Restore`, `Command::InsertNext`, … not found).

- [ ] **Step 3: Replace `engine/crates/engine-core/src/player/mod.rs`**

The M1 logic (gapless feeding, reopen, exclusive fallback, device loss, buffer growth) is unchanged. What changes:
- the queue type;
- the new commands and `edit_queue`/`hold`/`rehome`;
- playback-order navigation (`auto_next`/`user_next`) in `start_at`, `advance`, `maybe_prepare_next`, `seek`, Next and Prev;
- listening accounting and `Played`;
- `Restore`;
- the paused-seek path;
- the complete `publish`/`emit_queue`.

```rust
pub mod chain;
pub mod pipeline;
pub mod queue;
pub mod signal_path;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering::*;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use protocol::*;

use crate::decode::{self, DecodeError, TrackDecoder};
use crate::dsp::resample::StreamResampler;
use crate::dsp::volume::volume_gain;
use crate::output::format::OutputFormat;
use crate::output::ring::{ring_pair, StreamShared};
use crate::output::{DeviceCaps, OutputBackend, OutputError, OutputStream, StreamInfo};
use chain::ProcessChain;
use pipeline::{plan_output, Passthrough, PipelinePlan, PlanInput, EXCLUSIVE_UNAVAILABLE};
pub use queue::{Entry, NewEntry, Queue};
use signal_path::build_signal_path;

const MIN_FREE_BYTES: usize = 16 * 1024;
/// "Played" after half the track or this much listening, whichever comes first.
pub const PLAYED_MAX_MS: u64 = 240_000;

pub fn played_threshold_ms(duration_ms: Option<u64>) -> u64 {
    duration_ms.map_or(PLAYED_MAX_MS, |d| (d / 2).min(PLAYED_MAX_MS)).max(1)
}

#[derive(Debug)]
pub enum Command {
    SetQueue { items: Vec<NewEntry>, start: usize },
    Add(Vec<NewEntry>),
    InsertNext(Vec<NewEntry>),
    Move { from: usize, to: usize },
    Remove(Vec<u32>),
    Clear,
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    Restore(Box<Restore>),
    Play(Option<u32>), Pause, Toggle, Stop, Next, Prev,
    Seek(u64), SetVolume(f32), SetMode(OutputMode), SetDevice(Option<String>), Shutdown,
}

/// A saved session (spec §5.2). The player restores it paused; the first Play opens the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Restore {
    pub items: Vec<NewEntry>,
    pub current: usize,
    pub position_ms: u64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub volume: f32,
    pub mode: OutputMode,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    State(PlayerState), SignalPath(SignalPath), Queue(QueueChanged), Error(EngineErrorEvent), DevicesChanged(Vec<DeviceInfo>), Ended,
    /// The audible track crossed `played_threshold_ms` of actual listening (once per play).
    Played { track_id: Option<i64>, path: PathBuf },
}

#[derive(Clone, Debug)]
pub struct PlayerConfig { pub buffer_secs: f64, pub preroll_secs: f64 }
impl Default for PlayerConfig { fn default() -> Self { Self { buffer_secs: 2.0, preroll_secs: 10.0 } } }

pub struct PlayerHandle { tx: Sender<Command>, state: Arc<Mutex<PlayerState>>, join: Option<JoinHandle<()>> }

/// A cloneable command sender (for OS media keys and other non-RPC sources).
#[derive(Clone)]
pub struct Commander(Sender<Command>);
impl Commander { pub fn send(&self, c: Command) { let _ = self.0.send(c); } }

impl PlayerHandle {
    pub fn send(&self, c: Command) { let _ = self.tx.send(c); }
    pub fn state(&self) -> PlayerState { self.state.lock().unwrap().clone() }
    pub fn commander(&self) -> Commander { Commander(self.tx.clone()) }
    pub fn shutdown(mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(j) = self.join.take() { let _ = j.join(); }
    }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) { let _ = self.tx.send(Command::Shutdown); }
}

pub fn spawn_player(backend: Arc<dyn OutputBackend>, cfg: PlayerConfig, events: Sender<PlayerEvent>) -> PlayerHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let state = Arc::new(Mutex::new(PlayerState { volume: 1.0, ..Default::default() }));
    let st = state.clone();
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
    let join = std::thread::Builder::new()
        .name("aurora-player".into())
        .spawn(move || Player::new(backend, cfg, events, st, seed).run(rx))
        .expect("spawn player thread");
    PlayerHandle { tx, state, join: Some(join) }
}

/// The open output stream and the producer side of its ring.
struct Active {
    /// Held only to keep the device open; declared first so it closes before the producer drops.
    _stream: Box<dyn OutputStream>,
    prod: rtrb::Producer<u8>,
    shared: Arc<StreamShared>,
    info: StreamInfo,
    /// Bluetooth codec details, queried once per opened stream (the probe may spawn `pw-dump`).
    bluetooth: Option<BluetoothInfo>,
    /// Exclusive caps this stream was planned with. A real exclusive backend cannot re-probe a
    /// device our own stream holds (EBUSY), so later plans reuse these.
    excl_caps: Option<DeviceCaps>,
    /// Bytes committed to the ring (whole frames only).
    written_bytes: u64,
    /// Processed bytes waiting for ring space.
    carry: Vec<u8>,
    carry_pos: usize,
}

impl Active {
    fn bpf(&self) -> u64 { self.info.format.bytes_per_frame() as u64 }
    fn written_frames(&self) -> u64 { self.written_bytes / self.bpf() }
    fn exclusive(&self) -> bool { self.info.share == ShareMode::Exclusive }
}

/// The track currently being decoded into the ring (may be ahead of what is audible).
struct Feeding { idx: usize, dec: Box<dyn TrackDecoder>, chain: ProcessChain, decoded: u64, eof: bool }

/// Where a track starts in the ring's frame timeline; the front marker is the audible track.
struct Marker {
    start: u64, idx: usize, start_ms: u64, out_rate: u32, signal: SignalPath, duration_ms: Option<u64>,
    /// Listening carried over from before a seek or restart of this same play.
    listened_carry_ms: u64,
    /// `Played` was already sent for this play.
    reported: bool,
}

/// A track waiting for the ring to drain before the stream is reopened in its format.
/// `prev` is the format of the stream being replaced (for the gap note).
struct Reopen { idx: usize, dec: Box<dyn TrackDecoder>, plan: PipelinePlan, prev: Option<OutputFormat> }

/// Queue positions the player holds, captured by entry id before an edit.
struct Held { cur: Option<u32>, feeding: Option<u32>, prepared: Option<u32>, prepare_failed: Option<u32>, reopen: Option<u32>, shown: Option<u32>, markers: Vec<Option<u32>> }

struct Player {
    backend: Arc<dyn OutputBackend>,
    cfg: PlayerConfig,
    events: Sender<PlayerEvent>,
    shared_state: Arc<Mutex<PlayerState>>,
    queue: Queue,
    cur: usize,
    status: PlaybackStatus,
    volume: f32,
    mode: OutputMode,
    device: Option<String>,
    active: Option<Active>,
    feeding: Option<Feeding>,
    prepared: Option<(usize, Box<dyn TrackDecoder>)>,
    /// Queue index whose pre-open failed, so it isn't retried on every block (advance reports the error).
    prepare_failed: Option<usize>,
    markers: VecDeque<Marker>,
    reopen: Option<Reopen>,
    /// A freshly opened stream waits for its first data before `playing` is set, so the callback
    /// never sees an empty ring that counts as an underrun.
    pending_start: bool,
    /// Why exclusive access was denied on this device. While set, every plan is shared, so gapless
    /// advances and same-stream seeks keep the shared stream. It is cleared on every user-initiated
    /// start (`start_at`: Play from Stopped, play item, Next/Prev, SetMode, SetDevice) and on device
    /// loss, so a transient hold (e.g. PipeWire) is re-probed.
    exclusive_denied: Option<String>,
    /// Exclusive caps used by the most recent plan; handed to `Active` when a stream opens for it.
    last_excl_caps: Option<DeviceCaps>,
    end_pending: bool,
    shown_idx: Option<usize>,
    resume_ms: Option<u64>,
    last_tick: Instant,
    last_underruns: (u64, Instant),
    last_devices: (Vec<DeviceInfo>, Instant),
    buffer_secs: f64,
    underrun_times: VecDeque<Instant>,
}

/// Ring size after `recent` underruns within the last 10 s (spec §6: +1 s per burst, max 8 s).
pub fn grown_buffer(current: f64, recent: usize) -> f64 {
    if recent >= 3 { (current + 1.0).min(8.0) } else { current }
}

impl Player {
    fn new(backend: Arc<dyn OutputBackend>, cfg: PlayerConfig, events: Sender<PlayerEvent>, shared_state: Arc<Mutex<PlayerState>>, seed: u64) -> Self {
        let devices = backend.devices().unwrap_or_default();
        Self {
            backend, events, shared_state, queue: Queue::new(seed), cur: 0, status: PlaybackStatus::Stopped,
            volume: 1.0, mode: OutputMode::Processed, device: None, active: None, feeding: None, prepared: None,
            prepare_failed: None, markers: VecDeque::new(), reopen: None, pending_start: false, exclusive_denied: None, last_excl_caps: None, end_pending: false, shown_idx: None, resume_ms: None,
            last_tick: Instant::now(), last_underruns: (0, Instant::now()), last_devices: (devices, Instant::now()),
            buffer_secs: cfg.buffer_secs, underrun_times: VecDeque::new(), cfg,
        }
    }

    fn run(mut self, rx: Receiver<Command>) {
        loop {
            let timeout = Duration::from_millis(if self.status == PlaybackStatus::Playing { 2 } else { 50 });
            match rx.recv_timeout(timeout) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self.status == PlaybackStatus::Playing { self.pump(); }
            self.update_position();
            self.check_device();
        }
        self.active = None;
    }

    // ---------- commands ----------

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::SetQueue { items, start } => {
                self.stop_internal();
                self.cur = self.queue.replace(items, start);
                self.emit_queue();
                self.publish(true);
            }
            Command::Add(items) => self.edit_queue(|q, _| { q.add(items); }),
            Command::InsertNext(items) => self.edit_queue(|q, cur| { q.insert_after(cur, items); }),
            Command::Move { from, to } => self.edit_queue(|q, _| { q.move_item(from, to); }),
            Command::Remove(ids) => self.edit_queue(|q, _| { q.remove(&ids); }),
            Command::SetShuffle(on) => self.edit_queue(|q, cur| q.set_shuffle(on, cur)),
            Command::SetRepeat(r) => self.edit_queue(|q, _| q.set_repeat(r)),
            Command::Clear => {
                self.stop_internal();
                self.queue.clear();
                self.cur = 0;
                self.emit_queue();
                self.publish(true);
            }
            Command::Restore(r) => {
                let r = *r;
                self.stop_internal();
                self.queue.restore(r.items, r.shuffle, r.repeat);
                self.volume = r.volume.clamp(0.0, 1.0);
                self.mode = r.mode;
                self.cur = 0;
                if !self.queue.is_empty() {
                    self.cur = r.current.min(self.queue.len() - 1);
                    self.status = PlaybackStatus::Paused;
                    self.resume_ms = Some(r.position_ms);
                }
                self.emit_queue();
                self.publish(true);
            }
            Command::Play(Some(id)) => if let Some(i) = self.queue.index_of(id) { self.start_at(i, 0) },
            Command::Play(None) => match self.status {
                PlaybackStatus::Paused => self.resume(),
                PlaybackStatus::Stopped if !self.queue.is_empty() => self.start_at(self.cur.min(self.queue.len() - 1), 0),
                _ => {}
            },
            Command::Pause => self.pause(),
            Command::Toggle => if self.status == PlaybackStatus::Playing { self.pause() } else { self.handle(Command::Play(None)) },
            Command::Stop => { self.stop_internal(); self.publish(true); }
            Command::Next => match self.queue.user_next(self.cur) {
                Some(n) => self.start_at(n, 0),
                None => { self.stop_internal(); self.publish(true) }
            },
            Command::Prev => {
                if self.queue.is_empty() { return; }
                let target = if self.position_ms() > 3000 { Some(self.cur) } else { self.queue.user_prev(self.cur) };
                if let Some(t) = target { self.start_at(t, 0) }
            }
            Command::Seek(ms) => self.seek(ms),
            Command::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                if let Some(a) = &self.active { a.shared.set_gain(volume_gain(self.volume) as f32); }
                let software_gain = self.software_gain();
                if let Some(m) = self.markers.front_mut() {
                    if let Some(vol) = m.signal.stages.iter_mut().find(|s| software_gain && s.kind == StageKind::Volume && s.detail.ends_with('%')) {
                        vol.detail = format!("{}%", (self.volume * 100.0).round() as u32);
                        vol.active = self.volume < 1.0;
                    }
                    let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
                }
                self.publish(true);
            }
            Command::SetMode(m) => { self.mode = m; self.exclusive_denied = None; self.restart_if_active(); self.publish(true); }
            Command::SetDevice(d) => { self.device = d; self.exclusive_denied = None; self.restart_if_active(); self.publish(true); }
            Command::Shutdown => {}
        }
    }

    /// Runs a queue edit, then re-resolves every queue position the player holds by entry id.
    /// If the audible entry was removed, the entry now at its position plays (paused/stopped
    /// players just move there). If audio already decoded for what follows the audible track no
    /// longer matches the queue's playback order, the ring is resynced with a same-position seek.
    fn edit_queue(&mut self, edit: impl FnOnce(&mut Queue, Option<usize>)) {
        let audible = self.markers.front().map_or(self.cur, |m| m.idx);
        let audible_id = self.queue.get(audible).map(|e| e.id);
        let held = self.hold();
        edit(&mut self.queue, audible_id.map(|_| audible));
        let audible_now = audible_id.and_then(|id| self.queue.index_of(id));
        if self.status != PlaybackStatus::Stopped && audible_now.is_none() {
            let was_playing = self.status == PlaybackStatus::Playing;
            let next = if audible < self.queue.len() {
                Some(audible)
            } else if self.queue.repeat() == RepeatMode::All && !self.queue.is_empty() {
                Some(0)
            } else {
                None
            };
            match next {
                Some(i) if was_playing => self.start_at(i, 0),
                Some(i) => { self.stop_internal(); self.cur = i; }
                None => { self.stop_internal(); self.cur = 0; }
            }
        } else {
            let consistent = self.rehome(held);
            if audible_now.is_none() { self.cur = self.cur.min(self.queue.len().saturating_sub(1)); }
            if !consistent && self.active.is_some() && self.status != PlaybackStatus::Stopped {
                let ms = self.position_ms();
                self.seek(ms);
            }
        }
        self.emit_queue();
        self.publish(true);
    }

    fn hold(&self) -> Held {
        let id = |i: usize| self.queue.get(i).map(|e| e.id);
        Held {
            cur: id(self.cur),
            feeding: self.feeding.as_ref().and_then(|f| id(f.idx)),
            prepared: self.prepared.as_ref().and_then(|(i, _)| id(*i)),
            prepare_failed: self.prepare_failed.and_then(id),
            reopen: self.reopen.as_ref().and_then(|r| id(r.idx)),
            shown: self.shown_idx.and_then(id),
            markers: self.markers.iter().map(|m| id(m.idx)).collect(),
        }
    }

    /// Points held positions at their entries' new indices. Returns false when audio decoded past
    /// the audible track (later markers, or a pending reopen) belongs to a removed entry or no
    /// longer follows the playback order, or when the queue now continues past a track the player
    /// was draining as the last one.
    fn rehome(&mut self, h: Held) -> bool {
        let q = &self.queue;
        let at = |id: Option<u32>| id.and_then(|id| q.index_of(id));
        let mut consistent = true;
        if let Some(i) = at(h.cur) { self.cur = i; }
        self.shown_idx = at(h.shown);
        for (m, id) in self.markers.iter_mut().zip(&h.markers) {
            match at(*id) { Some(i) => m.idx = i, None => consistent = false }
        }
        if let Some(f) = self.feeding.as_mut() {
            match at(h.feeding) { Some(i) => f.idx = i, None => consistent = false }
        }
        if let Some(r) = self.reopen.as_mut() {
            match at(h.reopen) { Some(i) => r.idx = i, None => consistent = false }
        }
        let mut expected = self.markers.front().map(|m| m.idx);
        for idx in self.markers.iter().skip(1).map(|m| m.idx).chain(self.reopen.as_ref().map(|r| r.idx)) {
            expected = expected.and_then(|e| q.auto_next(e));
            if expected != Some(idx) { consistent = false; }
        }
        let last = self.reopen.as_ref().map(|r| r.idx).or(self.markers.back().map(|m| m.idx));
        if self.end_pending && last.and_then(|l| q.auto_next(l)).is_some() { consistent = false; }
        let feed_next = self.feeding.as_ref().and_then(|f| q.auto_next(f.idx));
        match at(h.prepared).filter(|i| Some(*i) == feed_next) {
            Some(i) => if let Some((slot, _)) = self.prepared.as_mut() { *slot = i },
            None => self.prepared = None,
        }
        self.prepare_failed = at(h.prepare_failed).filter(|i| Some(*i) == feed_next);
        consistent
    }

    /// (listened ms, reported) of the audible play, to carry across a seek or restart.
    fn take_listen(&self) -> (u64, bool) {
        match (&self.active, self.markers.front()) {
            (Some(a), Some(m)) => {
                let consumed = a.shared.consumed_frames.load(Acquire);
                (m.listened_carry_ms + consumed.saturating_sub(m.start) * 1000 / m.out_rate.max(1) as u64, m.reported)
            }
            _ => (0, false),
        }
    }

    fn put_listen(&mut self, (carry, reported): (u64, bool)) {
        if let Some(m) = self.markers.front_mut() { m.listened_carry_ms = carry; m.reported = reported; }
    }

    fn restart_if_active(&mut self) {
        if self.status == PlaybackStatus::Stopped { return; }
        let (idx, ms, paused) = (self.cur, self.position_ms(), self.status == PlaybackStatus::Paused);
        let listen = self.take_listen();
        self.start_at(idx, ms);
        self.put_listen(listen);
        if paused { self.pause(); }
    }

    fn pause(&mut self) {
        if self.status != PlaybackStatus::Playing { return; }
        if let Some(a) = &self.active { a.shared.playing.store(false, Release); }
        self.status = PlaybackStatus::Paused;
        self.publish(true);
    }

    fn resume(&mut self) {
        if self.active.is_none() {
            let ms = self.resume_ms.take().unwrap_or(0);
            self.start_at(self.cur, ms);
            return;
        }
        // A stream still waiting for its first data is released by `pump`.
        if !self.pending_start && let Some(a) = &self.active { a.shared.playing.store(true, Release); }
        self.status = PlaybackStatus::Playing;
        self.publish(true);
    }

    fn stop_internal(&mut self) {
        self.active = None;
        self.feeding = None;
        self.prepared = None;
        self.prepare_failed = None;
        self.reopen = None;
        self.pending_start = false;
        self.markers.clear();
        self.end_pending = false;
        self.shown_idx = None;
        self.resume_ms = None;
        self.status = PlaybackStatus::Stopped;
    }

    // ---------- opening ----------

    fn open_decoder(&mut self, idx: usize) -> Result<Box<dyn TrackDecoder>, DecodeError> {
        if self.prepared.as_ref().is_some_and(|(i, _)| *i == idx) { return Ok(self.prepared.take().unwrap().1); }
        decode::open(&self.queue.entries()[idx].path)
    }

    /// Software volume applies only in processed mode. Bit-perfect mode hides the volume control, so
    /// it means unity gain even when a track falls back to a processed stream.
    fn software_gain(&self) -> bool { self.mode == OutputMode::Processed }

    /// The volume the signal path reports for streams opened now.
    fn effective_volume(&self) -> f32 { if self.software_gain() { self.volume } else { 1.0 } }

    fn device_transport(&self) -> Transport {
        let devices = &self.last_devices.0;
        let found = match &self.device { Some(id) => devices.iter().find(|d| &d.id == id), None => devices.iter().find(|d| d.is_default) };
        found.map_or(Transport::Other, |d| d.transport)
    }

    /// Plans `dec` for the current mode and device. Once exclusive access has been denied, plans
    /// are shared (processed) and carry the denial note, so gapless continuation still matches.
    fn plan_for(&mut self, dec: &mut dyn TrackDecoder, current: Option<(OutputFormat, bool)>) -> PipelinePlan {
        self.plan_with(dec, current, None)
    }

    /// `plan_for`, but `open_failed` (why an exclusive open just failed) forces a shared plan for
    /// this track only, with a note.
    fn plan_with(&mut self, dec: &mut dyn TrackDecoder, current: Option<(OutputFormat, bool)>, open_failed: Option<&str>) -> PipelinePlan {
        let device = self.device.clone();
        let dev = device.as_deref();
        let shared = self.backend.caps(dev, false).unwrap_or_else(|_| DeviceCaps::fallback());
        let mut exclusive = None;
        let mut unavailable = None;
        if self.mode == OutputMode::BitPerfect && self.exclusive_denied.is_none() && open_failed.is_none() {
            // While our own exclusive stream is open the device is busy to a fresh probe, so reuse its caps.
            let held = self.active.as_ref().filter(|a| a.exclusive()).and_then(|a| a.excl_caps.clone());
            match held.map(Ok).unwrap_or_else(|| self.backend.caps(dev, true)) {
                Ok(c) => exclusive = Some(c),
                Err(OutputError::ExclusiveDenied(reason)) => self.exclusive_denied = Some(reason),
                Err(e) => unavailable = Some(e.to_string()),
            }
        }
        self.last_excl_caps = exclusive.clone();
        let force_shared = self.exclusive_denied.is_some() || open_failed.is_some();
        let spec = dec.info().spec;
        let mut plan = plan_output(&PlanInput {
            source: &spec, dsd_rate: dec.info().dsd_rate, mode: if force_shared { OutputMode::Processed } else { self.mode },
            transport: self.device_transport(), shared_caps: &shared, exclusive_caps: exclusive.as_ref(), current,
        });
        if let Some(reason) = &self.exclusive_denied {
            plan.notes.push(format!("Exclusive access denied ({reason}) — using shared mode"));
        } else if let Some(reason) = open_failed {
            plan.notes.push(format!("Exclusive open failed ({reason}) — using shared mode"));
        } else if let Some(reason) = unavailable {
            for n in plan.notes.iter_mut().filter(|n| *n == EXCLUSIVE_UNAVAILABLE) {
                *n = format!("Exclusive mode unavailable ({reason}) — using processed mode");
            }
        }
        if let Some(out) = plan.dsd { dec.set_dsd_output(out); }
        plan
    }

    fn open_stream(&mut self, plan: &PipelinePlan) -> Result<(), OutputError> {
        let fmt = plan.format;
        let (prod, src, shared) = ring_pair((fmt.sample_rate as f64 * self.buffer_secs) as usize, fmt);
        shared.apply_gain.store(self.software_gain() && plan.passthrough == Passthrough::None, Release);
        shared.set_gain(volume_gain(self.volume) as f32);
        let mut stream = self.backend.open(self.device.as_deref(), fmt, plan.exclusive, src)?;
        stream.start()?;
        let info = stream.info().clone();
        let bluetooth = self.backend.bluetooth(&info);
        let excl_caps = if plan.exclusive { self.last_excl_caps.clone() } else { None };
        self.active = Some(Active { _stream: stream, prod, shared, info, bluetooth, excl_caps, written_bytes: 0, carry: vec![], carry_pos: 0 });
        // Underrun reporting compares against the new ring's counter, which starts at 0.
        self.last_underruns.0 = 0;
        Ok(())
    }

    /// Opens a stream for `plan`. If an exclusive open fails, re-plans this track in shared mode
    /// with a note: a denial (`ExclusiveDenied`) is remembered (sticky) and noted as such; any other
    /// error (format rejected, rate not settable, ...) only affects this track. Returns the plan
    /// actually used. The planner's notes for the failed exclusive plan describe that plan, so they
    /// aren't carried; player notes (the reopen gap note) are added by the caller to the returned plan.
    fn open_stream_with_fallback(&mut self, dec: &mut dyn TrackDecoder, plan: PipelinePlan) -> Result<PipelinePlan, OutputError> {
        match self.open_stream(&plan) {
            Ok(()) => Ok(plan),
            Err(e) if plan.exclusive => {
                let shared = match e {
                    OutputError::ExclusiveDenied(reason) => {
                        self.exclusive_denied = Some(reason);
                        self.plan_for(dec, None)
                    }
                    other => self.plan_with(dec, None, Some(&other.to_string())),
                };
                self.open_stream(&shared)?;
                Ok(shared)
            }
            Err(e) => Err(e),
        }
    }

    /// Starts what follows `idx`: in playback order when the track ended by itself (`natural`, so
    /// repeat-one replays it), else the next entry (skipping a broken one). Finishes at the end.
    fn start_after(&mut self, idx: usize, natural: bool) {
        let next = if natural { self.queue.auto_next(idx) } else { self.queue.user_next(idx) };
        match next { Some(n) => self.start_at(n, 0), None => self.finish() }
    }

    /// Opens the decoder at `idx` (skipping broken entries), seeks to `ms`, opens a stream and starts feeding.
    fn start_at(&mut self, idx: usize, ms: u64) {
        let keep_prepared = self.prepared.take();
        self.stop_internal();
        // A user-initiated start re-probes exclusive access (the device may have been released).
        self.exclusive_denied = None;
        self.prepared = keep_prepared;
        let (mut idx, mut ms) = (idx, ms);
        for _ in 0..self.queue.len() {
            if idx >= self.queue.len() { break; }
            let mut dec = match self.open_decoder(idx) {
                Ok(d) => d,
                Err(e) => {
                    self.track_error(idx, &e);
                    match self.queue.user_next(idx) { Some(n) => { idx = n; ms = 0; continue; } None => break }
                }
            };
            if ms > 0 {
                let frame = ms * dec.info().spec.sample_rate as u64 / 1000;
                if dec.info().total_frames.is_some_and(|t| frame >= t) {
                    match self.queue.auto_next(idx) { Some(n) => { idx = n; ms = 0; continue; } None => break }
                }
                if let Err(e) = dec.seek(frame) {
                    self.track_error(idx, &e);
                    match self.queue.user_next(idx) { Some(n) => { idx = n; ms = 0; continue; } None => break }
                }
            }
            let plan = self.plan_for(dec.as_mut(), None);
            let plan = match self.open_stream_with_fallback(dec.as_mut(), plan) { Ok(p) => p, Err(e) => { self.output_error(&e); return; } };
            self.begin_feeding(idx, dec, plan, ms, None);
            self.cur = idx;
            self.status = PlaybackStatus::Playing;
            self.pending_start = true;
            self.publish(true);
            return;
        }
        self.finish();
    }

    fn begin_feeding(&mut self, idx: usize, dec: Box<dyn TrackDecoder>, plan: PipelinePlan, start_ms: u64, reuse: Option<StreamResampler>) {
        let a = self.active.as_ref().expect("stream open before feeding");
        let info = dec.info().clone();
        let signal = build_signal_path(&info, &plan, &a.info, self.effective_volume(), a.bluetooth.clone(), a.shared.underruns.load(Relaxed));
        self.markers.push_back(Marker {
            start: a.written_frames(), idx, start_ms, out_rate: plan.format.sample_rate, signal,
            duration_ms: info.total_frames.map(|f| f * 1000 / info.spec.sample_rate.max(1) as u64),
            listened_carry_ms: 0, reported: false,
        });
        let chain = ProcessChain::new(plan, info.spec, reuse);
        self.feeding = Some(Feeding { idx, dec, chain, decoded: start_ms * info.spec.sample_rate as u64 / 1000, eof: false });
    }

    // ---------- feeding ----------

    fn pump(&mut self) {
        self.pump_blocks();
        self.release_pending_start();
    }

    /// Starts a new stream's callback once data beyond any seek discard point is in its ring. Runs
    /// at the end of a pump pass, so the ring holds a whole pass of prefill, not a single block.
    fn release_pending_start(&mut self) {
        if !self.pending_start || self.status != PlaybackStatus::Playing { return; }
        let Some(a) = &self.active else { return };
        if a.written_frames() > a.shared.discard_to.load(Acquire) {
            a.shared.playing.store(true, Release);
            self.pending_start = false;
        }
    }

    fn pump_blocks(&mut self) {
        for _ in 0..64 {
            if !self.flush_carry() { return; }
            if self.reopen.is_some() {
                if !self.try_reopen() { return; }
                continue;
            }
            let Some(feed) = self.feeding.as_mut() else { self.check_drained(); return; };
            if feed.eof { self.advance(); continue; }
            if self.active.as_ref().map_or(0, |a| a.prod.slots()) < MIN_FREE_BYTES { return; }
            let feed = self.feeding.as_mut().unwrap();
            match feed.dec.next_block() {
                Ok(Some(block)) => {
                    feed.decoded += block.frames() as u64;
                    let mut bytes = Vec::with_capacity(block.frames() * 16);
                    feed.chain.process(&block, &mut bytes);
                    self.push_bytes(bytes);
                    self.maybe_prepare_next();
                }
                Ok(None) => feed.eof = true,
                Err(e) => { let idx = feed.idx; feed.eof = true; self.track_error(idx, &e); }
            }
        }
    }

    fn push_bytes(&mut self, bytes: Vec<u8>) {
        if let Some(a) = self.active.as_mut() {
            if a.carry_pos < a.carry.len() { a.carry.drain(..a.carry_pos); a.carry.extend(bytes); } else { a.carry = bytes; }
            a.carry_pos = 0;
        }
    }

    /// Writes pending bytes into the ring, whole frames only, so `written_frames()` is exact and a
    /// seek's `discard_to` never splits a frame. Returns true when nothing is left pending.
    fn flush_carry(&mut self) -> bool {
        let Some(a) = self.active.as_mut() else { return false };
        let remaining = a.carry.len() - a.carry_pos;
        if remaining == 0 { return true; }
        let bpf = a.bpf() as usize;
        let n = a.prod.slots().min(remaining) / bpf * bpf;
        if n > 0 {
            let mut chunk = a.prod.write_chunk(n).expect("slots checked");
            let (x, y) = chunk.as_mut_slices();
            let src = &a.carry[a.carry_pos..a.carry_pos + n];
            x.copy_from_slice(&src[..x.len()]);
            y.copy_from_slice(&src[x.len()..]);
            chunk.commit_all();
            a.carry_pos += n;
            a.written_bytes += n as u64;
        }
        a.carry_pos == a.carry.len()
    }

    fn maybe_prepare_next(&mut self) {
        let Some(feed) = &self.feeding else { return };
        let Some(next) = self.queue.auto_next(feed.idx) else { return };
        if self.prepared.as_ref().is_some_and(|(i, _)| *i == next) || self.prepare_failed == Some(next) { return; }
        let info = feed.dec.info();
        let Some(total) = info.total_frames else { return };
        let remaining_secs = total.saturating_sub(feed.decoded) as f64 / info.spec.sample_rate.max(1) as f64;
        if remaining_secs <= self.cfg.preroll_secs {
            match decode::open(&self.queue.entries()[next].path) {
                Ok(dec) => self.prepared = Some((next, dec)),
                Err(_) => self.prepare_failed = Some(next),
            }
        }
    }

    /// The feeding decoder hit EOF: continue gaplessly with what plays next, schedule a reopen, or end.
    fn advance(&mut self) {
        let mut feed = self.feeding.take().expect("advance with feeding");
        let mut next = self.queue.auto_next(feed.idx);
        let mut attempts = self.queue.len();
        loop {
            let Some(idx) = next.filter(|_| attempts > 0) else {
                let mut tail = vec![];
                feed.chain.flush(&mut tail);
                self.push_bytes(tail);
                self.end_pending = true;
                if let Some(a) = &self.active { a.shared.draining.store(true, Release); }
                return;
            };
            attempts -= 1;
            let mut dec = match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); next = self.queue.user_next(idx); continue; } };
            let current = self.active.as_ref().map(|a| (a.info.format, a.exclusive()));
            let plan = self.plan_for(dec.as_mut(), current);
            let same = current.is_some_and(|(f, ex)| f == plan.format && ex == plan.exclusive);
            if same {
                // Gapless: hand the resampler (and its buffered latency) on; never flush here.
                let reuse = feed.chain.take_resampler();
                self.begin_feeding(idx, dec, plan, 0, reuse);
            } else {
                let mut tail = vec![];
                feed.chain.flush(&mut tail);
                self.push_bytes(tail);
                if let Some(a) = &self.active { a.shared.draining.store(true, Release); }
                self.reopen = Some(Reopen { idx, dec, plan, prev: current.map(|(f, _)| f) });
            }
            return;
        }
    }

    fn drained(&self) -> bool {
        self.active.as_ref().is_none_or(|a| a.carry_pos == a.carry.len() && a.shared.consumed_frames.load(Acquire) >= a.written_frames())
    }

    /// Once the old ring has drained, reopens the stream for the pending track. Returns true when
    /// the new stream is open and feeding can continue.
    fn try_reopen(&mut self) -> bool {
        if !self.drained() { return false; }
        self.update_position();
        let Reopen { idx, mut dec, plan, prev } = self.reopen.take().unwrap();
        self.active = None;
        self.markers.clear();
        let mut plan = match self.open_stream_with_fallback(dec.as_mut(), plan) { Ok(p) => p, Err(e) => { self.output_error(&e); return false; } };
        let rate_changed = prev.is_some_and(|f| f.sample_rate != plan.format.sample_rate);
        plan.notes.push(if rate_changed { "Rate change: brief gap" } else { "Format change: brief gap" }.into());
        self.begin_feeding(idx, dec, plan, 0, None);
        self.pending_start = true;
        true
    }

    fn check_drained(&mut self) {
        if self.end_pending && self.drained() {
            self.update_position();
            self.finish();
        }
    }

    fn finish(&mut self) {
        self.stop_internal();
        self.cur = 0;
        let _ = self.events.send(PlayerEvent::Ended);
        self.publish(true);
    }

    // ---------- seek ----------

    fn seek(&mut self, ms: u64) {
        if self.status == PlaybackStatus::Stopped { return; }
        if self.active.is_none() {
            // Paused without a stream (restored session, device loss): move the resume point.
            if self.status == PlaybackStatus::Paused { self.resume_ms = Some(ms); self.publish(true); }
            return;
        }
        let idx = self.markers.front().map_or(self.cur, |m| m.idx);
        let listen = self.take_listen();
        {
            // Everything already in the ring is stale: drop it before any new bytes are written.
            let a = self.active.as_mut().unwrap();
            a.carry.clear();
            a.carry_pos = 0;
            a.shared.discard_to.store(a.written_frames(), Release);
            a.shared.draining.store(false, Release);
        }
        self.reopen = None;
        self.end_pending = false;
        self.markers.clear();
        self.shown_idx = None;
        let mut dec = match self.feeding.take() {
            Some(f) if f.idx == idx => f.dec,
            _ => match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); return self.start_after(idx, false); } },
        };
        let frame = ms * dec.info().spec.sample_rate as u64 / 1000;
        if dec.info().total_frames.is_some_and(|t| frame >= t) { return self.start_after(idx, true); }
        if let Err(e) = dec.seek(frame) { self.track_error(idx, &e); return self.start_after(idx, false); }
        let current = self.active.as_ref().map(|a| (a.info.format, a.exclusive()));
        let plan = self.plan_for(dec.as_mut(), current);
        if current.is_some_and(|(f, ex)| f != plan.format || ex != plan.exclusive) {
            self.start_at(idx, ms);
            self.put_listen(listen);
            return;
        }
        self.begin_feeding(idx, dec, plan, ms, None);
        self.put_listen(listen);
        self.cur = idx;
        self.publish(true);
    }

    // ---------- position, events ----------

    fn position_ms(&self) -> u64 {
        if self.status == PlaybackStatus::Stopped { return 0; }
        let (Some(a), Some(m)) = (&self.active, self.markers.front()) else { return self.resume_ms.unwrap_or(0) };
        let consumed = a.shared.consumed_frames.load(Acquire);
        m.start_ms + consumed.saturating_sub(m.start) * 1000 / m.out_rate.max(1) as u64
    }

    fn update_position(&mut self) {
        let Some(a) = &self.active else { return };
        let consumed = a.shared.consumed_frames.load(Acquire);
        let underruns = a.shared.underruns.load(Relaxed);
        let discard_to = a.shared.discard_to.load(Acquire);
        while self.markers.len() > 1 && self.markers[1].start <= consumed { self.markers.pop_front(); }
        let mut changed = false;
        if let Some(m) = self.markers.front_mut() {
            if self.shown_idx != Some(m.idx) && m.start <= consumed.max(discard_to) {
                self.shown_idx = Some(m.idx);
                self.cur = m.idx;
                m.signal.underruns = underruns;
                let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
                changed = true;
            } else if underruns != self.last_underruns.0 && self.last_underruns.1.elapsed() >= Duration::from_secs(1) {
                let fresh = underruns.saturating_sub(self.last_underruns.0).min(16);
                let now = Instant::now();
                self.underrun_times.extend(std::iter::repeat_n(now, fresh as usize));
                while self.underrun_times.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(10)) { self.underrun_times.pop_front(); }
                let grown = grown_buffer(self.buffer_secs, self.underrun_times.len());
                if grown > self.buffer_secs {
                    self.buffer_secs = grown;
                    self.underrun_times.clear();
                    m.signal.notes.push(format!("Buffer increased to {grown:.0} s after repeated underruns"));
                }
                m.signal.underruns = underruns;
                self.last_underruns = (underruns, now);
                let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
            }
            if self.status == PlaybackStatus::Playing && !m.reported && m.start <= consumed {
                let listened = m.listened_carry_ms + (consumed - m.start) * 1000 / m.out_rate.max(1) as u64;
                if listened >= played_threshold_ms(m.duration_ms) {
                    m.reported = true;
                    if let Some(e) = self.queue.get(m.idx) {
                        let _ = self.events.send(PlayerEvent::Played { track_id: e.track_id, path: e.path.clone() });
                    }
                }
            }
        }
        if changed || (self.status == PlaybackStatus::Playing && self.last_tick.elapsed() >= Duration::from_millis(250)) {
            self.publish(true);
        }
    }

    fn publish(&mut self, emit: bool) {
        self.last_tick = Instant::now();
        let entry = self.queue.get(self.cur);
        let duration_ms = self.markers.front().filter(|m| m.idx == self.cur).and_then(|m| m.duration_ms).or_else(|| entry.and_then(|e| e.duration_ms));
        let state = PlayerState {
            status: self.status,
            queue_index: entry.map(|_| self.cur as u32),
            queue_item_id: entry.map(|e| e.id),
            track: entry.map(|e| TrackSummary {
                path: e.path.to_string_lossy().into_owned(), title: e.title.clone(), duration_ms,
                track_id: e.track_id, artist: e.artist.clone(), album: e.album.clone(), artwork_id: e.artwork_id,
            }),
            position_ms: self.position_ms(),
            duration_ms,
            volume: self.volume,
            mode: self.mode,
            device_id: self.device.clone(),
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
        };
        *self.shared_state.lock().unwrap() = state.clone();
        if emit { let _ = self.events.send(PlayerEvent::State(state)); }
    }

    fn emit_queue(&self) {
        let current = (!self.queue.is_empty()).then_some(self.cur as u32);
        let _ = self.events.send(PlayerEvent::Queue(QueueChanged { items: self.queue.items(), current }));
    }

    fn track_error(&self, idx: usize, e: &DecodeError) {
        let code = e.code();
        let _ = self.events.send(PlayerEvent::Error(EngineErrorEvent {
            code: code.code(), name: code.name().into(), message: e.to_string(),
            path: self.queue.get(idx).map(|q| q.path.to_string_lossy().into_owned()),
        }));
    }

    fn output_error(&mut self, e: &OutputError) {
        let code = e.code();
        let _ = self.events.send(PlayerEvent::Error(EngineErrorEvent { code: code.code(), name: code.name().into(), message: e.to_string(), path: None }));
        self.stop_internal();
        self.publish(true);
    }

    // ---------- devices ----------

    fn check_device(&mut self) {
        let lost = self.active.as_ref().is_some_and(|a| a.shared.device_lost.load(Acquire));
        if lost {
            let ms = self.position_ms();
            let idx = self.cur;
            self.stop_internal();
            self.cur = idx;
            self.resume_ms = Some(ms);
            self.device = None;
            self.exclusive_denied = None;
            self.status = PlaybackStatus::Paused;
            let code = ErrorCode::DeviceUnavailable;
            let _ = self.events.send(PlayerEvent::Error(EngineErrorEvent { code: code.code(), name: code.name().into(), message: "Output device disconnected; switched to the system default".into(), path: None }));
            self.refresh_devices(true);
            self.publish(true);
            return;
        }
        if self.last_devices.1.elapsed() >= Duration::from_secs(2) { self.refresh_devices(false); }
    }

    fn refresh_devices(&mut self, force_emit: bool) {
        let list = self.backend.devices().unwrap_or_default();
        let changed = list != self.last_devices.0;
        self.last_devices = (list.clone(), Instant::now());
        if changed || force_emit { let _ = self.events.send(PlayerEvent::DevicesChanged(list)); }
    }
}

#[cfg(test)]
mod tests {
    use super::grown_buffer;
    #[test]
    fn buffer_grows_on_bursts_and_caps_at_eight() {
        assert_eq!(grown_buffer(2.0, 2), 2.0);
        assert_eq!(grown_buffer(2.0, 3), 3.0);
        assert_eq!(grown_buffer(7.5, 5), 8.0);
        assert_eq!(grown_buffer(8.0, 9), 8.0);
    }
}
```

`rehome` borrows `self.queue` immutably (`let q = &self.queue`) while it mutates `self.markers`, `self.feeding`, `self.reopen` and `self.prepared`. Those are disjoint fields, which the borrow checker accepts inside one method body. Don't move this logic into helper methods that take `&mut self`.

- [ ] **Step 4: Fix the other `SetQueue` call sites and the event arm**

In `engine/crates/aurora-engine/src/main.rs` `play()`:

```rust
    player.send(Command::SetQueue { items: files.into_iter().map(engine_core::player::NewEntry::from_path).collect(), start: 0 });
```

In `engine/crates/aurora-engine/src/rpc/dispatch.rs` `queue.setPaths`:

```rust
            ok(Command::SetQueue { items: p.paths.into_iter().map(engine_core::player::NewEntry::from_path).collect(), start: p.start_index as usize })
```

Remove the now-unused `use std::path::PathBuf;` from `dispatch.rs` if the compiler flags it.

In `engine/crates/aurora-engine/src/rpc/mod.rs`, add `PlayerEvent::Played { .. } => {}` to the event `match`. Task 13 records history here.

- [ ] **Step 5: Run the whole engine suite**

Run: `cargo test --manifest-path engine/Cargo.toml --workspace`
Expected:
- the 13 `queue_player` tests pass;
- every M1 player, gapless, seek and rpc test still passes;
- the queue model's unit tests pass.

Then run: `cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings`
Expected: clean.

If `moving_upcoming_tracks_drops_the_preopened_decoder` reports `["gap_a", "short_a", "short_b"]`, the prepared decoder wasn't dropped. Check the prepared-decoder `match` near the end of `rehome`.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/engine-core engine/crates/aurora-engine
git commit -m "feat(player): queue editing with id remapping, prefeed resync, shuffle/repeat, played events and restore"
```

### Task 13: Engine RPC — `--data-dir`, library service, `library.*` / `playlists.*` / `queue.*` methods

**Files:**
- Modify: `engine/crates/aurora-engine/Cargo.toml` (deps `library`; dev-dep `tempfile`), `src/main.rs` (`rpc --data-dir`), `src/rpc/mod.rs`, `src/rpc/dispatch.rs`
- Create: `src/rpc/library_service.rs`, `src/rpc/library_methods.rs`, `engine/crates/aurora-engine/tests/common/mod.rs`, `engine/crates/aurora-engine/tests/library_rpc.rs`
- Modify: `engine/crates/aurora-engine/tests/rpc.rs` (use the shared harness)
- Modify (library crate): `engine/crates/library/src/artwork.rs` (`Library::artwork_info`), `engine/crates/library/tests/artwork.rs` (its test)

**Interfaces:**
- Consumes:
  - From Tasks 2–9: every `Library` method, plus `library::{scan, watch}`.
  - From Task 12: `Command::{SetQueue, Add, InsertNext, Move, Remove, Clear, SetShuffle, SetRepeat}`, `PlayerEvent::Played` and `NewEntry`.
  - From Task 1: the protocol params.
- Produces:
  - **CLI:** `aurora-engine rpc [--data-dir <DIR>]`.
  - **`rpc::RunOptions { data_dir: Option<PathBuf> }`** (Default) and `rpc::run(RunOptions) -> i32`.
  - **Methods on the wire:**
    - **Library, data:** `library.folders`, `library.albums|artists|genres|tracks(ListQuery)`, `library.album|artist|genre|track({id})`, `library.search({text, limit?})` (default 20), `library.artwork({id, size}) → ArtworkInfo`, `library.recent({kind, limit?})` (default 20).
      - `library.track` fills `gapless` by probing the file.
    - **Library, folders:** `library.addFolder({path}) → Folder`, `library.removeFolder({id})`, `library.rescan({folderId?})`.
    - **Playlists:** `playlists.list`, `playlists.get({id})`, `playlists.create({name})`, `playlists.rename({id,name})`, `playlists.delete({id})`, `playlists.addTracks({id, trackIds, index?})`, `playlists.removeItems({id, itemIds})`, `playlists.reorder({id, from, to})`, `playlists.importM3u({path}) → ImportResult`, `playlists.exportM3u({id, path})`.
    - **Queue:** `queue.set({trackIds, startIndex?})`, `queue.add({trackIds})`, `queue.insertNext({trackIds})`, `queue.move({from, to})`, `queue.remove({itemIds})`, `queue.clear`, `queue.setShuffle({shuffle})`, `queue.setRepeat({repeat})`.
  - **Events:**
    - `library.scanProgress(ScanProgress)`.
    - `library.changed(LibraryChanged)`:
      - `folders` after add/remove;
      - `tracks` after a scan that changed anything, or after an unplayable mark;
      - `playlists` after any playlist mutation;
      - `history` after a recorded play.
    - `engine.error` also reports a DB recovery (code -32008, sent once, after `engine.ready`) and scan failures.
  - **Errors:** library errors map to `LibError::code()`, with `data: { name }`. Without `--data-dir`, `library.*`, `playlists.*` and `queue.set|add|insertNext` return -32008 with the message `library unavailable: engine started without --data-dir`.
  - **Startup:** with a data dir, every folder is watched and gets one incremental scan.
  - **Shutdown:** stdin EOF or `engine.shutdown` cancels the scan worker before the player stops.
  - **`Library::artwork_info(&self, id: i64, size: u32) -> Result<ArtworkInfo>`:** the path is the nearest cached size. It gives `NotFound` for an unknown id and `FileMissing` when the cache file is gone.
  - **Test harness:** `tests/common/mod.rs` provides `Engine::spawn()` and `Engine::spawn_with(args: &[&str], envs: &[(&str, &str)])`, plus `call`, `notified`, `wait_for`, `next`, the `seen`/`all` fields and `fixture(name) -> String`. The harness always sets `AURORA_OUTPUT=null`, `AURORA_NULL_SPEED=10`, `AURORA_OSMEDIA=0`.

- [ ] **Step 1: Move the RPC test harness into `tests/common/mod.rs`**

Create `engine/crates/aurora-engine/tests/common/mod.rs`:

```rust
#![allow(dead_code)]
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub fn fixture(name: &str) -> String { format!("{}/../../../fixtures/out/{name}", env!("CARGO_MANIFEST_DIR")) }

/// `seen` is a window that tests may clear; `all` keeps every message for final assertions.
pub struct Engine { pub child: Child, pub stdin: ChildStdin, rx: Receiver<String>, next_id: u64, pub seen: Vec<Value>, pub all: Vec<Value> }

impl Engine {
    pub fn spawn() -> Self { Self::spawn_with(&[], &[]) }

    pub fn spawn_with(args: &[&str], envs: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_aurora-engine"));
        cmd.args(args).env("AURORA_OUTPUT", "null").env("AURORA_NULL_SPEED", "10").env("AURORA_OSMEDIA", "0");
        for (k, v) in envs { cmd.env(k, v); }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || for line in BufReader::new(stdout).lines().map_while(Result::ok) { let _ = tx.send(line); });
        Engine { child, stdin, rx, next_id: 1, seen: vec![], all: vec![] }
    }

    pub fn next(&mut self, timeout: Duration) -> Value {
        let line = self.rx.recv_timeout(timeout).expect("engine went quiet");
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
        self.seen.push(v.clone());
        self.all.push(v.clone());
        v
    }

    pub fn wait_for(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        if let Some(v) = self.seen.iter().find(|v| pred(v)) { return v.clone(); }
        loop {
            assert!(start.elapsed() < Duration::from_secs(30), "timed out; seen: {:#?}", self.seen);
            let v = self.next(Duration::from_secs(30));
            if pred(&v) { return v; }
        }
    }

    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
        self.stdin.flush().unwrap();
        self.wait_for(|v| v["id"] == id)
    }

    /// `call` that asserts success and returns `result`.
    pub fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params);
        assert!(v.get("error").is_none(), "{method} failed: {v}");
        v["result"].clone()
    }

    pub fn notified(&mut self, method: &'static str, pred: impl Fn(&Value) -> bool) -> Value {
        self.wait_for(move |v| v["method"] == method && pred(&v["params"]))["params"].clone()
    }

    /// Asks for a clean shutdown and waits for exit (the engine saves state on the way out).
    pub fn close(mut self) -> std::process::ExitStatus {
        let id = self.next_id;
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": "engine.shutdown" })).unwrap();
        self.stdin.flush().unwrap();
        let start = Instant::now();
        loop {
            if let Some(s) = self.child.try_wait().unwrap() { return s; }
            assert!(start.elapsed() < Duration::from_secs(10), "engine didn't exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Engine { fn drop(&mut self) { let _ = self.child.kill(); } }
```

Change the top of `engine/crates/aurora-engine/tests/rpc.rs` so the harness definitions are gone (they now live in `common`). Keep every test body unchanged:

```rust
mod common;
use common::{fixture, Engine};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
```

Remove the old `fixture`, `struct Engine`, `impl Engine` and `impl Drop for Engine` blocks from `rpc.rs`.

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --test rpc`
Expected: the existing 6 tests still pass.

- [ ] **Step 2: Write the failing tests**

Add `tempfile.workspace = true` under a new `[dev-dependencies]` section in `engine/crates/aurora-engine/Cargo.toml`.

Create `engine/crates/aurora-engine/tests/library_rpc.rs`:

```rust
mod common;
use common::{fixture, Engine};
use serde_json::{json, Value};

fn music_dir(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for f in files { std::fs::copy(fixture(f), dir.path().join(f)).unwrap(); }
    dir
}

fn with_library(data: &std::path::Path) -> Engine {
    Engine::spawn_with(&["rpc", "--data-dir", &data.to_string_lossy()], &[])
}

fn wait_scan_done(e: &mut Engine) -> Value { e.notified("library.scanProgress", |p| p["phase"] == "done") }

#[test]
fn library_methods_need_a_data_dir() {
    let mut e = Engine::spawn();
    for (m, p) in [("library.folders", json!({})), ("playlists.list", json!({})), ("queue.set", json!({ "trackIds": [1] }))] {
        let v = e.call(m, p);
        assert_eq!(v["error"]["code"], -32008, "{m}: {v}");
        assert!(v["error"]["message"].as_str().unwrap().contains("--data-dir"));
    }
}

#[test]
fn add_folder_scans_lists_and_plays_by_track_id() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac", "short_b.flac"]);
    let mut e = with_library(data.path());
    assert_eq!(e.next(std::time::Duration::from_secs(10))["method"], "engine.ready");
    let folder = e.ok("library.addFolder", json!({ "path": music.path() }));
    assert!(folder["id"].as_i64().unwrap() > 0);
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "folders"));
    let done = wait_scan_done(&mut e);
    assert_eq!(done["added"], 2);
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "tracks"));
    let tracks = e.ok("library.tracks", json!({ "sort": "title" }));
    assert_eq!(tracks["total"], 2);
    let ids: Vec<i64> = tracks["items"].as_array().unwrap().iter().map(|t| t["id"].as_i64().unwrap()).collect();
    assert_eq!(tracks["items"][0]["title"], "short_a");

    e.ok("queue.set", json!({ "trackIds": ids, "startIndex": 1 }));
    let q = e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.len() == 2));
    assert_eq!(q["items"][1]["trackId"], ids[1]);
    assert_eq!(q["current"], 1);
    e.ok("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert_eq!(st["track"]["trackId"], ids[1]);
    assert_eq!(st["track"]["title"], "short_b");
}

#[test]
fn queue_set_validates_ids_and_start_index() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac"]);
    let mut e = with_library(data.path());
    e.ok("library.addFolder", json!({ "path": music.path() }));
    wait_scan_done(&mut e);
    let id = e.ok("library.tracks", json!({}))["items"][0]["id"].as_i64().unwrap();
    assert_eq!(e.call("queue.set", json!({ "trackIds": [id, 999_999] }))["error"]["code"], -32001);
    assert_eq!(e.call("queue.set", json!({ "trackIds": [id], "startIndex": 1 }))["error"]["code"], -32009);
    assert_eq!(e.call("queue.move", json!({ "from": 0, "to": 5 }))["error"]["code"], -32009);
}

#[test]
fn add_folder_errors_are_typed() {
    let data = tempfile::tempdir().unwrap();
    let mut e = with_library(data.path());
    let v = e.call("library.addFolder", json!({ "path": data.path().join("nope") }));
    assert_eq!(v["error"]["code"], -32009);
    assert_eq!(v["error"]["data"]["name"], "INVALID_ARGUMENT");
    assert_eq!(e.call("library.album", json!({ "id": 42 }))["error"]["code"], -32001);
    assert_eq!(e.call("library.nothing", json!({}))["error"]["code"], -32601);
}
```

Append to `engine/crates/library/tests/artwork.rs`:

```rust
#[test]
fn artwork_info_resolves_the_nearest_cached_size_and_palette() {
    use common::{tagged, Tags};
    use lofty::picture::PictureType;
    let blue = common::png(400, 400, [0, 0, 255]);
    let s = common::scanned(|root| {
        tagged("flac16_44k.flac", &root.join("a.flac"), &Tags { album: Some("A"), pictures: &[(PictureType::CoverFront, &blue)], ..Default::default() });
    });
    let id: i64 = s.lib.conn().query_row("SELECT id FROM artwork", [], |r| r.get(0)).unwrap();
    let info = s.lib.artwork_info(id, 200).unwrap();
    assert!(info.path.ends_with("-300.webp"), "{}", info.path);
    assert!(std::path::Path::new(&info.path).exists());
    assert_eq!(info.palette.unwrap().dominant, "#0000ff");
    assert!(matches!(s.lib.artwork_info(9999, 64), Err(library::LibError::NotFound(_))));
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --test library_rpc`
Expected: tests fail. Without `--data-dir` support, clap rejects the argument and the engine exits, and the library methods return -32601.

- [ ] **Step 4: Implement `Library::artwork_info`**

Append to `engine/crates/library/src/artwork.rs`:

```rust
impl crate::Library {
    /// Cache file of the nearest stored size for artwork `id`, plus its palette.
    pub fn artwork_info(&self, id: i64, size: u32) -> crate::Result<protocol::ArtworkInfo> {
        use rusqlite::OptionalExtension;
        let row: Option<(String, Option<String>, Option<String>, Option<String>)> = self
            .conn()
            .query_row("SELECT hash, dominant, vibrant, muted FROM artwork WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?;
        let (hash, dominant, vibrant, muted) = row.ok_or_else(|| crate::LibError::NotFound(format!("artwork {id}")))?;
        let path = path_for(&self.artwork_dir(), &hash, nearest_size(size));
        if !path.exists() {
            return Err(crate::LibError::FileMissing(path.display().to_string()));
        }
        let palette = match (dominant, vibrant, muted) {
            (Some(dominant), Some(vibrant), Some(muted)) => Some(Palette { dominant, vibrant, muted }),
            _ => None,
        };
        Ok(protocol::ArtworkInfo { path: path.to_string_lossy().into_owned(), palette })
    }
}
```

- [ ] **Step 5: Implement the library service**

Add `library.workspace = true` to `[dependencies]` in `engine/crates/aurora-engine/Cargo.toml`.

Create `engine/crates/aurora-engine/src/rpc/library_service.rs`:

```rust
//! Owns the library on the engine side: a scan worker thread (its own DB connection), the folder
//! watcher, and the notifications they produce.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use library::scan::{Fallback, ScanOptions, Scanner};
use library::watch::{group_by_folder, Watcher};
use library::{LibError, Library, OpenReport};
use protocol::{EngineErrorEvent, ErrorCode, Folder, LibraryChanged, LibraryScope};

use super::writer::Writer;

#[derive(Debug, Clone, PartialEq)]
pub struct ScanJob {
    pub folder_id: i64,
    /// Files/folders to rescan; empty means the whole folder.
    pub roots: Vec<PathBuf>,
    pub full: bool,
}

/// Merges queued jobs per folder: a whole-folder job absorbs partial ones; partial roots are unioned.
pub fn coalesce(jobs: Vec<ScanJob>) -> Vec<ScanJob> {
    let mut out: Vec<ScanJob> = vec![];
    for j in jobs {
        match out.iter_mut().find(|o| o.folder_id == j.folder_id) {
            Some(o) => {
                if o.roots.is_empty() || j.roots.is_empty() {
                    o.roots.clear();
                } else {
                    for r in j.roots { if !o.roots.contains(&r) { o.roots.push(r); } }
                }
                o.full |= j.full;
            }
            None => out.push(j),
        }
    }
    out
}

pub fn changed(out: &Writer, scopes: &[LibraryScope]) {
    out.notify("library.changed", LibraryChanged { scopes: scopes.to_vec() });
}

pub fn notify_error(out: &Writer, code: ErrorCode, message: String) {
    out.notify("engine.error", EngineErrorEvent { code: code.code(), name: code.name().into(), message, path: None });
}

enum Msg { Scan(ScanJob), Stop }

#[derive(Default)]
struct WorkerState { scanning: Mutex<Option<i64>>, cancel: AtomicBool, stopping: AtomicBool }

pub struct LibraryService {
    pub lib: Arc<Library>,
    out: Arc<Writer>,
    tx: Sender<Msg>,
    worker: Mutex<Option<JoinHandle<()>>>,
    watcher: Mutex<Option<Watcher>>,
    roots: Arc<Mutex<Vec<(i64, PathBuf)>>>,
    state: Arc<WorkerState>,
    report: Mutex<Option<OpenReport>>,
}

fn scan_worker(mut scanner: Scanner, rx: Receiver<Msg>, out: Arc<Writer>, state: Arc<WorkerState>) {
    while let Ok(first) = rx.recv() {
        let mut jobs = vec![];
        for m in std::iter::once(first).chain(rx.try_iter()) {
            match m { Msg::Scan(j) => jobs.push(j), Msg::Stop => return }
        }
        for job in coalesce(jobs) {
            if state.stopping.load(SeqCst) { return; }
            state.cancel.store(false, SeqCst);
            *state.scanning.lock().unwrap() = Some(job.folder_id);
            let result = scanner.scan(job.folder_id, &job.roots, job.full, &mut |p| out.notify("library.scanProgress", p), &state.cancel);
            *state.scanning.lock().unwrap() = None;
            match result {
                Ok(s) => {
                    let any = s.added + s.updated + s.missing + s.restored + s.purged > 0;
                    if any { changed(&out, &[LibraryScope::Tracks, LibraryScope::Folders]); }
                }
                Err(LibError::Cancelled) | Err(LibError::NotFound(_)) => {}
                Err(e) => notify_error(&out, e.code(), format!("Library scan failed: {e}")),
            }
        }
    }
}

impl LibraryService {
    /// Opens the library in `dir` and starts the scan worker. Sends nothing yet: `engine.ready`
    /// must be the first line on stdout, so call `begin` after the handshake.
    pub fn open(dir: &Path, out: Arc<Writer>, fallback: Fallback) -> Result<Self, LibError> {
        let (lib, report) = Library::open(dir)?;
        let lib = Arc::new(lib);
        let scanner = lib.scanner(ScanOptions { fallback: Some(fallback), now: None })?;
        let (tx, rx) = crossbeam_channel::unbounded();
        let state = Arc::new(WorkerState::default());
        let worker = {
            let (out, state) = (out.clone(), state.clone());
            std::thread::Builder::new().name("aurora-scan".into()).spawn(move || scan_worker(scanner, rx, out, state)).expect("spawn scan worker")
        };
        let roots = lib.folders()?.into_iter().map(|f| (f.id, PathBuf::from(f.path))).collect();
        Ok(LibraryService {
            lib, out, tx, worker: Mutex::new(Some(worker)), watcher: Mutex::new(None), roots: Arc::new(Mutex::new(roots)), state,
            report: Mutex::new(Some(report)),
        })
    }

    fn queue_scan(&self, folder_id: i64, roots: Vec<PathBuf>, full: bool) {
        let _ = self.tx.send(Msg::Scan(ScanJob { folder_id, roots, full }));
    }

    /// After the handshake: reports a database recovery, watches every folder and queues an
    /// incremental scan of each (catching changes made while Aurora was closed).
    pub fn begin(&self) {
        if let Some(report) = self.report.lock().unwrap().take() && let Some(aside) = report.recovered_from {
            notify_error(&self.out, ErrorCode::DbError, format!(
                "The library database was damaged and has been rebuilt (the damaged file was kept as {}). Rescanning your folders.",
                aside.display()
            ));
        }
        let tx = self.tx.clone();
        let roots = self.roots.clone();
        match Watcher::new(move |paths| {
            let folders = roots.lock().unwrap().clone();
            for (folder_id, roots) in group_by_folder(&folders, paths) {
                let full = roots.is_empty();
                let _ = tx.send(Msg::Scan(ScanJob { folder_id, roots, full }));
            }
        }) {
            Ok(mut w) => {
                for (_, root) in self.roots.lock().unwrap().iter() {
                    if let Err(e) = w.watch(root) { tracing::warn!(root = %root.display(), error = %e, "not watching folder"); }
                }
                *self.watcher.lock().unwrap() = Some(w);
            }
            Err(e) => tracing::warn!(error = %e, "folder watching unavailable; changes need a manual rescan"),
        }
        let ids: Vec<i64> = self.roots.lock().unwrap().iter().map(|(id, _)| *id).collect();
        for id in ids { self.queue_scan(id, vec![], true); }
    }

    pub fn notify_changed(&self, scopes: &[LibraryScope]) { changed(&self.out, scopes); }

    pub fn add_folder(&self, path: &Path) -> Result<Folder, LibError> {
        let f = self.lib.add_folder(path)?;
        let root = PathBuf::from(&f.path);
        self.roots.lock().unwrap().push((f.id, root.clone()));
        if let Some(w) = self.watcher.lock().unwrap().as_mut() && let Err(e) = w.watch(&root) {
            tracing::warn!(root = %root.display(), error = %e, "not watching folder");
        }
        self.queue_scan(f.id, vec![], true);
        changed(&self.out, &[LibraryScope::Folders]);
        Ok(f)
    }

    pub fn remove_folder(&self, id: i64) -> Result<(), LibError> {
        let root = PathBuf::from(self.lib.folder(id)?.path);
        if *self.state.scanning.lock().unwrap() == Some(id) {
            self.state.cancel.store(true, SeqCst);
            let deadline = Instant::now() + Duration::from_secs(5);
            while *self.state.scanning.lock().unwrap() == Some(id) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if let Some(w) = self.watcher.lock().unwrap().as_mut() { let _ = w.unwatch(&root); }
        self.roots.lock().unwrap().retain(|(f, _)| *f != id);
        self.lib.remove_folder(id)?;
        changed(&self.out, &[LibraryScope::Folders, LibraryScope::Tracks]);
        Ok(())
    }

    pub fn rescan(&self, folder_id: Option<i64>) -> Result<(), LibError> {
        let ids = match folder_id {
            Some(id) => { self.lib.folder(id)?; vec![id] }
            None => self.roots.lock().unwrap().iter().map(|(id, _)| *id).collect(),
        };
        for id in ids { self.queue_scan(id, vec![], true); }
        Ok(())
    }

    /// Cancels any running scan, stops the worker and the watcher.
    pub fn shutdown(&self) {
        self.state.stopping.store(true, SeqCst);
        self.state.cancel.store(true, SeqCst);
        let _ = self.tx.send(Msg::Stop);
        *self.watcher.lock().unwrap() = None;
        if let Some(j) = self.worker.lock().unwrap().take() { let _ = j.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: i64, roots: &[&str], full: bool) -> ScanJob { ScanJob { folder_id: id, roots: roots.iter().map(PathBuf::from).collect(), full } }

    #[test]
    fn coalescing_merges_per_folder() {
        let got = coalesce(vec![job(1, &["/a/x"], false), job(2, &[], true), job(1, &["/a/y", "/a/x"], false)]);
        assert_eq!(got, vec![job(1, &["/a/x", "/a/y"], false), job(2, &[], true)]);
        let got = coalesce(vec![job(1, &["/a/x"], false), job(1, &[], true)]);
        assert_eq!(got, vec![job(1, &[], true)]);
    }
}
```

- [ ] **Step 6: Implement the library/playlist methods**

Create `engine/crates/aurora-engine/src/rpc/library_methods.rs`:

```rust
//! `library.*` and `playlists.*` (spec §4.3).
use std::path::Path;

use library::LibError;
use protocol::rpc::*;
use protocol::{ErrorCode, GaplessSummary, LibraryScope, ListQuery, TrackDetail};
use serde::Serialize;
use serde_json::{json, Value};

use super::dispatch::params;
use super::library_service::LibraryService;

pub const UNAVAILABLE: &str = "library unavailable: engine started without --data-dir";

pub fn lib_err(e: LibError) -> RpcError {
    let c = e.code();
    RpcError { code: c.code(), message: e.to_string(), data: Some(json!({ "name": c.name() })) }
}

pub fn unavailable() -> RpcError {
    RpcError { code: ErrorCode::DbError.code(), message: UNAVAILABLE.into(), data: Some(json!({ "name": ErrorCode::DbError.name() })) }
}

fn reply<T: Serialize>(r: Result<T, LibError>) -> Result<Value, RpcError> {
    r.map(|v| serde_json::to_value(v).expect("protocol types serialize")).map_err(lib_err)
}

/// Encoder delay/padding for the track-info dialog, probed from the file itself.
fn with_gapless(mut d: TrackDetail) -> TrackDetail {
    if !d.track.missing && let Ok(dec) = engine_core::decode::open(Path::new(&d.track.path)) {
        d.gapless = dec.info().gapless.as_ref().map(|g| GaplessSummary { delay: g.delay, padding: g.padding });
    }
    d
}

/// Handles `library.*` and `playlists.*`; `None` for any other method.
pub fn dispatch(svc: Option<&LibraryService>, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !(method.starts_with("library.") || method.starts_with("playlists.")) { return None; }
    Some(match svc { Some(svc) => handle(svc, method, p), None => Err(unavailable()) })
}

fn handle(svc: &LibraryService, method: &str, p: Value) -> Result<Value, RpcError> {
    let lib = &svc.lib;
    let mutate = |r: Result<Value, RpcError>| {
        if r.is_ok() { svc.notify_changed(&[LibraryScope::Playlists]); }
        r
    };
    match method {
        "library.folders" => reply(lib.folders()),
        "library.addFolder" => { let p: PathParams = params(p)?; reply(svc.add_folder(Path::new(&p.path))) }
        "library.removeFolder" => { let p: IdParams = params(p)?; reply(svc.remove_folder(p.id)) }
        "library.rescan" => { let p: RescanParams = params(p)?; reply(svc.rescan(p.folder_id)) }
        "library.albums" => { let q: ListQuery = params(p)?; reply(lib.albums(&q)) }
        "library.artists" => { let q: ListQuery = params(p)?; reply(lib.artists(&q)) }
        "library.genres" => { let q: ListQuery = params(p)?; reply(lib.genres(&q)) }
        "library.tracks" => { let q: ListQuery = params(p)?; reply(lib.tracks(&q)) }
        "library.album" => { let p: IdParams = params(p)?; reply(lib.album(p.id)) }
        "library.artist" => { let p: IdParams = params(p)?; reply(lib.artist(p.id)) }
        "library.genre" => { let p: IdParams = params(p)?; reply(lib.genre(p.id)) }
        "library.track" => { let p: IdParams = params(p)?; reply(lib.track(p.id).map(with_gapless)) }
        "library.search" => { let p: SearchParams = params(p)?; reply(lib.search(&p.text, p.limit.unwrap_or(20))) }
        "library.artwork" => { let p: ArtworkParams = params(p)?; reply(lib.artwork_info(p.id, p.size)) }
        "library.recent" => { let p: RecentParams = params(p)?; reply(lib.recent(p.kind, p.limit.unwrap_or(20))) }
        "playlists.list" => reply(lib.playlists()),
        "playlists.get" => { let p: IdParams = params(p)?; reply(lib.playlist(p.id)) }
        "playlists.create" => { let p: NameParams = params(p)?; mutate(reply(lib.create_playlist(&p.name))) }
        "playlists.rename" => { let p: RenameParams = params(p)?; mutate(reply(lib.rename_playlist(p.id, &p.name))) }
        "playlists.delete" => { let p: IdParams = params(p)?; mutate(reply(lib.delete_playlist(p.id))) }
        "playlists.addTracks" => { let p: PlaylistAddParams = params(p)?; mutate(reply(lib.add_to_playlist(p.id, &p.track_ids, p.index))) }
        "playlists.removeItems" => { let p: PlaylistRemoveParams = params(p)?; mutate(reply(lib.remove_from_playlist(p.id, &p.item_ids))) }
        "playlists.reorder" => { let p: PlaylistReorderParams = params(p)?; mutate(reply(lib.reorder_playlist(p.id, p.from, p.to))) }
        "playlists.importM3u" => { let p: PathParams = params(p)?; mutate(reply(lib.import_m3u(Path::new(&p.path)))) }
        "playlists.exportM3u" => { let p: ExportParams = params(p)?; reply(lib.export_m3u(p.id, Path::new(&p.path))) }
        other => Err(RpcError { code: -32601, message: format!("method not found: {other}"), data: None }),
    }
}
```

- [ ] **Step 7: Wire the queue methods into `dispatch.rs`**

Replace `engine/crates/aurora-engine/src/rpc/dispatch.rs` with:

```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use engine_core::output::OutputBackend;
use engine_core::player::{Command, NewEntry, PlayerHandle};
use protocol::rpc::*;
use protocol::{ErrorCode, QueueChanged, TrackRow};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::library_methods::{self, lib_err, unavailable};
use super::library_service::LibraryService;

pub struct Ctx {
    pub player: PlayerHandle,
    pub backend: Arc<dyn OutputBackend>,
    pub queue: Arc<Mutex<QueueChanged>>,
    pub library: Option<Arc<LibraryService>>,
}

pub enum Outcome { Reply(Value), Shutdown }

fn err(code: i32, message: impl Into<String>) -> RpcError { RpcError { code, message: message.into(), data: None } }

pub(super) fn params<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| err(-32602, format!("invalid params: {e}")))
}

/// A queue entry carrying the library's metadata for `t`.
pub fn entry_from_row(t: TrackRow) -> NewEntry {
    NewEntry {
        path: PathBuf::from(t.path),
        title: Some(t.title),
        track_id: Some(t.id),
        artist: (!t.artist.is_empty()).then_some(t.artist),
        album: (!t.album.is_empty()).then_some(t.album),
        artwork_id: t.artwork_id,
        duration_ms: t.duration_ms,
    }
}

fn entries_for(ctx: &Ctx, ids: &[i64]) -> Result<Vec<NewEntry>, RpcError> {
    let svc = ctx.library.as_deref().ok_or_else(unavailable)?;
    Ok(svc.lib.tracks_by_ids(ids).map_err(lib_err)?.into_iter().map(entry_from_row).collect())
}

fn queue_len(ctx: &Ctx) -> usize { ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).items.len() }

pub fn dispatch(ctx: &Ctx, method: &str, p: Value) -> Result<Outcome, RpcError> {
    let ok = |c: Command| -> Result<Outcome, RpcError> { ctx.player.send(c); Ok(Outcome::Reply(Value::Null)) };
    let invalid = |m: &str| err(ErrorCode::InvalidArgument.code(), m);
    match method {
        "player.play" => { let p: PlayParams = params(p)?; ok(Command::Play(p.queue_item_id)) }
        "player.pause" => ok(Command::Pause),
        "player.toggle" => ok(Command::Toggle),
        "player.stop" => ok(Command::Stop),
        "player.next" => ok(Command::Next),
        "player.prev" => ok(Command::Prev),
        "player.seek" => { let p: SeekParams = params(p)?; ok(Command::Seek(p.ms)) }
        "player.setVolume" => {
            let p: VolumeParams = params(p)?;
            if !(0.0..=1.0).contains(&p.volume) { return Err(invalid("volume must be in [0, 1]")); }
            ok(Command::SetVolume(p.volume))
        }
        "player.setMode" => { let p: ModeParams = params(p)?; ok(Command::SetMode(p.mode)) }
        "player.setDevice" => { let p: DeviceParams = params(p)?; ok(Command::SetDevice(p.id)) }
        "player.state" => Ok(Outcome::Reply(serde_json::to_value(ctx.player.state()).unwrap())),
        "queue.setPaths" => {
            let p: QueueSetPathsParams = params(p)?;
            if (p.start_index as usize) >= p.paths.len().max(1) { return Err(invalid("startIndex out of range")); }
            ok(Command::SetQueue { items: p.paths.into_iter().map(NewEntry::from_path).collect(), start: p.start_index as usize })
        }
        "queue.set" => {
            let p: QueueSetParams = params(p)?;
            let items = entries_for(ctx, &p.track_ids)?;
            let start = p.start_index.unwrap_or(0) as usize;
            if start >= items.len().max(1) { return Err(invalid("startIndex out of range")); }
            ok(Command::SetQueue { items, start })
        }
        "queue.add" => { let p: TrackIdsParams = params(p)?; ok(Command::Add(entries_for(ctx, &p.track_ids)?)) }
        "queue.insertNext" => { let p: TrackIdsParams = params(p)?; ok(Command::InsertNext(entries_for(ctx, &p.track_ids)?)) }
        "queue.move" => {
            let p: QueueMoveParams = params(p)?;
            let len = queue_len(ctx);
            if p.from as usize >= len || p.to as usize >= len { return Err(invalid("queue position out of range")); }
            ok(Command::Move { from: p.from as usize, to: p.to as usize })
        }
        "queue.remove" => { let p: QueueRemoveParams = params(p)?; ok(Command::Remove(p.item_ids)) }
        "queue.clear" => ok(Command::Clear),
        "queue.setShuffle" => { let p: ShuffleParams = params(p)?; ok(Command::SetShuffle(p.shuffle)) }
        "queue.setRepeat" => { let p: RepeatParams = params(p)?; ok(Command::SetRepeat(p.repeat)) }
        "queue.get" => Ok(Outcome::Reply(serde_json::to_value(&*ctx.queue.lock().unwrap_or_else(|p| p.into_inner())).unwrap())),
        "devices.list" => ctx.backend.devices().map(|d| Outcome::Reply(serde_json::to_value(d).unwrap())).map_err(|e| err(e.code().code(), e.to_string())),
        "engine.shutdown" => Ok(Outcome::Shutdown),
        other => match library_methods::dispatch(ctx.library.as_deref(), other, p) {
            Some(r) => r.map(Outcome::Reply),
            None => Err(err(-32601, format!("method not found: {other}"))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_carry_library_metadata_and_omit_unknowns() {
        let row = TrackRow {
            id: 7, path: "/m/a.flac".into(), title: "A".into(), artist: String::new(), artist_id: None, album: "Al".into(), album_id: None,
            album_artist: None, track_no: None, disc_no: None, year: None, genre: None, duration_ms: Some(5), codec: None, container: "flac".into(),
            bitrate_kbps: None, sample_rate: None, bit_depth: None, channels: None, artwork_id: Some(3), missing: false, unplayable: None,
            play_count: 0, added_at: 0, last_played_at: None,
        };
        let e = entry_from_row(row);
        assert_eq!((e.track_id, e.artist, e.album.as_deref(), e.artwork_id, e.duration_ms), (Some(7), None, Some("Al"), Some(3), Some(5)));
        assert_eq!(e.title.as_deref(), Some("A"));
    }
}
```

`queue.setPaths` keeps M1's validation, minus its redundant second clause: `startIndex` must be below `max(len, 1)`. That covers both the empty and non-empty cases.

- [ ] **Step 8: Wire `run()` and the CLI**

Replace `engine/crates/aurora-engine/src/rpc/mod.rs` with:

```rust
mod dispatch;
mod library_methods;
mod library_service;
mod writer;

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use engine_core::output::null::{NullBackend, NullMode};
use engine_core::output::{system::SystemBackend, OutputBackend};
use engine_core::player::{spawn_player, PlayerConfig, PlayerEvent};
use protocol::rpc::{Request, RpcError};
use protocol::{EngineReady, ErrorCode, LibraryScope, QueueChanged, PROTOCOL_VERSION};
use serde_json::{json, Value};

use dispatch::{dispatch, Ctx, Outcome};
use library_service::{changed, notify_error, LibraryService};
use writer::Writer;

#[derive(Default)]
pub struct RunOptions {
    /// Where the library, artwork cache and session live. None = no persistence (M1 behaviour).
    pub data_dir: Option<PathBuf>,
}

fn features() -> Vec<String> {
    let mut f = vec!["opus".to_string(), "dsd".into(), "dop".into()];
    if cfg!(feature = "fdk-aac") { f.push("fdk-aac".into()); }
    f
}

fn backend_from_env() -> Arc<dyn OutputBackend> {
    if std::env::var("AURORA_OUTPUT").as_deref() == Ok("null") {
        let speed = std::env::var("AURORA_NULL_SPEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
        Arc::new(NullBackend::new(NullMode::Realtime { speed }).0)
    } else {
        Arc::new(SystemBackend::new())
    }
}

/// Reads properties through our own decoders for files `lofty` can't parse (DSF/DFF).
fn fallback_probe() -> library::scan::Fallback {
    Arc::new(|p: &Path| {
        let dec = engine_core::decode::open(p).ok()?;
        let i = dec.info();
        let s = engine_core::decode::source_summary(i);
        Some(library::tags::AudioProps {
            duration_ms: i.total_frames.map(|f| f * 1000 / i.spec.sample_rate.max(1) as u64),
            codec: Some(i.codec),
            container: library::tags::container_of(p),
            bitrate_kbps: i.bitrate_kbps,
            sample_rate: Some(i.dsd_rate.unwrap_or(s.sample_rate)),
            bit_depth: s.bit_depth,
            channels: Some(s.channels),
        })
    })
}

/// Player errors that mean the file itself can't be played (recorded on its library track).
const UNPLAYABLE: [ErrorCode; 4] = [ErrorCode::FileMissing, ErrorCode::UnsupportedFormat, ErrorCode::CodecNotInBuild, ErrorCode::DecodeFailed];

fn record_play(svc: &LibraryService, out: &Writer, track_id: Option<i64>, path: &Path) {
    let path = path.to_string_lossy();
    let Some(id) = track_id.or_else(|| svc.lib.track_id_for_path(&path)) else { return };
    let _ = svc.lib.set_unplayable(&path, None);
    match svc.lib.record_play(id, library::unix_now()) {
        Ok(()) => changed(out, &[LibraryScope::History]),
        Err(e) => tracing::warn!(error = %e, "couldn't record a play"),
    }
}

pub fn run(opts: RunOptions) -> i32 {
    let out = Arc::new(Writer::new());
    let backend = backend_from_env();
    let (tx, rx) = crossbeam_channel::unbounded::<PlayerEvent>();
    let player = spawn_player(backend.clone(), PlayerConfig::default(), tx);
    let queue = Arc::new(Mutex::new(QueueChanged { items: vec![], current: None }));
    let (library, library_error) = match &opts.data_dir {
        Some(dir) => match LibraryService::open(dir, out.clone(), fallback_probe()) {
            Ok(s) => (Some(Arc::new(s)), None),
            Err(e) => (None, Some(e)),
        },
        None => (None, None),
    };

    out.notify("engine.ready", EngineReady { version: env!("CARGO_PKG_VERSION").into(), protocol_version: PROTOCOL_VERSION, features: features() });
    if let Some(e) = library_error { notify_error(&out, e.code(), format!("Library unavailable: {e}")); }
    if let Some(svc) = &library { svc.begin(); }

    let (out2, queue2, lib2) = (out.clone(), queue.clone(), library.clone());
    std::thread::Builder::new().name("aurora-events".into()).spawn(move || {
        for ev in rx.iter() {
            match ev {
                PlayerEvent::State(s) => out2.notify("player.state", s),
                PlayerEvent::SignalPath(s) => out2.notify("player.signalPath", s),
                PlayerEvent::Queue(q) => { *queue2.lock().unwrap_or_else(|p| p.into_inner()) = q.clone(); out2.notify("queue.changed", q) }
                PlayerEvent::Error(e) => {
                    if let (Some(svc), Some(path)) = (&lib2, &e.path)
                        && UNPLAYABLE.iter().any(|c| c.code() == e.code)
                        && svc.lib.set_unplayable(path, Some(&e.name)).unwrap_or(false)
                    {
                        changed(&out2, &[LibraryScope::Tracks]);
                    }
                    out2.notify("engine.error", e)
                }
                PlayerEvent::DevicesChanged(d) => out2.notify("devices.changed", d),
                PlayerEvent::Played { track_id, path } => if let Some(svc) = &lib2 { record_play(svc, &out2, track_id, &path) },
                PlayerEvent::Ended => {}
            }
        }
    }).expect("spawn event thread");

    let ctx = Ctx { player, backend, queue, library: library.clone() };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => { out.send(&json!({ "jsonrpc": "2.0", "id": Value::Null, "error": RpcError { code: -32700, message: format!("parse error: {e}"), data: None } })); continue; }
        };
        let id = req.id.clone();
        let result = dispatch(&ctx, &req.method, req.params);
        let Some(id) = id else { continue }; // notifications from the client get no response
        match result {
            Ok(Outcome::Reply(v)) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": v })),
            Ok(Outcome::Shutdown) => { out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null })); break; }
            Err(e) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "error": e })),
        }
    }
    if let Some(svc) = &library { svc.shutdown(); }
    ctx.player.shutdown();
    0
}
```

In `engine/crates/aurora-engine/src/main.rs`:

```rust
    /// Run the JSON-RPC server on stdio (default).
    Rpc {
        /// Folder for the library database, artwork cache and session. Without it nothing is persisted.
        #[arg(long, value_name = "DIR")]
        data_dir: Option<PathBuf>,
    },
```

The dispatch in `main()` becomes:

```rust
        Some(Cmd::Rpc { data_dir }) => rpc::run(rpc::RunOptions { data_dir }),
        None => rpc::run(rpc::RunOptions::default()),
```

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml --workspace`
Expected: all pass, including:
- the 4 `library_rpc` tests;
- `artwork_info_resolves_the_nearest_cached_size_and_palette`;
- `coalescing_merges_per_folder`;
- `entries_carry_library_metadata_and_omit_unknowns`;
- the existing rpc and cli tests.

Then run: `cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add engine
git commit -m "feat(engine): --data-dir library service with scan worker, watcher and library/playlist/queue RPC"
```

### Task 14: Session restore (`session.json`)

**Files:**
- Create: `engine/crates/aurora-engine/src/rpc/session.rs` (with unit tests)
- Modify: `engine/crates/aurora-engine/src/rpc/mod.rs` (load + restore at startup, 5 s saver, final save), `engine/crates/engine-core/src/player/mod.rs` (`PlayerHandle::state_reader`)

**Interfaces:**
- Consumes: `Command::Restore` and `Restore` (Task 12); `RunOptions.data_dir` and the `queue` mirror (Task 13).
- Produces:
  - **`rpc::session` items:**
    - `FILE = "session.json"` and `VERSION = 1`.
    - `Session { version, items: Vec<SessionItem>, current: usize, position_ms: u64, shuffle, repeat: RepeatMode, volume: f32, mode: OutputMode }` (camelCase JSON).
    - `SessionItem { path, title, track_id, artist, album, artwork_id, duration_ms }`.
    - `snapshot(&QueueChanged, &PlayerState) -> Session`.
    - `save(dir, &Session) -> io::Result<()>`: atomic, via a temp file and rename.
    - `load(dir) -> Option<Session>`: a missing, unparsable or other-version file gives `None` and a warning, never an error.
    - `restore(Session) -> Option<Restore>`: `None` for an empty queue.
  - **`engine_core::player::StateReader`** (Clone) with `get() -> PlayerState`, and `PlayerHandle::state_reader() -> StateReader`.
  - **Engine behaviour with `--data-dir`:**
    - At startup a saved session is restored **paused** (spec §5.2).
    - The session is saved every 5 s when it changed, and once more on clean shutdown (stdin EOF or `engine.shutdown`).
    - A stopped player saves position 0.
    - The output device is not persisted, because device ids aren't stable across boots and the restore uses the system default.

- [ ] **Step 1: Write the failing tests**

Create `engine/crates/aurora-engine/src/rpc/session.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{PlaybackStatus, QueueItem};

    fn item(id: u32, path: &str) -> QueueItem {
        QueueItem { id, path: path.into(), title: format!("t{id}"), track_id: Some(id as i64 * 10), artist: Some("A".into()), album: None, artwork_id: Some(3), duration_ms: Some(1000) }
    }

    fn state(status: PlaybackStatus, index: u32, pos: u64) -> PlayerState {
        PlayerState { status, queue_index: Some(index), position_ms: pos, volume: 0.7, shuffle: true, repeat: RepeatMode::All, mode: OutputMode::BitPerfect, ..Default::default() }
    }

    #[test]
    fn snapshot_round_trips_through_disk_into_a_restore() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueChanged { items: vec![item(1, "/m/a.flac"), item(2, "/m/b.flac")], current: Some(1) };
        let s = snapshot(&q, &state(PlaybackStatus::Paused, 1, 4_200));
        save(dir.path(), &s).unwrap();
        assert!(!dir.path().join(format!("{FILE}.tmp")).exists());
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded, s);
        let r = restore(loaded).unwrap();
        assert_eq!((r.current, r.position_ms, r.shuffle, r.repeat, r.volume, r.mode), (1, 4_200, true, RepeatMode::All, 0.7, OutputMode::BitPerfect));
        assert_eq!(r.items[1].path, std::path::PathBuf::from("/m/b.flac"));
        assert_eq!((r.items[1].track_id, r.items[1].title.as_deref(), r.items[1].duration_ms), (Some(20), Some("t2"), Some(1000)));
    }

    #[test]
    fn stopped_player_saves_position_zero() {
        let q = QueueChanged { items: vec![item(1, "/m/a.flac")], current: Some(0) };
        assert_eq!(snapshot(&q, &state(PlaybackStatus::Stopped, 0, 9_000)).position_ms, 0);
    }

    #[test]
    fn truncated_session_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), br#"{"version":1,"items":[{"path":"/m/a.fl"#).unwrap();
        assert!(load(dir.path()).is_none());
    }

    #[test]
    fn other_versions_and_missing_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_none());
        let q = QueueChanged { items: vec![item(1, "/m/a.flac")], current: Some(0) };
        let mut s = snapshot(&q, &state(PlaybackStatus::Paused, 0, 0));
        s.version = 99;
        save(dir.path(), &s).unwrap();
        assert!(load(dir.path()).is_none());
    }

    #[test]
    fn empty_queue_restores_nothing() {
        let s = snapshot(&QueueChanged { items: vec![], current: None }, &PlayerState::default());
        assert!(restore(s).is_none());
    }
}
```

Add `mod session;` to `src/rpc/mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --bin aurora-engine session`
Expected: compile errors (`snapshot`, `save`, … not found).

- [ ] **Step 3: Implement `session.rs`**

Put this above the test module:

```rust
//! `session.json`: the queue, current item and position, restored paused on startup (spec §2, §5.2).
use std::path::{Path, PathBuf};

use engine_core::player::{NewEntry, Restore};
use protocol::{OutputMode, PlaybackStatus, PlayerState, QueueChanged, RepeatMode};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "session.json";
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionItem {
    pub path: String,
    pub title: String,
    pub track_id: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_id: Option<i64>,
    pub duration_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub version: u32,
    pub items: Vec<SessionItem>,
    pub current: usize,
    pub position_ms: u64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub volume: f32,
    pub mode: OutputMode,
}

pub fn snapshot(queue: &QueueChanged, state: &PlayerState) -> Session {
    Session {
        version: VERSION,
        items: queue.items.iter().map(|i| SessionItem {
            path: i.path.clone(), title: i.title.clone(), track_id: i.track_id, artist: i.artist.clone(),
            album: i.album.clone(), artwork_id: i.artwork_id, duration_ms: i.duration_ms,
        }).collect(),
        current: state.queue_index.or(queue.current).unwrap_or(0) as usize,
        position_ms: if state.status == PlaybackStatus::Stopped { 0 } else { state.position_ms },
        shuffle: state.shuffle,
        repeat: state.repeat,
        volume: state.volume,
        mode: state.mode,
    }
}

pub fn save(dir: &Path, s: &Session) -> std::io::Result<()> {
    let tmp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(s).expect("session serializes"))?;
    std::fs::rename(&tmp, dir.join(FILE))
}

pub fn load(dir: &Path) -> Option<Session> {
    let bytes = std::fs::read(dir.join(FILE)).ok()?;
    match serde_json::from_slice::<Session>(&bytes) {
        Ok(s) if s.version == VERSION => Some(s),
        Ok(s) => { tracing::warn!(version = s.version, "ignoring session.json from another version"); None }
        Err(e) => { tracing::warn!(error = %e, "ignoring unreadable session.json"); None }
    }
}

pub fn restore(s: Session) -> Option<Restore> {
    if s.items.is_empty() { return None; }
    Some(Restore {
        items: s.items.into_iter().map(|i| NewEntry {
            path: PathBuf::from(i.path), title: Some(i.title), track_id: i.track_id, artist: i.artist,
            album: i.album, artwork_id: i.artwork_id, duration_ms: i.duration_ms,
        }).collect(),
        current: s.current,
        position_ms: s.position_ms,
        shuffle: s.shuffle,
        repeat: s.repeat,
        volume: s.volume,
        mode: s.mode,
    })
}
```

- [ ] **Step 4: Add `StateReader` to the player**

In `engine/crates/engine-core/src/player/mod.rs`, next to `Commander`:

```rust
/// A cloneable view of the latest published player state.
#[derive(Clone)]
pub struct StateReader(Arc<Mutex<PlayerState>>);
impl StateReader { pub fn get(&self) -> PlayerState { self.0.lock().unwrap_or_else(|p| p.into_inner()).clone() } }
```

Add this to `impl PlayerHandle`:

```rust
    pub fn state_reader(&self) -> StateReader { StateReader(self.state.clone()) }
```

- [ ] **Step 5: Wire restore and saving into `run()`**

In `src/rpc/mod.rs`:
- Add `use std::sync::atomic::{AtomicBool, Ordering::SeqCst};` and `use std::time::Duration;`.
- Add `use engine_core::player::Command;`.
- Make the three changes below.

(a) Right after `let queue = …;` restore the saved session. The commands are queued before the event thread starts, so their events still follow `engine.ready`:

```rust
    if let Some(dir) = &opts.data_dir && let Some(r) = session::load(dir).and_then(session::restore) {
        player.send(Command::Restore(Box::new(r)));
    }
```

(b) After the event thread is spawned and before `let ctx = …`, start the saver:

```rust
    let stop_saver = Arc::new(AtomicBool::new(false));
    let saver = opts.data_dir.clone().map(|dir| {
        let (reader, queue, stop) = (player.state_reader(), queue.clone(), stop_saver.clone());
        std::thread::Builder::new().name("aurora-session".into()).spawn(move || {
            let mut last: Option<session::Session> = None;
            while !stop.load(SeqCst) {
                for _ in 0..50 { if stop.load(SeqCst) { return; } std::thread::sleep(Duration::from_millis(100)); }
                let snap = session::snapshot(&queue.lock().unwrap_or_else(|p| p.into_inner()), &reader.get());
                if last.as_ref() != Some(&snap) {
                    match session::save(&dir, &snap) { Ok(()) => last = Some(snap), Err(e) => tracing::warn!(error = %e, "couldn't save session.json") }
                }
            }
        }).expect("spawn session saver")
    });
```

(c) After the stdin loop ends and before `svc.shutdown()`, stop the saver and write the final snapshot:

```rust
    stop_saver.store(true, SeqCst);
    if let Some(j) = saver { let _ = j.join(); }
    if let Some(dir) = &opts.data_dir {
        let snap = session::snapshot(&ctx.queue.lock().unwrap_or_else(|p| p.into_inner()), &ctx.player.state());
        if let Err(e) = session::save(dir, &snap) { tracing::warn!(error = %e, "couldn't save session.json"); }
    }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml --workspace`
Expected: the 5 session unit tests pass and everything else still passes. Task 16 adds the end-to-end restart test.

- [ ] **Step 7: Commit**

```bash
git add engine
git commit -m "feat(engine): persist the queue and position to session.json and restore paused on startup"
```

### Task 15: OS media integration (`souvlaki`: SMTC / MPRIS / Now Playing)

**Files:**
- Modify: `engine/Cargo.toml` (`souvlaki` workspace dep), `engine/crates/aurora-engine/Cargo.toml` (souvlaki; Windows-only `windows` dep), `engine/crates/aurora-engine/src/main.rs` (`mod osmedia;`), `engine/crates/aurora-engine/src/rpc/mod.rs` (spawn + forward state)
- Create: `engine/crates/aurora-engine/src/osmedia/mod.rs` (pure mapping + thread, with unit tests), `engine/crates/aurora-engine/src/osmedia/win_window.rs`

**Interfaces:**
- Consumes:
  - From Tasks 12 and 14: `Commander`, `StateReader` and `Command`.
  - From Task 13: `Library::artwork_info`.
  - From Task 1: `protocol::PlayerState`.
- Produces (`crate::osmedia`):
  - **Types and constants:** `Action { Play, Pause, Toggle, Next, Prev, Stop, SeekTo(u64), SeekBy(i64) }` and `SEEK_STEP_MS = 10_000`.
  - **Mapping:**
    - `map_event(&MediaControlEvent) -> Option<Action>`: volume, open-URI, raise and quit are ignored.
    - `to_command(&Action, &PlayerState) -> Command`: relative seeks are clamped to `0..=duration`.
  - **Display:**
    - `file_url(&Path) -> String`.
    - `NowPlaying { title, artist, album, cover: Option<PathBuf>, duration_ms }`.
    - `now_playing(&PlayerState, cover: impl Fn(i64) -> Option<PathBuf>) -> Option<NowPlaying>`.
    - `playback(&PlayerState) -> MediaPlayback`.
  - **Thread:**
    - `enabled() -> bool`: false when `AURORA_OSMEDIA=0`.
    - `spawn(states: Receiver<PlayerState>, commander: Commander, reader: StateReader, cover: impl Fn(i64) -> Option<PathBuf> + Send + 'static)`.
    - It runs its own thread, logs and exits if the OS refuses the controls, and never panics.
  - **Engine behaviour:** now-playing metadata (title, artist, album, duration, 300 px cover as a `file://` URL) is pushed on every track change, and playback status on every status change (spec §3.10). Hardware media keys and the system flyout drive the player.
  - **Ruling recorded here:** on Windows the engine creates its **own hidden window** for SMTC instead of using Electron's HWND.
    - `ISystemMediaTransportControlsInterop::GetForWindow` needs a window owned by the calling process, which is the fallback spec §11 anticipates.
    - The cost: the flyout shows the engine's executable name until M4 sets an app identity.
    - On macOS, remote commands need a main-thread run loop the sidecar doesn't have, so macOS is best-effort (metadata may show, keys may not reach the engine). README documents this.

- [ ] **Step 1: Add the dependencies**

In `engine/Cargo.toml` `[workspace.dependencies]`:

```toml
souvlaki = { version = "0.8.3", default-features = false, features = ["use_zbus"] }
```

The `use_zbus` feature is pure-Rust D-Bus, so Linux builds need no `libdbus-1-dev`.

In `engine/crates/aurora-engine/Cargo.toml`:

```toml
souvlaki.workspace = true

[target.'cfg(windows)'.dependencies]
windows = { version = "0.62", features = ["Win32_Foundation", "Win32_UI_WindowsAndMessaging", "Win32_System_LibraryLoader"] }
```

The `souvlaki` line goes under `[dependencies]`.

- [ ] **Step 2: Write the failing tests**

Create `engine/crates/aurora-engine/src/osmedia/mod.rs` with the test module first, and add `mod osmedia;` to `main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{PlaybackStatus, TrackSummary};
    use souvlaki::{MediaControlEvent as E, MediaPosition, SeekDirection};

    fn playing(pos: u64, dur: Option<u64>) -> PlayerState {
        PlayerState {
            status: PlaybackStatus::Playing, position_ms: pos, duration_ms: dur,
            track: Some(TrackSummary { path: "/m/a.flac".into(), title: "Song".into(), duration_ms: dur, track_id: Some(1), artist: Some("Alpha".into()), album: Some("Light".into()), artwork_id: Some(7) }),
            ..Default::default()
        }
    }

    #[test]
    fn maps_os_events_to_actions() {
        assert_eq!(map_event(&E::Toggle), Some(Action::Toggle));
        assert_eq!(map_event(&E::Previous), Some(Action::Prev));
        assert_eq!(map_event(&E::Seek(SeekDirection::Backward)), Some(Action::SeekBy(-SEEK_STEP_MS)));
        assert_eq!(map_event(&E::SeekBy(SeekDirection::Forward, Duration::from_secs(3))), Some(Action::SeekBy(3_000)));
        assert_eq!(map_event(&E::SetPosition(MediaPosition(Duration::from_millis(1_500)))), Some(Action::SeekTo(1_500)));
        assert_eq!(map_event(&E::Raise), None);
        assert_eq!(map_event(&E::Quit), None);
    }

    #[test]
    fn relative_seeks_are_clamped() {
        let s = playing(5_000, Some(8_000));
        assert!(matches!(to_command(&Action::SeekBy(-10_000), &s), Command::Seek(0)));
        assert!(matches!(to_command(&Action::SeekBy(10_000), &s), Command::Seek(8_000)));
        assert!(matches!(to_command(&Action::SeekBy(1_000), &playing(5_000, None)), Command::Seek(6_000)));
        assert!(matches!(to_command(&Action::Toggle, &s), Command::Toggle));
    }

    #[test]
    fn now_playing_uses_track_metadata_and_cover_lookup() {
        let np = now_playing(&playing(0, Some(8_000)), |id| (id == 7).then(|| PathBuf::from("/art/x-300.webp"))).unwrap();
        assert_eq!((np.title.as_str(), np.artist.as_deref(), np.album.as_deref(), np.duration_ms), ("Song", Some("Alpha"), Some("Light"), Some(8_000)));
        assert_eq!(np.cover, Some(PathBuf::from("/art/x-300.webp")));
        assert!(now_playing(&PlayerState::default(), |_| None).is_none());
    }

    #[test]
    fn playback_status_mapping() {
        assert!(matches!(playback(&PlayerState::default()), MediaPlayback::Stopped));
        assert!(matches!(playback(&playing(1_000, None)), MediaPlayback::Playing { progress: Some(MediaPosition(d)) } if d == Duration::from_secs(1)));
    }

    #[test]
    fn cover_urls_are_file_urls() {
        #[cfg(windows)]
        assert_eq!(file_url(Path::new(r"C:\Art Cache\ab\x-300.webp")), "file:///C:/Art%20Cache/ab/x-300.webp");
        #[cfg(not(windows))]
        assert_eq!(file_url(Path::new("/art cache/ab/x-300.webp")), "file:///art%20cache/ab/x-300.webp");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --bin aurora-engine osmedia`
Expected: compile errors (`map_event`, … not found).

- [ ] **Step 4: Implement**

Put this above the tests in `osmedia/mod.rs`:

```rust
//! OS media integration (spec §3.10) through `souvlaki`: Windows SMTC, Linux MPRIS, macOS
//! MPNowPlayingInfoCenter. Pure mapping functions plus one thread that owns the controls.
#[cfg(windows)]
mod win_window;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use engine_core::player::{Command, Commander, StateReader};
use protocol::{PlaybackStatus, PlayerState};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

pub const SEEK_STEP_MS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Action { Play, Pause, Toggle, Next, Prev, Stop, SeekTo(u64), SeekBy(i64) }

pub fn map_event(e: &MediaControlEvent) -> Option<Action> {
    let signed = |dir: &SeekDirection, ms: i64| if matches!(dir, SeekDirection::Forward) { ms } else { -ms };
    Some(match e {
        MediaControlEvent::Play => Action::Play,
        MediaControlEvent::Pause => Action::Pause,
        MediaControlEvent::Toggle => Action::Toggle,
        MediaControlEvent::Next => Action::Next,
        MediaControlEvent::Previous => Action::Prev,
        MediaControlEvent::Stop => Action::Stop,
        MediaControlEvent::Seek(dir) => Action::SeekBy(signed(dir, SEEK_STEP_MS)),
        MediaControlEvent::SeekBy(dir, d) => Action::SeekBy(signed(dir, d.as_millis() as i64)),
        MediaControlEvent::SetPosition(MediaPosition(d)) => Action::SeekTo(d.as_millis() as u64),
        _ => return None,
    })
}

pub fn to_command(a: &Action, state: &PlayerState) -> Command {
    match a {
        Action::Play => Command::Play(None),
        Action::Pause => Command::Pause,
        Action::Toggle => Command::Toggle,
        Action::Next => Command::Next,
        Action::Prev => Command::Prev,
        Action::Stop => Command::Stop,
        Action::SeekTo(ms) => Command::Seek(*ms),
        Action::SeekBy(d) => {
            let target = (state.position_ms as i64 + d).max(0) as u64;
            Command::Seek(state.duration_ms.map_or(target, |dur| target.min(dur)))
        }
    }
}

/// `file://` URL for a local file, percent-encoding everything but unreserved characters and `/`.
pub fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let enc: String = s.bytes().map(|b| {
        if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }
    }).collect();
    if enc.starts_with('/') { format!("file://{enc}") } else { format!("file:///{enc}") }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying { pub title: String, pub artist: Option<String>, pub album: Option<String>, pub cover: Option<PathBuf>, pub duration_ms: Option<u64> }

pub fn now_playing(state: &PlayerState, cover: impl Fn(i64) -> Option<PathBuf>) -> Option<NowPlaying> {
    let t = state.track.as_ref()?;
    Some(NowPlaying {
        title: t.title.clone(), artist: t.artist.clone(), album: t.album.clone(),
        cover: t.artwork_id.and_then(cover), duration_ms: state.duration_ms.or(t.duration_ms),
    })
}

pub fn playback(state: &PlayerState) -> MediaPlayback {
    let progress = Some(MediaPosition(Duration::from_millis(state.position_ms)));
    match (state.status, state.track.is_some()) {
        (PlaybackStatus::Playing, true) => MediaPlayback::Playing { progress },
        (PlaybackStatus::Paused, true) => MediaPlayback::Paused { progress },
        _ => MediaPlayback::Stopped,
    }
}

pub fn enabled() -> bool { std::env::var("AURORA_OSMEDIA").as_deref() != Ok("0") }

/// Owns the OS media controls on a dedicated thread: OS events become player commands; the latest
/// state from `states` updates what the OS shows. Exits quietly when `states` closes.
pub fn spawn(states: Receiver<PlayerState>, commander: Commander, reader: StateReader, cover: impl Fn(i64) -> Option<PathBuf> + Send + 'static) -> Option<JoinHandle<()>> {
    std::thread::Builder::new().name("aurora-osmedia".into()).spawn(move || {
        #[cfg(windows)]
        let window = match win_window::HiddenWindow::new() {
            Ok(w) => w,
            Err(e) => { tracing::warn!(error = %e, "media keys unavailable: no window for SMTC"); return; }
        };
        #[cfg(windows)]
        let hwnd = Some(window.hwnd());
        #[cfg(not(windows))]
        let hwnd = None;
        let mut controls = match MediaControls::new(PlatformConfig { display_name: "Aurora", dbus_name: "aurora", hwnd }) {
            Ok(c) => c,
            Err(e) => { tracing::warn!(error = ?e, "media keys unavailable"); return; }
        };
        let (cmd, rd) = (commander.clone(), reader.clone());
        if let Err(e) = controls.attach(move |ev: MediaControlEvent| {
            if let Some(a) = map_event(&ev) { cmd.send(to_command(&a, &rd.get())); }
        }) {
            tracing::warn!(error = ?e, "media keys unavailable");
            return;
        }
        let mut shown: Option<NowPlaying> = None;
        let mut status: Option<(PlaybackStatus, bool)> = None;
        loop {
            match states.recv_timeout(Duration::from_millis(50)) {
                Ok(first) => {
                    let s = states.try_iter().last().unwrap_or(first);
                    let np = now_playing(&s, &cover);
                    let track_changed = np != shown;
                    if track_changed && let Some(n) = &np {
                        let url = n.cover.as_deref().map(file_url);
                        let _ = controls.set_metadata(MediaMetadata {
                            title: Some(&n.title), artist: n.artist.as_deref(), album: n.album.as_deref(),
                            cover_url: url.as_deref(), duration: n.duration_ms.map(Duration::from_millis),
                            ..Default::default()
                        });
                    }
                    shown = np;
                    let now = (s.status, s.track.is_some());
                    if track_changed || status != Some(now) {
                        let _ = controls.set_playback(playback(&s));
                        status = Some(now);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            #[cfg(windows)]
            win_window::pump();
        }
    }).ok()
}
```

Create `engine/crates/aurora-engine/src/osmedia/win_window.rs`:

```rust
//! A hidden window owned by the engine process, which SMTC needs (`GetForWindow`), plus the message
//! pump that delivers its media-button events.
use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassExW, TranslateMessage, MSG,
    PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
};

pub struct HiddenWindow { hwnd: HWND }

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

impl HiddenWindow {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = w!("AuroraMediaWindow");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                hInstance: instance.into(),
                lpszClassName: class,
                lpfnWndProc: Some(wnd_proc),
                ..Default::default()
            };
            if RegisterClassExW(&wc) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE::default(), class, w!("Aurora"), WINDOW_STYLE::default(), 0, 0, 0, 0, None, None, Some(instance.into()), None)
                .map_err(|e| e.to_string())?;
            Ok(HiddenWindow { hwnd })
        }
    }

    pub fn hwnd(&self) -> *mut c_void { self.hwnd.0 }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.hwnd); } }
}

/// Drains this thread's message queue (souvlaki's SMTC events arrive as window messages).
pub fn pump() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
```

The `windows` 0.62 signatures used here are:
- `CreateWindowExW(.., hwndparent: Option<HWND>, hmenu: Option<HMENU>, hinstance: Option<HINSTANCE>, lpparam: Option<*const c_void>) -> Result<HWND>`;
- `HWND(pub *mut c_void)`.

If a signature differs in the resolved version, follow the compiler but keep the behaviour: a plain invisible top-level window (not message-only, which SMTC rejects) and a non-blocking `PeekMessageW` pump.

- [ ] **Step 5: Wire it into `run()`**

In `src/rpc/mod.rs`:

(a) Before spawning the event thread, create the state channel and start the media thread:

```rust
    let os_tx = crate::osmedia::enabled().then(|| {
        let (tx, rx) = crossbeam_channel::unbounded::<protocol::PlayerState>();
        let lib = library.clone();
        let cover = move |art: i64| lib.as_ref().and_then(|s| s.lib.artwork_info(art, 300).ok()).map(|i| PathBuf::from(i.path));
        crate::osmedia::spawn(rx, player.commander(), player.state_reader(), cover);
        tx
    });
```

(b) Move `os_tx` into the event thread. Change the `State` arm to forward a copy:

```rust
                PlayerEvent::State(s) => {
                    if let Some(tx) = &os_tx { let _ = tx.send(s.clone()); }
                    out2.notify("player.state", s)
                }
```

When the player shuts down, the event thread ends and drops `os_tx`, so the media thread sees `Disconnected` and exits.

- [ ] **Step 6: Run the tests**

Run: `cargo test --manifest-path engine/Cargo.toml --workspace`
Expected: the 5 osmedia unit tests pass. All integration tests set `AURORA_OSMEDIA=0`, so CI never touches D-Bus or SMTC.

Then run: `cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Check it by hand (Windows dev machine)**

Run `npm run engine:build`, then start the engine with a real device and a queue. The `sleep` keeps stdin open, because the engine exits on EOF:

```bash
(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"queue.setPaths","params":{"paths":["fixtures/out/flac24_96k.flac"],"startIndex":0}}' '{"jsonrpc":"2.0","id":2,"method":"player.play"}'; sleep 30) | engine/target/debug/aurora-engine rpc
```

While it plays, press the keyboard's play/pause media key. Check two things:
- playback pauses;
- the Windows media flyout shows "flac24_96k".

Record the result (or "no media keys on this machine") in the task report. Linux MPRIS and macOS are verified in the M4 manual matrix.

- [ ] **Step 8: Commit**

```bash
git add engine
git commit -m "feat(engine): OS media keys and now-playing via souvlaki (SMTC, MPRIS, Now Playing)"
```

### Task 16: End-to-end engine tests — queue editing, session restart, history, playlists, watcher

**Files:**
- Modify: `engine/crates/aurora-engine/tests/library_rpc.rs` (append tests)

**Interfaces:**
- Consumes: everything from Tasks 12–15, through the real binary and the `tests/common` harness from Task 13.
- Produces: no new code. This task is the spec §7 "protocol integration tests" for M2, and it pins Review Focus item 5 (`restored_queue_skips_missing_file`).

These tests drive the real `aurora-engine` binary over stdio, using `AURORA_OUTPUT=null`, `AURORA_NULL_SPEED=10` and `AURORA_OSMEDIA=0`. Every library is built from copies of generated fixtures, so file stems are the titles (`short_a`, …).

- [ ] **Step 1: Write the tests**

Append to `engine/crates/aurora-engine/tests/library_rpc.rs`:

```rust
fn scanned_engine(data: &std::path::Path, music: &std::path::Path) -> (Engine, Vec<i64>) {
    let mut e = with_library(data);
    e.ok("library.addFolder", json!({ "path": music }));
    wait_scan_done(&mut e);
    let tracks = e.ok("library.tracks", json!({ "sort": "title" }));
    let ids = tracks["items"].as_array().unwrap().iter().map(|t| t["id"].as_i64().unwrap()).collect();
    (e, ids)
}

fn titles(q: &Value) -> Vec<String> { q["items"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap().to_string()).collect() }

#[test]
fn queue_edits_over_rpc() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["gap_a.flac", "short_a.flac", "short_b.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path()); // title order: gap_a, short_a, short_b
    e.ok("queue.set", json!({ "trackIds": [ids[0]] }));
    e.ok("queue.add", json!({ "trackIds": [ids[1]] }));
    e.ok("queue.insertNext", json!({ "trackIds": [ids[2]] }));
    let q = e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.len() == 3));
    assert_eq!(titles(&q), ["gap_a", "short_b", "short_a"]);
    e.ok("queue.move", json!({ "from": 2, "to": 1 }));
    let q = e.notified("queue.changed", |p| p["items"][1]["title"] == "short_a");
    let remove = q["items"][2]["id"].clone();
    e.ok("queue.remove", json!({ "itemIds": [remove] }));
    e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.len() == 2));
    e.ok("queue.setRepeat", json!({ "repeat": "all" }));
    e.notified("player.state", |p| p["repeat"] == "all");
    e.ok("queue.setShuffle", json!({ "shuffle": true }));
    e.notified("player.state", |p| p["shuffle"] == true);
    e.ok("queue.clear", json!({}));
    e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.is_empty()));
}

#[test]
fn session_restores_after_restart() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["gap_a.flac", "gap_b.flac", "short_a.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path()); // gap_a, gap_b, short_a
    e.ok("queue.set", json!({ "trackIds": ids, "startIndex": 1 }));
    e.ok("player.play", json!({}));
    e.notified("player.state", |p| p["status"] == "playing");
    e.ok("player.pause", json!({}));
    e.notified("player.state", |p| p["status"] == "paused");
    e.ok("player.seek", json!({ "ms": 1_200 }));
    e.notified("player.state", |p| p["status"] == "paused" && p["positionMs"] == 1_200);
    assert!(e.close().success());

    let mut e = with_library(data.path());
    e.notified("player.state", |p| p["status"] == "paused"); // the restore's own state event
    let st = e.ok("player.state", json!({}));
    assert_eq!(st["status"], "paused");
    assert_eq!(st["queueIndex"], 1);
    assert_eq!(st["positionMs"], 1_200);
    assert_eq!(st["track"]["trackId"], ids[1]);
    assert_eq!(st["track"]["title"], "gap_b");
    assert_eq!(e.ok("queue.get", json!({}))["items"].as_array().unwrap().len(), 3);
    e.ok("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert!(st["positionMs"].as_u64().unwrap() >= 1_200, "resumes where it left off: {st}");
}

#[test]
fn restored_queue_skips_missing_file() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac", "short_b.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    e.ok("queue.set", json!({ "trackIds": ids }));
    assert!(e.close().success());
    std::fs::remove_file(music.path().join("short_a.flac")).unwrap();

    let mut e = with_library(data.path());
    e.notified("player.state", |p| p["status"] == "paused");
    e.ok("player.play", json!({}));
    let err = e.notified("engine.error", |p| p["name"] == "FILE_MISSING");
    assert!(err["path"].as_str().unwrap().ends_with("short_a.flac"));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert_eq!(st["track"]["title"], "short_b");
}

#[test]
fn a_finished_track_is_recorded_in_history() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    e.ok("queue.set", json!({ "trackIds": ids }));
    e.ok("player.play", json!({}));
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "history"));
    let recent = e.ok("library.recent", json!({ "kind": "played" }));
    assert_eq!(recent["tracks"][0]["id"], ids[0]);
    assert_eq!(recent["tracks"][0]["playCount"], 1);
    assert_eq!(e.ok("library.recent", json!({ "kind": "mostPlayed" }))["tracks"].as_array().unwrap().len(), 1);
}

#[test]
fn playlists_over_rpc_round_trip_through_m3u() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac", "short_b.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    let p = e.ok("playlists.create", json!({ "name": "Mix" }));
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "playlists"));
    let id = p["id"].clone();
    e.ok("playlists.addTracks", json!({ "id": id, "trackIds": ids }));
    e.ok("playlists.reorder", json!({ "id": id, "from": 0, "to": 1 }));
    let detail = e.ok("playlists.get", json!({ "id": id }));
    assert_eq!(detail["items"][0]["track"]["title"], "short_b");
    let out = music.path().join("Mix.m3u8");
    e.ok("playlists.exportM3u", json!({ "id": id, "path": out }));
    let imported = e.ok("playlists.importM3u", json!({ "path": out }));
    assert_eq!(imported["skipped"], 0);
    assert_eq!(imported["playlist"]["trackCount"], 2);
    e.ok("playlists.delete", json!({ "id": id }));
    assert_eq!(e.ok("playlists.list", json!({})).as_array().unwrap().len(), 1, "only the imported playlist remains");
}

#[test]
fn watcher_picks_up_new_files_and_remove_folder_forgets_them() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac"]);
    let (mut e, _) = scanned_engine(data.path(), music.path());
    e.seen.clear();
    std::fs::copy(fixture("short_b.flac"), music.path().join("short_b.flac")).unwrap();
    let done = e.notified("library.scanProgress", |p| p["phase"] == "done" && p["added"] == 1);
    assert_eq!(done["added"], 1);
    assert_eq!(e.ok("library.tracks", json!({}))["total"], 2);
    let folder = e.ok("library.folders", json!({}))[0]["id"].clone();
    e.ok("library.removeFolder", json!({ "id": folder }));
    assert_eq!(e.ok("library.tracks", json!({}))["total"], 0);
    assert!(e.ok("library.folders", json!({})).as_array().unwrap().is_empty());
}

#[test]
fn track_detail_includes_probed_gapless_info() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["gap_a.m4a"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    let d = e.ok("library.track", json!({ "id": ids[0] }));
    assert_eq!(d["track"]["codec"], "aac-lc");
    assert!(d["gapless"]["delay"].as_u64().unwrap() > 0, "{d}");
    assert!(d["sizeBytes"].as_u64().unwrap() > 0);
}

#[test]
fn unicode_folder_paths_work_end_to_end() {
    let data = tempfile::tempdir().unwrap();
    let music = tempfile::tempdir().unwrap();
    let dir = music.path().join("Beyoncé").join("日本語");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture("short_a.flac"), dir.join("♫ song.flac")).unwrap();
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    assert_eq!(e.ok("library.search", json!({ "text": "song" }))["tracks"][0]["id"], ids[0]);
    e.ok("queue.set", json!({ "trackIds": ids }));
    e.ok("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert!(st["track"]["path"].as_str().unwrap().contains("日本語"));
}
```

- [ ] **Step 2: Run them**

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --test library_rpc`
Expected: 12 tests pass (4 from Task 13 plus these 8).

These tests should pass against Tasks 12–15 as written. A failure here is an integration bug in those tasks: fix it at its cause (with a unit test in the owning crate when the bug is in `library` or `engine-core`), not by loosening the assertion.

`watcher_picks_up_new_files…` waits out the 2 s debounce. Its 30 s harness timeout is generous on purpose.

- [ ] **Step 3: Commit**

```bash
git add engine/crates/aurora-engine/tests/library_rpc.rs
git commit -m "test(engine): end-to-end library, queue, session, history, playlist and watcher tests"
```

### Task 17: Electron main — data dir, `aurora-art://` artwork, settings, IPC, graceful quit

**Files:**
- Create:
  - `app/src/main/engineArgs.ts`;
  - `app/src/main/artProtocol.ts`;
  - `app/src/main/settings.ts`;
  - `app/src/shared/settings.ts`;
  - tests `app/test/engineArgs.test.ts`, `app/test/artProtocol.test.ts`, `app/test/settings.test.ts`.
- Modify:
  - `app/package.json`: `"productName": "Aurora"`;
  - `app/src/shared/api.ts`;
  - `app/src/preload/index.ts`;
  - `app/src/main/index.ts`;
  - `app/src/renderer/index.html`: the CSP;
  - `app/test/api.test.ts`.

**Interfaces:**
- Consumes: the engine's `rpc --data-dir`, `library.artwork` and the method set from Task 13; generated protocol types (Task 1).
- Produces:
  - **`app/src/main/engineArgs.ts`:**
    - `resolveDataDir({ isPackaged, env, userData }) -> string`: `AURORA_DATA_DIR` is honoured only in unpackaged builds.
    - `engineArgs(dataDir) -> ['rpc', '--data-dir', dataDir]`.
  - **`app/src/main/artProtocol.ts`:**
    - `ART_SCHEME = 'aurora-art'`.
    - `parseArtUrl(url) -> { id, size } | null`, for `aurora-art://art/<id>/<size>`.
    - `createArtHandler(call, fetchFile) -> (req: Request) => Promise<Response>`: 404 on a bad URL or engine error; `Cache-Control: max-age=3600`.
  - **`app/src/shared/settings.ts`:**
    - `Settings { windowBounds: Bounds | null; sidebarExpanded: boolean; lastView: string }` and `Bounds { x: number; y: number; width: number; height: number; maximized: boolean }`.
    - `DEFAULT_SETTINGS`.
    - `mergeSettings(base: Settings, raw: unknown) -> Settings`, a pure validator. Unknown keys and wrong types are dropped. `lastView` must start with `/` and be ≤ 200 characters.
  - **`app/src/main/settings.ts`:**
    - `loadSettings(file) -> Settings`: a missing or corrupt file gives the defaults.
    - `SettingsStore { get(); update(patch: unknown): Settings; flush(): void }`: `update` saves debounced by 300 ms, atomically.
    - `boundsVisible(b: Bounds, workAreas: {x, y, width, height}[]) -> boolean`.
  - **`AuroraApi` (preload) gains:**
    - `openFolder(): Promise<string | null>`;
    - `pickM3u(): Promise<string | null>`;
    - `pickM3uSave(defaultName: string): Promise<string | null>`;
    - `showItemInFolder(path: string): Promise<void>`;
    - `getSettings(): Promise<Settings>`;
    - `setSettings(patch: Partial<Settings>): Promise<Settings>`.
  - **`Methods`:** gains every Task 13 method with typed params/results. **`Events`:** gains `library.scanProgress` and `library.changed`.
  - **`ALLOWED_METHODS`:** every key of `Methods`, enforced at compile time. `engine.shutdown` stays excluded.
  - **Test hooks** (unpackaged only): `AURORA_TEST_OPEN_FOLDER`, `AURORA_TEST_OPEN_M3U`, `AURORA_TEST_SAVE_M3U`, plus M1's `AURORA_TEST_OPEN_FILES`.
  - **Quitting:** before quit, the app flushes settings and awaits `supervisor.stop()`, so the engine writes `session.json`.

- [ ] **Step 1: Write the failing tests**

`app/test/engineArgs.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { engineArgs, resolveDataDir } from '../src/main/engineArgs'

describe('engine arguments', () => {
  it('passes the data dir to rpc mode', () => {
    expect(engineArgs('/u/Aurora')).toEqual(['rpc', '--data-dir', '/u/Aurora'])
  })
  it('honours AURORA_DATA_DIR only in unpackaged builds', () => {
    const env = { AURORA_DATA_DIR: '/tmp/test-data' }
    expect(resolveDataDir({ isPackaged: false, env, userData: '/u/Aurora' })).toBe('/tmp/test-data')
    expect(resolveDataDir({ isPackaged: true, env, userData: '/u/Aurora' })).toBe('/u/Aurora')
    expect(resolveDataDir({ isPackaged: false, env: {}, userData: '/u/Aurora' })).toBe('/u/Aurora')
  })
})
```

`app/test/artProtocol.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { createArtHandler, parseArtUrl } from '../src/main/artProtocol'

describe('aurora-art protocol', () => {
  it('parses art URLs strictly', () => {
    expect(parseArtUrl('aurora-art://art/12/300')).toEqual({ id: 12, size: 300 })
    expect(parseArtUrl('aurora-art://art/12/300?x=1')).toEqual({ id: 12, size: 300 })
    for (const bad of ['aurora-art://art/12', 'aurora-art://art/x/300', 'aurora-art://other/1/300', 'aurora-art://art/-1/64', 'aurora-art://art/1/0', 'https://art/1/64', 'nonsense'])
      expect(parseArtUrl(bad)).toBeNull()
  })

  it('serves the cached file the engine names', async () => {
    const call = vi.fn(async () => ({ path: '/data/artwork/ab/abc-300.webp', palette: null }))
    const fetchFile = vi.fn(async () => new Response('img', { headers: { 'content-type': 'image/webp' } }))
    const res = await createArtHandler(call, fetchFile)(new Request('aurora-art://art/7/200'))
    expect(call).toHaveBeenCalledWith('library.artwork', { id: 7, size: 200 })
    expect(fetchFile).toHaveBeenCalledWith('/data/artwork/ab/abc-300.webp')
    expect(res.status).toBe(200)
    expect(res.headers.get('cache-control')).toBe('max-age=3600')
    expect(await res.text()).toBe('img')
  })

  it('answers 404 for bad URLs and engine errors', async () => {
    const call = vi.fn(async () => { throw new Error('artwork 9 not found') })
    const handler = createArtHandler(call, vi.fn())
    expect((await handler(new Request('aurora-art://art/9/64'))).status).toBe(404)
    expect((await handler(new Request('aurora-art://art/nope/64'))).status).toBe(404)
    expect(call).toHaveBeenCalledTimes(1)
  })
})
```

`app/test/settings.test.ts`:

```ts
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { boundsVisible, loadSettings, SettingsStore } from '../src/main/settings'
import { DEFAULT_SETTINGS, mergeSettings } from '../src/shared/settings'

const dir = () => mkdtempSync(join(tmpdir(), 'aurora-settings-'))

describe('settings', () => {
  afterEach(() => vi.useRealTimers())

  it('merges only valid known fields', () => {
    const s = mergeSettings(DEFAULT_SETTINGS, { sidebarExpanded: true, lastView: '/albums', extra: 1, windowBounds: { x: 1, y: 2, width: 900, height: 700, maximized: false } })
    expect(s).toEqual({ sidebarExpanded: true, lastView: '/albums', windowBounds: { x: 1, y: 2, width: 900, height: 700, maximized: false } })
    expect(mergeSettings(DEFAULT_SETTINGS, { sidebarExpanded: 'yes', lastView: 'http://evil', windowBounds: { width: 'big' } })).toEqual(DEFAULT_SETTINGS)
    expect(mergeSettings(DEFAULT_SETTINGS, { lastView: '/' + 'a'.repeat(300) }).lastView).toBe('/')
    expect(mergeSettings(DEFAULT_SETTINGS, null)).toEqual(DEFAULT_SETTINGS)
  })

  it('loads defaults for missing or corrupt files', () => {
    const d = dir()
    expect(loadSettings(join(d, 'none.json'))).toEqual(DEFAULT_SETTINGS)
    writeFileSync(join(d, 'bad.json'), '{"sidebarExpanded": tr')
    expect(loadSettings(join(d, 'bad.json'))).toEqual(DEFAULT_SETTINGS)
  })

  it('saves updates after a debounce and on flush', () => {
    vi.useFakeTimers()
    const file = join(dir(), 'settings.json')
    const store = new SettingsStore(file)
    expect(store.update({ sidebarExpanded: true }).sidebarExpanded).toBe(true)
    expect(() => readFileSync(file)).toThrow()
    vi.advanceTimersByTime(300)
    expect(JSON.parse(readFileSync(file, 'utf8')).sidebarExpanded).toBe(true)
    store.update({ lastView: '/artists' })
    store.flush()
    expect(loadSettings(file).lastView).toBe('/artists')
  })

  it('only restores window bounds that are on a screen', () => {
    const screens = [{ x: 0, y: 0, width: 1920, height: 1040 }]
    expect(boundsVisible({ x: 100, y: 100, width: 1280, height: 800, maximized: false }, screens)).toBe(true)
    expect(boundsVisible({ x: 5000, y: 100, width: 1280, height: 800, maximized: false }, screens)).toBe(false)
    expect(boundsVisible({ x: -1200, y: 0, width: 1280, height: 800, maximized: false }, screens)).toBe(true) // 80 px still visible
  })
})
```

Replace the first test in `app/test/api.test.ts` with:

```ts
  it('allows every typed engine method except shutdown', () => {
    for (const m of ['player.play', 'queue.set', 'queue.insertNext', 'queue.setRepeat', 'library.albums', 'library.search', 'library.addFolder', 'playlists.reorder', 'playlists.importM3u'])
      expect(isAllowedMethod(m)).toBe(true)
    expect(isAllowedMethod('engine.shutdown')).toBe(false)
    expect(isAllowedMethod('library.nothing')).toBe(false)
    expect(isAllowedMethod({})).toBe(false)
    expect(new Set(ALLOWED_METHODS).size).toBe(ALLOWED_METHODS.length)
  })
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm --prefix app test`
Expected: the new test files fail to import (modules don't exist). The api test fails on `queue.set`.

- [ ] **Step 3: Implement the pure modules**

`app/src/main/engineArgs.ts`:

```ts
/** Where the engine keeps library.db, artwork/ and session.json. Tests may redirect it (unpackaged only). */
export function resolveDataDir(o: { isPackaged: boolean; env: NodeJS.ProcessEnv | Record<string, string | undefined>; userData: string }): string {
  return (!o.isPackaged && o.env.AURORA_DATA_DIR) || o.userData
}

export function engineArgs(dataDir: string): string[] {
  return ['rpc', '--data-dir', dataDir]
}
```

`app/src/main/artProtocol.ts`:

```ts
import type { ArtworkInfo } from '@protocol/ArtworkInfo'

export const ART_SCHEME = 'aurora-art'

/** `aurora-art://art/<id>/<size>` → ids and sizes are positive integers. */
export function parseArtUrl(url: string): { id: number; size: number } | null {
  let u: URL
  try { u = new URL(url) } catch { return null }
  if (u.protocol !== `${ART_SCHEME}:` || u.hostname !== 'art') return null
  const m = /^\/(\d+)\/(\d+)$/.exec(u.pathname)
  if (!m) return null
  const id = Number(m[1]), size = Number(m[2])
  return id > 0 && size > 0 && size <= 4096 ? { id, size } : null
}

type Call = (method: 'library.artwork', params: { id: number; size: number }) => Promise<ArtworkInfo>

/** Resolves artwork through the engine (which owns the cache) and streams the file. */
export function createArtHandler(call: Call, fetchFile: (path: string) => Promise<Response>) {
  return async (req: Request): Promise<Response> => {
    const parsed = parseArtUrl(req.url)
    if (!parsed) return new Response(null, { status: 404 })
    try {
      const info = await call('library.artwork', parsed)
      const file = await fetchFile(info.path)
      const headers = new Headers(file.headers)
      headers.set('cache-control', 'max-age=3600')
      return new Response(file.body, { status: file.status, headers })
    } catch {
      return new Response(null, { status: 404 })
    }
  }
}
```

`app/src/shared/settings.ts`:

```ts
/** UI preferences owned by Electron (spec §5.2). */
export interface Bounds { x: number; y: number; width: number; height: number; maximized: boolean }
export interface Settings { windowBounds: Bounds | null; sidebarExpanded: boolean; lastView: string }

export const DEFAULT_SETTINGS: Settings = { windowBounds: null, sidebarExpanded: false, lastView: '/' }

const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

function bounds(v: unknown): Bounds | null | undefined {
  if (v === null) return null
  if (typeof v !== 'object' || v === null) return undefined
  const b = v as Record<string, unknown>
  if (![b.x, b.y, b.width, b.height].every(num) || (b.width as number) < 200 || (b.height as number) < 200) return undefined
  return { x: b.x as number, y: b.y as number, width: b.width as number, height: b.height as number, maximized: b.maximized === true }
}

/** `base` with every valid field of `raw` applied; anything unknown or malformed is ignored. */
export function mergeSettings(base: Settings, raw: unknown): Settings {
  const out: Settings = { ...base }
  if (typeof raw !== 'object' || raw === null) return out
  const r = raw as Record<string, unknown>
  if (typeof r.sidebarExpanded === 'boolean') out.sidebarExpanded = r.sidebarExpanded
  if (typeof r.lastView === 'string' && r.lastView.startsWith('/')) out.lastView = r.lastView.length <= 200 ? r.lastView : '/'
  if ('windowBounds' in r) { const b = bounds(r.windowBounds); if (b !== undefined) out.windowBounds = b }
  return out
}
```

`app/src/main/settings.ts`:**
    - `loadSettings(file) -> Settings`: a missing or corrupt file gives the defaults.
    - `SettingsStore { get(); update(patch: unknown): Settings; flush(): void }`: `update` saves debounced by 300 ms, atomically.
    - `boundsVisible(b: Bounds, workAreas: {x, y, width, height}[]) -> boolean`.
  - **`AuroraApi` (preload) gains:**
    - `openFolder(): Promise<string | null>`;
    - `pickM3u(): Promise<string | null>`;
    - `pickM3uSave(defaultName: string): Promise<string | null>`;
    - `showItemInFolder(path: string): Promise<void>`;
    - `getSettings(): Promise<Settings>`;
    - `setSettings(patch: Partial<Settings>): Promise<Settings>`.
  - **`Methods`:** gains every Task 13 method with typed params/results. **`Events`:** gains `library.scanProgress` and `library.changed`.
  - **`ALLOWED_METHODS`:** every key of `Methods`, enforced at compile time. `engine.shutdown` stays excluded.
  - **Test hooks** (unpackaged only): `AURORA_TEST_OPEN_FOLDER`, `AURORA_TEST_OPEN_M3U`, `AURORA_TEST_SAVE_M3U`, plus M1's `AURORA_TEST_OPEN_FILES`.
  - **Quitting:** before quit, the app flushes settings and awaits `supervisor.stop()`, so the engine writes `session.json`.

- [ ] **Step 1: Write the failing tests**

`app/test/engineArgs.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { engineArgs, resolveDataDir } from '../src/main/engineArgs'

describe('engine arguments', () => {
  it('passes the data dir to rpc mode', () => {
    expect(engineArgs('/u/Aurora')).toEqual(['rpc', '--data-dir', '/u/Aurora'])
  })
  it('honours AURORA_DATA_DIR only in unpackaged builds', () => {
    const env = { AURORA_DATA_DIR: '/tmp/test-data' }
    expect(resolveDataDir({ isPackaged: false, env, userData: '/u/Aurora' })).toBe('/tmp/test-data')
    expect(resolveDataDir({ isPackaged: true, env, userData: '/u/Aurora' })).toBe('/u/Aurora')
    expect(resolveDataDir({ isPackaged: false, env: {}, userData: '/u/Aurora' })).toBe('/u/Aurora')
  })
})
```

`app/test/artProtocol.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { createArtHandler, parseArtUrl } from '../src/main/artProtocol'

describe('aurora-art protocol', () => {
  it('parses art URLs strictly', () => {
    expect(parseArtUrl('aurora-art://art/12/300')).toEqual({ id: 12, size: 300 })
    expect(parseArtUrl('aurora-art://art/12/300?x=1')).toEqual({ id: 12, size: 300 })
    for (const bad of ['aurora-art://art/12', 'aurora-art://art/x/300', 'aurora-art://other/1/300', 'aurora-art://art/-1/64', 'aurora-art://art/1/0', 'https://art/1/64', 'nonsense'])
      expect(parseArtUrl(bad)).toBeNull()
  })

  it('serves the cached file the engine names', async () => {
    const call = vi.fn(async () => ({ path: '/data/artwork/ab/abc-300.webp', palette: null }))
    const fetchFile = vi.fn(async () => new Response('img', { headers: { 'content-type': 'image/webp' } }))
    const res = await createArtHandler(call, fetchFile)(new Request('aurora-art://art/7/200'))
    expect(call).toHaveBeenCalledWith('library.artwork', { id: 7, size: 200 })
    expect(fetchFile).toHaveBeenCalledWith('/data/artwork/ab/abc-300.webp')
    expect(res.status).toBe(200)
    expect(res.headers.get('cache-control')).toBe('max-age=3600')
    expect(await res.text()).toBe('img')
  })

  it('answers 404 for bad URLs and engine errors', async () => {
    const call = vi.fn(async () => { throw new Error('artwork 9 not found') })
    const handler = createArtHandler(call, vi.fn())
    expect((await handler(new Request('aurora-art://art/9/64'))).status).toBe(404)
    expect((await handler(new Request('aurora-art://art/nope/64'))).status).toBe(404)
    expect(call).toHaveBeenCalledTimes(1)
  })
})
```

`app/test/settings.test.ts`:

```ts
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { boundsVisible, loadSettings, SettingsStore } from '../src/main/settings'
import { DEFAULT_SETTINGS, mergeSettings } from '../src/shared/settings'

const dir = () => mkdtempSync(join(tmpdir(), 'aurora-settings-'))

describe('settings', () => {
  afterEach(() => vi.useRealTimers())

  it('merges only valid known fields', () => {
    const s = mergeSettings(DEFAULT_SETTINGS, { sidebarExpanded: true, lastView: '/albums', extra: 1, windowBounds: { x: 1, y: 2, width: 900, height: 700, maximized: false } })
    expect(s).toEqual({ sidebarExpanded: true, lastView: '/albums', windowBounds: { x: 1, y: 2, width: 900, height: 700, maximized: false } })
    expect(mergeSettings(DEFAULT_SETTINGS, { sidebarExpanded: 'yes', lastView: 'http://evil', windowBounds: { width: 'big' } })).toEqual(DEFAULT_SETTINGS)
    expect(mergeSettings(DEFAULT_SETTINGS, { lastView: '/' + 'a'.repeat(300) }).lastView).toBe('/')
    expect(mergeSettings(DEFAULT_SETTINGS, null)).toEqual(DEFAULT_SETTINGS)
  })

  it('loads defaults for missing or corrupt files', () => {
    const d = dir()
    expect(loadSettings(join(d, 'none.json'))).toEqual(DEFAULT_SETTINGS)
    writeFileSync(join(d, 'bad.json'), '{"sidebarExpanded": tr')
    expect(loadSettings(join(d, 'bad.json'))).toEqual(DEFAULT_SETTINGS)
  })

  it('saves updates after a debounce and on flush', () => {
    vi.useFakeTimers()
    const file = join(dir(), 'settings.json')
    const store = new SettingsStore(file)
    expect(store.update({ sidebarExpanded: true }).sidebarExpanded).toBe(true)
    expect(() => readFileSync(file)).toThrow()
    vi.advanceTimersByTime(300)
    expect(JSON.parse(readFileSync(file, 'utf8')).sidebarExpanded).toBe(true)
    store.update({ lastView: '/artists' })
    store.flush()
    expect(loadSettings(file).lastView).toBe('/artists')
  })

  it('only restores window bounds that are on a screen', () => {
    const screens = [{ x: 0, y: 0, width: 1920, height: 1040 }]
    expect(boundsVisible({ x: 100, y: 100, width: 1280, height: 800, maximized: false }, screens)).toBe(true)
    expect(boundsVisible({ x: 5000, y: 100, width: 1280, height: 800, maximized: false }, screens)).toBe(false)
    expect(boundsVisible({ x: -1200, y: 0, width: 1280, height: 800, maximized: false }, screens)).toBe(true) // 80 px still visible
  })
})
```

Replace the first test in `app/test/api.test.ts` with:

```ts
  it('allows every typed engine method except shutdown', () => {
    for (const m of ['player.play', 'queue.set', 'queue.insertNext', 'queue.setRepeat', 'library.albums', 'library.search', 'library.addFolder', 'playlists.reorder', 'playlists.importM3u'])
      expect(isAllowedMethod(m)).toBe(true)
    expect(isAllowedMethod('engine.shutdown')).toBe(false)
    expect(isAllowedMethod('library.nothing')).toBe(false)
    expect(isAllowedMethod({})).toBe(false)
    expect(new Set(ALLOWED_METHODS).size).toBe(ALLOWED_METHODS.length)
  })
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm --prefix app test`
Expected: the new test files fail to import (modules don't exist). The api test fails on `queue.set`.

- [ ] **Step 3: Implement the pure modules**

`app/src/main/engineArgs.ts`:

```ts
/** Where the engine keeps library.db, artwork/ and session.json. Tests may redirect it (unpackaged only). */
export function resolveDataDir(o: { isPackaged: boolean; env: NodeJS.ProcessEnv | Record<string, string | undefined>; userData: string }): string {
  return (!o.isPackaged && o.env.AURORA_DATA_DIR) || o.userData
}

export function engineArgs(dataDir: string): string[] {
  return ['rpc', '--data-dir', dataDir]
}
```

`app/src/main/artProtocol.ts`:

```ts
import type { ArtworkInfo } from '@protocol/ArtworkInfo'

export const ART_SCHEME = 'aurora-art'

/** `aurora-art://art/<id>/<size>` → ids and sizes are positive integers. */
export function parseArtUrl(url: string): { id: number; size: number } | null {
  let u: URL
  try { u = new URL(url) } catch { return null }
  if (u.protocol !== `${ART_SCHEME}:` || u.hostname !== 'art') return null
  const m = /^\/(\d+)\/(\d+)$/.exec(u.pathname)
  if (!m) return null
  const id = Number(m[1]), size = Number(m[2])
  return id > 0 && size > 0 && size <= 4096 ? { id, size } : null
}

type Call = (method: 'library.artwork', params: { id: number; size: number }) => Promise<ArtworkInfo>

/** Resolves artwork through the engine (which owns the cache) and streams the file. */
export function createArtHandler(call: Call, fetchFile: (path: string) => Promise<Response>) {
  return async (req: Request): Promise<Response> => {
    const parsed = parseArtUrl(req.url)
    if (!parsed) return new Response(null, { status: 404 })
    try {
      const info = await call('library.artwork', parsed)
      const file = await fetchFile(info.path)
      const headers = new Headers(file.headers)
      headers.set('cache-control', 'max-age=3600')
      return new Response(file.body, { status: file.status, headers })
    } catch {
      return new Response(null, { status: 404 })
    }
  }
}
```

`app/src/shared/settings.ts`:

```ts
/** UI preferences owned by Electron (spec §5.2). */
export interface Bounds { x: number; y: number; width: number; height: number; maximized: boolean }
export interface Settings { windowBounds: Bounds | null; sidebarExpanded: boolean; lastView: string }

export const DEFAULT_SETTINGS: Settings = { windowBounds: null, sidebarExpanded: false, lastView: '/' }

const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

function bounds(v: unknown): Bounds | null | undefined {
  if (v === null) return null
  if (typeof v !== 'object' || v === null) return undefined
  const b = v as Record<string, unknown>
  if (![b.x, b.y, b.width, b.height].every(num) || (b.width as number) < 200 || (b.height as number) < 200) return undefined
  return { x: b.x as number, y: b.y as number, width: b.width as number, height: b.height as number, maximized: b.maximized === true }
}

/** `base` with every valid field of `raw` applied; anything unknown or malformed is ignored. */
export function mergeSettings(base: Settings, raw: unknown): Settings {
  const out: Settings = { ...base }
  if (typeof raw !== 'object' || raw === null) return out
  const r = raw as Record<string, unknown>
  if (typeof r.sidebarExpanded === 'boolean') out.sidebarExpanded = r.sidebarExpanded
  if (typeof r.lastView === 'string' && r.lastView.startsWith('/')) out.lastView = r.lastView.length <= 200 ? r.lastView : '/'
  if ('windowBounds' in r) { const b = bounds(r.windowBounds); if (b !== undefined) out.windowBounds = b }
  return out
}
```

The `lastView` rule simplifies to: a valid value is taken; an invalid one becomes `'/'` only when it was too long, and is ignored otherwise. Write it as exactly this:

```ts
  if (typeof r.lastView === 'string' && r.lastView.startsWith('/')) out.lastView = r.lastView.length <= 200 ? r.lastView : '/'
```

`app/src/main/settings.ts`:

```ts
import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import { DEFAULT_SETTINGS, mergeSettings, type Bounds, type Settings } from '@shared/settings'

export function loadSettings(file: string): Settings {
  try { return mergeSettings(DEFAULT_SETTINGS, JSON.parse(readFileSync(file, 'utf8'))) } catch { return { ...DEFAULT_SETTINGS } }
}

export class SettingsStore {
  private current: Settings
  private timer: ReturnType<typeof setTimeout> | null = null
  constructor(private readonly file: string) { this.current = loadSettings(file) }
  get(): Settings { return this.current }
  update(patch: unknown): Settings {
    this.current = mergeSettings(this.current, patch)
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => this.flush(), 300)
    return this.current
  }
  flush(): void {
    if (this.timer) { clearTimeout(this.timer); this.timer = null }
    try {
      mkdirSync(dirname(this.file), { recursive: true })
      const tmp = `${this.file}.tmp`
      writeFileSync(tmp, JSON.stringify(this.current, null, 2))
      renameSync(tmp, this.file)
    } catch { /* preferences are best-effort */ }
  }
}

/** True when at least an 80×80 px piece of `b` lies on one of the screens' work areas. */
export function boundsVisible(b: Bounds, workAreas: { x: number; y: number; width: number; height: number }[]): boolean {
  return workAreas.some((a) => {
    const w = Math.min(b.x + b.width, a.x + a.width) - Math.max(b.x, a.x)
    const h = Math.min(b.y + b.height, a.y + a.height) - Math.max(b.y, a.y)
    return w >= 80 && h >= 80
  })
}
```

- [ ] **Step 4: Expand the shared API**

Replace `app/src/shared/api.ts` with:

```ts
import type { AlbumDetail } from '@protocol/AlbumDetail'
import type { AlbumRow } from '@protocol/AlbumRow'
import type { ArtistDetail } from '@protocol/ArtistDetail'
import type { ArtistRow } from '@protocol/ArtistRow'
import type { ArtworkInfo } from '@protocol/ArtworkInfo'
import type { ArtworkParams } from '@protocol/ArtworkParams'
import type { DeviceInfo } from '@protocol/DeviceInfo'
import type { DeviceParams } from '@protocol/DeviceParams'
import type { EngineErrorEvent } from '@protocol/EngineErrorEvent'
import type { ExportParams } from '@protocol/ExportParams'
import type { Folder } from '@protocol/Folder'
import type { GenreRow } from '@protocol/GenreRow'
import type { IdParams } from '@protocol/IdParams'
import type { ImportResult } from '@protocol/ImportResult'
import type { LibraryChanged } from '@protocol/LibraryChanged'
import type { ListQuery } from '@protocol/ListQuery'
import type { ModeParams } from '@protocol/ModeParams'
import type { NameParams } from '@protocol/NameParams'
import type { Page } from '@protocol/Page'
import type { PathParams } from '@protocol/PathParams'
import type { PlayParams } from '@protocol/PlayParams'
import type { PlayerState } from '@protocol/PlayerState'
import type { Playlist } from '@protocol/Playlist'
import type { PlaylistAddParams } from '@protocol/PlaylistAddParams'
import type { PlaylistDetail } from '@protocol/PlaylistDetail'
import type { PlaylistRemoveParams } from '@protocol/PlaylistRemoveParams'
import type { PlaylistReorderParams } from '@protocol/PlaylistReorderParams'
import type { QueueChanged } from '@protocol/QueueChanged'
import type { QueueMoveParams } from '@protocol/QueueMoveParams'
import type { QueueRemoveParams } from '@protocol/QueueRemoveParams'
import type { QueueSetParams } from '@protocol/QueueSetParams'
import type { QueueSetPathsParams } from '@protocol/QueueSetPathsParams'
import type { RecentParams } from '@protocol/RecentParams'
import type { RecentResult } from '@protocol/RecentResult'
import type { RenameParams } from '@protocol/RenameParams'
import type { RepeatParams } from '@protocol/RepeatParams'
import type { RescanParams } from '@protocol/RescanParams'
import type { ScanProgress } from '@protocol/ScanProgress'
import type { SearchParams } from '@protocol/SearchParams'
import type { SearchResults } from '@protocol/SearchResults'
import type { SeekParams } from '@protocol/SeekParams'
import type { ShuffleParams } from '@protocol/ShuffleParams'
import type { SignalPath } from '@protocol/SignalPath'
import type { TrackDetail } from '@protocol/TrackDetail'
import type { TrackIdsParams } from '@protocol/TrackIdsParams'
import type { TrackRow } from '@protocol/TrackRow'
import type { VolumeParams } from '@protocol/VolumeParams'
import type { Settings } from './settings'

export type EngineStatus = { state: 'starting' | 'ready' | 'restarting' | 'failed' | 'mismatch'; message?: string }
type None = Record<string, never>
type M<P, R> = { params: P; result: R }

export type Methods = {
  'player.play': M<PlayParams, null>
  'player.pause': M<None, null>
  'player.toggle': M<None, null>
  'player.stop': M<None, null>
  'player.next': M<None, null>
  'player.prev': M<None, null>
  'player.seek': M<SeekParams, null>
  'player.setVolume': M<VolumeParams, null>
  'player.setMode': M<ModeParams, null>
  'player.setDevice': M<DeviceParams, null>
  'player.state': M<None, PlayerState>
  'queue.setPaths': M<QueueSetPathsParams, null>
  'queue.set': M<QueueSetParams, null>
  'queue.add': M<TrackIdsParams, null>
  'queue.insertNext': M<TrackIdsParams, null>
  'queue.move': M<QueueMoveParams, null>
  'queue.remove': M<QueueRemoveParams, null>
  'queue.clear': M<None, null>
  'queue.setShuffle': M<ShuffleParams, null>
  'queue.setRepeat': M<RepeatParams, null>
  'queue.get': M<None, QueueChanged>
  'devices.list': M<None, DeviceInfo[]>
  'library.folders': M<None, Folder[]>
  'library.addFolder': M<PathParams, Folder>
  'library.removeFolder': M<IdParams, null>
  'library.rescan': M<RescanParams, null>
  'library.albums': M<ListQuery, Page<AlbumRow>>
  'library.artists': M<ListQuery, Page<ArtistRow>>
  'library.genres': M<ListQuery, Page<GenreRow>>
  'library.tracks': M<ListQuery, Page<TrackRow>>
  'library.album': M<IdParams, AlbumDetail>
  'library.artist': M<IdParams, ArtistDetail>
  'library.genre': M<IdParams, GenreRow>
  'library.track': M<IdParams, TrackDetail>
  'library.search': M<SearchParams, SearchResults>
  'library.artwork': M<ArtworkParams, ArtworkInfo>
  'library.recent': M<RecentParams, RecentResult>
  'playlists.list': M<None, Playlist[]>
  'playlists.get': M<IdParams, PlaylistDetail>
  'playlists.create': M<NameParams, Playlist>
  'playlists.rename': M<RenameParams, Playlist>
  'playlists.delete': M<IdParams, null>
  'playlists.addTracks': M<PlaylistAddParams, Playlist>
  'playlists.removeItems': M<PlaylistRemoveParams, Playlist>
  'playlists.reorder': M<PlaylistReorderParams, Playlist>
  'playlists.importM3u': M<PathParams, ImportResult>
  'playlists.exportM3u': M<ExportParams, null>
}

export type Events = {
  'player.state': PlayerState
  'player.signalPath': SignalPath
  'queue.changed': QueueChanged
  'devices.changed': DeviceInfo[]
  'engine.error': EngineErrorEvent
  'engine.status': EngineStatus
  'library.scanProgress': ScanProgress
  'library.changed': LibraryChanged
}

export const ALLOWED_METHODS = [
  'player.play', 'player.pause', 'player.toggle', 'player.stop', 'player.next', 'player.prev', 'player.seek',
  'player.setVolume', 'player.setMode', 'player.setDevice', 'player.state',
  'queue.setPaths', 'queue.set', 'queue.add', 'queue.insertNext', 'queue.move', 'queue.remove', 'queue.clear',
  'queue.setShuffle', 'queue.setRepeat', 'queue.get', 'devices.list',
  'library.folders', 'library.addFolder', 'library.removeFolder', 'library.rescan', 'library.albums', 'library.artists',
  'library.genres', 'library.tracks', 'library.album', 'library.artist', 'library.genre', 'library.track', 'library.search',
  'library.artwork', 'library.recent',
  'playlists.list', 'playlists.get', 'playlists.create', 'playlists.rename', 'playlists.delete', 'playlists.addTracks',
  'playlists.removeItems', 'playlists.reorder', 'playlists.importM3u', 'playlists.exportM3u',
] as const satisfies readonly (keyof Methods)[]

// Compile-time check that every typed method is allowed (and nothing else is).
type Unlisted = Exclude<keyof Methods, (typeof ALLOWED_METHODS)[number]>
const everyMethodAllowed: [Unlisted] extends [never] ? true : Unlisted = true
void everyMethodAllowed

export function isAllowedMethod(m: unknown): m is keyof Methods {
  return typeof m === 'string' && (ALLOWED_METHODS as readonly string[]).includes(m)
}

export const AUDIO_EXTENSIONS = ['mp3', 'm4a', 'm4b', 'mp4', 'aac', 'flac', 'wav', 'aif', 'aiff', 'aifc', 'ogg', 'oga', 'opus', 'dsf', 'dff'] as const

export interface AuroraApi {
  call<K extends keyof Methods>(method: K, params: Methods[K]['params']): Promise<Methods[K]['result']>
  on<E extends keyof Events>(event: E, cb: (payload: Events[E]) => void): () => void
  openFiles(): Promise<string[]>
  openFolder(): Promise<string | null>
  pickM3u(): Promise<string | null>
  pickM3uSave(defaultName: string): Promise<string | null>
  showItemInFolder(path: string): Promise<void>
  getSettings(): Promise<Settings>
  setSettings(patch: Partial<Settings>): Promise<Settings>
  pathsForFiles(files: File[]): string[]
  status(): Promise<EngineStatus>
  restartEngine(): Promise<void>
  openLogs(): Promise<void>
  platform: string
}
```

M1's "alac" pseudo-extension is dropped from `AUDIO_EXTENSIONS`. It was a deferred minor, and no file has that extension.

`app/src/preload/index.ts`: add these to `api`:

```ts
  openFolder: () => ipcRenderer.invoke('aurora:openFolder'),
  pickM3u: () => ipcRenderer.invoke('aurora:pickM3u'),
  pickM3uSave: (defaultName) => ipcRenderer.invoke('aurora:pickM3uSave', defaultName),
  showItemInFolder: (path) => ipcRenderer.invoke('aurora:showItemInFolder', path),
  getSettings: () => ipcRenderer.invoke('aurora:getSettings'),
  setSettings: (patch) => ipcRenderer.invoke('aurora:setSettings', patch),
```

- [ ] **Step 5: Wire the main process**

In `app/package.json`, add `"productName": "Aurora"` after `"name"`. It makes `userData` the spec's `%APPDATA%/Aurora` / `~/Library/Application Support/Aurora` / `~/.config/Aurora`.

In `app/src/renderer/index.html`, change `img-src 'self' data:` to `img-src 'self' data: aurora-art:`.

Replace `app/src/main/index.ts` with:

```ts
import { app, BrowserWindow, dialog, ipcMain, net, protocol, screen, shell } from 'electron'
import { spawn } from 'node:child_process'
import { delimiter, isAbsolute, join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { PROTOCOL_VERSION } from '@protocol/ProtocolVersion'
import { AUDIO_EXTENSIONS, isAllowedMethod } from '@shared/api'
import { ART_SCHEME, createArtHandler } from './artProtocol'
import { engineArgs, resolveDataDir } from './engineArgs'
import { enginePath } from './engine/enginePath'
import { EngineSupervisor } from './engine/supervisor'
import { logDir, logger } from './logs'
import { shouldBlockNavigation } from './navigation'
import { safeSend } from './safeSend'
import { boundsVisible, SettingsStore } from './settings'

protocol.registerSchemesAsPrivileged([{ scheme: ART_SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true } }])

let win: BrowserWindow | null = null
let settings: SettingsStore
let quitting = false

/** Test-only hooks; ignored in packaged builds. */
const testEnv = (name: string): string | undefined => (app.isPackaged ? undefined : process.env[name])

const binary = () => enginePath({ isPackaged: app.isPackaged, resourcesPath: process.resourcesPath, appPath: app.getAppPath(), platform: process.platform, arch: process.arch, env: process.env })
const dataDir = () => resolveDataDir({ isPackaged: app.isPackaged, env: process.env, userData: app.getPath('userData') })

const supervisor = new EngineSupervisor(() => spawn(binary(), engineArgs(dataDir()), { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }), { protocolVersion: PROTOCOL_VERSION })

function send(event: string, payload: unknown) {
  safeSend(win, 'aurora:event', event, payload)
}

function saveBounds() {
  if (!win || win.isDestroyed()) return
  const b = win.getNormalBounds()
  settings.update({ windowBounds: { ...b, maximized: win.isMaximized() } })
}

function createWindow() {
  const mac = process.platform === 'darwin'
  const saved = settings.get().windowBounds
  const visible = saved && boundsVisible(saved, screen.getAllDisplays().map((d) => d.workArea))
  win = new BrowserWindow({
    width: 1280, height: 800, ...(visible ? { x: saved.x, y: saved.y, width: saved.width, height: saved.height } : {}),
    minWidth: 940, minHeight: 600, show: false,
    backgroundColor: '#17151A',
    titleBarStyle: mac ? 'hiddenInset' : 'hidden',
    titleBarOverlay: mac ? undefined : { color: '#100E12', symbolColor: '#EFE9E1', height: 36 },
    webPreferences: { preload: join(__dirname, '../preload/index.js'), contextIsolation: true, nodeIntegration: false, sandbox: true },
  })
  if (visible && saved.maximized) win.maximize()
  win.once('ready-to-show', () => win?.show())
  win.on('close', saveBounds)
  win.on('closed', () => { win = null })
  const devUrl = process.env.ELECTRON_RENDERER_URL
  const page = join(__dirname, '../renderer/index.html')
  const appUrl = devUrl ?? pathToFileURL(page).href
  // The window only ever shows the app: no navigating away (e.g. to a dropped file), no new windows.
  win.webContents.on('will-navigate', (e) => { if (shouldBlockNavigation(e.url, appUrl)) e.preventDefault() })
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  if (devUrl) win.loadURL(devUrl)
  else win.loadFile(page)
}

app.whenReady().then(() => {
  settings = new SettingsStore(join(app.getPath('userData'), 'settings.json'))
  const engineLog = logger('engine.log')
  supervisor.on('log', engineLog)
  supervisor.on('notification', (m: string, p: unknown) => send(m, p))
  supervisor.on('status', (s) => { engineLog(`[status] ${JSON.stringify(s)}`); send('engine.status', s) })

  protocol.handle(ART_SCHEME, createArtHandler(
    (method, params) => supervisor.call(method, params) as never,
    (path) => net.fetch(pathToFileURL(path).toString()),
  ))

  ipcMain.handle('aurora:call', (_e, method: unknown, params: unknown) => {
    if (!isAllowedMethod(method)) throw new Error(`method not allowed: ${String(method)}`)
    return supervisor.call(method, params ?? {})
  })
  ipcMain.handle('aurora:openFiles', async () => {
    const test = testEnv('AURORA_TEST_OPEN_FILES')
    if (test) return test.split(delimiter).filter(Boolean)
    if (!win) return []
    const r = await dialog.showOpenDialog(win, { properties: ['openFile', 'multiSelections'], filters: [{ name: 'Audio', extensions: [...AUDIO_EXTENSIONS] }] })
    return r.canceled ? [] : r.filePaths
  })
  ipcMain.handle('aurora:openFolder', async () => {
    const test = testEnv('AURORA_TEST_OPEN_FOLDER')
    if (test) return test
    if (!win) return null
    const r = await dialog.showOpenDialog(win, { properties: ['openDirectory'] })
    return r.canceled ? null : r.filePaths[0] ?? null
  })
  ipcMain.handle('aurora:pickM3u', async () => {
    const test = testEnv('AURORA_TEST_OPEN_M3U')
    if (test) return test
    if (!win) return null
    const r = await dialog.showOpenDialog(win, { properties: ['openFile'], filters: [{ name: 'Playlists', extensions: ['m3u8', 'm3u'] }] })
    return r.canceled ? null : r.filePaths[0] ?? null
  })
  ipcMain.handle('aurora:pickM3uSave', async (_e, defaultName: unknown) => {
    const test = testEnv('AURORA_TEST_SAVE_M3U')
    if (test) return test
    if (!win) return null
    const name = typeof defaultName === 'string' ? defaultName.replace(/[\\/:*?"<>|]/g, '_') : 'Playlist'
    const r = await dialog.showSaveDialog(win, { defaultPath: `${name}.m3u8`, filters: [{ name: 'Playlists', extensions: ['m3u8'] }] })
    return r.canceled ? null : r.filePath ?? null
  })
  ipcMain.handle('aurora:showItemInFolder', (_e, path: unknown) => {
    if (typeof path === 'string' && isAbsolute(path)) shell.showItemInFolder(path)
  })
  ipcMain.handle('aurora:getSettings', () => settings.get())
  ipcMain.handle('aurora:setSettings', (_e, patch: unknown) => settings.update(patch))
  ipcMain.handle('aurora:status', () => supervisor.status)
  ipcMain.handle('aurora:restartEngine', () => supervisor.restart())
  ipcMain.handle('aurora:openLogs', async () => { await shell.openPath(logDir()) })

  supervisor.start()
  createWindow()
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow() })
})

// Quit only after the engine has shut down cleanly (it writes session.json on the way out).
app.on('before-quit', (e) => {
  if (quitting) return
  e.preventDefault()
  quitting = true
  saveBounds()
  settings?.flush()
  void supervisor.stop().finally(() => app.quit())
})
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
```

`createArtHandler` types its `call` narrowly. The `as never` cast bridges it to the supervisor's untyped `call` without loosening the handler's own types.

- [ ] **Step 6: Run the checks**

Run: `npm --prefix app run typecheck && npm --prefix app test`
Expected: typecheck clean, with every `@protocol/*` import resolving to a generated file. All tests pass, including the 3 new test files and the updated api test.

- [ ] **Step 7: Commit**

```bash
git add app
git commit -m "feat(app): data dir, aurora-art artwork protocol, settings store, library IPC and graceful quit"
```

### Task 18: Renderer foundation — router, layout, sidebar, library/UI stores, menus, toasts, dialogs

**Files:**
- Modify:
  - `app/package.json`: add the router, virtual and dnd-kit packages;
  - `app/vitest.config.ts`: setup file;
  - `app/src/renderer/main.tsx`;
  - `app/src/renderer/stores/player.ts`: remove `panel`/`setPanel`;
  - `app/src/renderer/components/PlayerBar.tsx`: the queue button navigates;
  - `app/test/PlayerBar.test.tsx`: render inside the router.
- Create:
  - core: `app/src/renderer/router.tsx`, `app/src/renderer/Layout.tsx`, `app/src/renderer/Layout.module.css`;
  - `lib/`: `strings.ts`, `art.ts`, `nav.ts`, `useEngineQuery.ts`;
  - `stores/`: `library.ts`, `ui.ts`;
  - `components/`:
    - `Sidebar.tsx`, `Artwork.tsx`, `ContextMenu.tsx`, `Toasts.tsx`, `Dialog.tsx`, `PromptDialog.tsx`;
    - `components/ui.module.css`, which holds the shared styles for those components;
  - `views/`: `HomeView.tsx` + `HomeView.module.css` (moved from `components/Home*`), `SignalView.tsx`, `NotFoundView.tsx`;
  - tests: `app/test/setup.ts`, `app/test/helpers.tsx`, `app/test/library-store.test.ts`, `app/test/useEngineQuery.test.tsx`, `app/test/ContextMenu.test.tsx`, `app/test/Layout.test.tsx`.
- Delete: `app/src/renderer/App.tsx`, `App.module.css`, `components/IconRail.tsx`, `components/Home.tsx`, `components/Home.module.css`

**Interfaces:**
- Consumes: `AuroraApi`/`Methods`/`Events` and the settings IPC (Task 17); `usePlayer` (M1).
- Produces:
  - **Router:**
    - `createAppRouter(history?) -> Router`; the default is hash history.
    - `routeTree` with `/` (HomeView) and `/signal` (SignalView). The not-found component is `NotFoundView`.
    - Later tasks add their routes to `routeTree` in `router.tsx`.
  - **`Layout`:**
    - The structure is: titlebar, `Sidebar`, then `<main>` holding `EngineBanner` and `<Outlet/>`, then `PlayerBar`, then the hosts for `ContextMenu`, `Toasts` and `PromptDialog`.
    - It persists `lastView` on navigation.
    - It turns `usePlayer.error` into a toast (spec §6 non-blocking notices).
  - **`lib/strings.ts`:** a frozen `S` object. All copy added in M2 goes here (Tasks 19–21 add keys).
  - **`lib/art.ts`:** `artUrl(id: number, size: 64 | 300 | 1000) -> string`.
  - **`lib/nav.ts`:** `NAV: { to: string; label: string; icon: LucideIcon }[]`, which Tasks 19–21 extend, and `isActive(pathname, to)`.
  - **`lib/useEngineQuery.ts`:** `useEngineQuery<T>(key: unknown[], fetcher: (api: AuroraApi) => Promise<T>, scopes?: LibraryScope[]) -> { data: T | undefined; error: string | null; loading: boolean; reload(): void }`.
    - It re-runs when `key` changes, when any listed scope's version bumps (the default scope is `['tracks']`), or when the engine becomes ready.
    - Stale responses are dropped.
  - **`stores/library.ts` (`useLibrary`):**
    - State: `versions: Record<LibraryScope, number>`, `folders: Folder[]`, `scan: ScanProgress | null`, `playlists: Playlist[]`.
    - Actions:
      - `connect(api) -> unsubscribe`;
      - `refreshFolders()`, `refreshPlaylists()`;
      - `addFolder()`, `removeFolder(id)`, `rescan(id?)`;
      - `createPlaylist(name) -> Promise<Playlist | null>`, `addToPlaylist(playlistId, trackIds)`.
  - **`stores/ui.ts` (`useUi`):**
    - State: `sidebarExpanded`, `toasts: Toast[]`, `menu: MenuState | null`, `trackInfo: number | null`, `prompt: PromptState | null`, `searchText: string`.
    - Sidebar and settings: `setSidebar(expanded, persist?)`, `applySettings(Settings)`.
    - Toasts: `toast(kind: 'info' | 'error', text)` (auto-dismissed after 5 s) and `dismissToast(id)`.
    - Menu: `openMenu(x, y, items: MenuItem[])`, `closeMenu()`.
    - Dialogs and search: `showTrackInfo(id | null)`, `askName({ title, initial?, confirm }) -> Promise<string | null>`, `setSearchText(text)`.
  - **`MenuItem`:** `{ label: string; icon?: LucideIcon; onSelect?: () => void; disabled?: boolean; items?: MenuItem[]; separator?: boolean }`.
  - **`Artwork`:** `<Artwork id={number | null} size={64 | 300 | 1000} alt className? />`. It falls back to a gradient tile with a `Disc3` icon on null or on load error.
  - **`Dialog`:** `<Dialog title onClose labelledBy?>{children}</Dialog>`. It's modal (`role="dialog" aria-modal`), Escape closes it, and focus moves to the first field on open and returns to the opener on close.
  - **Test helpers (`test/helpers.tsx`):**
    - `fakeApi(overrides?) -> { api, emit, calls }`;
    - `renderAt(path, api?) -> Promise<{ router }>`: renders the real route tree in a memory history, with stores connected.

- [ ] **Step 1: Install the UI dependencies and the test setup**

Run: `npm --prefix app install --save-exact @tanstack/react-router@1.170.42 @tanstack/react-virtual@3.14.14 @dnd-kit/core@6.3.1 @dnd-kit/sortable@10.0.0 @dnd-kit/utilities@3.2.2`
Expected: `app/package.json` dependencies list those exact versions, and `package-lock.json` updates.

In `app/vitest.config.ts`, set `test: { include: ['test/**/*.test.{ts,tsx}'], environment: 'node', setupFiles: ['test/setup.ts'] }`.

Create `app/test/setup.ts`:

```ts
// jsdom has no layout: give elements a size so virtualized lists render rows, and stub the observers.
if (typeof window !== 'undefined') {
  const rect = { x: 0, y: 0, top: 0, left: 0, bottom: 800, right: 1000, width: 1000, height: 800, toJSON: () => ({}) }
  Element.prototype.getBoundingClientRect = function () { return rect as DOMRect }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, get: () => 800 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, get: () => 1000 })
  Element.prototype.scrollTo = function () {}
  Element.prototype.scrollIntoView = function () {}
  globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
}
```

- [ ] **Step 2: Write the shared test helpers and the failing tests**

`app/test/helpers.tsx`:

```tsx
import { render } from '@testing-library/react'
import { act } from 'react'
import { createMemoryHistory, RouterProvider } from '@tanstack/react-router'
import { vi } from 'vitest'
import type { AuroraApi, Methods } from '../src/shared/api'
import { createAppRouter } from '../src/renderer/router'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { DEFAULT_SETTINGS } from '../src/shared/settings'

type Handler = (params: any) => any
const DEFAULTS: Partial<Record<keyof Methods, Handler>> = {
  'player.state': () => ({ status: 'stopped', queueIndex: null, queueItemId: null, track: null, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null, shuffle: false, repeat: 'off' }),
  'queue.get': () => ({ items: [], current: null }),
  'devices.list': () => [],
  'library.folders': () => [],
  'playlists.list': () => [],
  'library.recent': () => ({ albums: [], tracks: [] }),
  'library.albums': () => ({ items: [], total: 0 }),
  'library.artists': () => ({ items: [], total: 0 }),
  'library.genres': () => ({ items: [], total: 0 }),
  'library.tracks': () => ({ items: [], total: 0 }),
  'library.search': () => ({ tracks: [], albums: [], artists: [] }),
}

export function fakeApi(overrides: Partial<Record<keyof Methods, Handler>> = {}) {
  const handlers = new Map<string, Set<(p: any) => void>>()
  const calls: [string, any][] = []
  const api = {
    call: vi.fn(async (m: string, p: any) => { calls.push([m, p]); const h = overrides[m as keyof Methods] ?? DEFAULTS[m as keyof Methods]; return h ? h(p) : null }),
    on: vi.fn((e: string, cb: (p: any) => void) => { if (!handlers.has(e)) handlers.set(e, new Set()); handlers.get(e)!.add(cb); return () => handlers.get(e)!.delete(cb) }),
    openFiles: vi.fn(async () => []),
    openFolder: vi.fn(async () => null),
    pickM3u: vi.fn(async () => null),
    pickM3uSave: vi.fn(async () => null),
    showItemInFolder: vi.fn(async () => {}),
    getSettings: vi.fn(async () => DEFAULT_SETTINGS),
    setSettings: vi.fn(async (p: any) => ({ ...DEFAULT_SETTINGS, ...p })),
    pathsForFiles: vi.fn(() => []),
    status: vi.fn(async () => ({ state: 'ready' })),
    restartEngine: vi.fn(async () => {}),
    openLogs: vi.fn(async () => {}),
    platform: 'linux',
  }
  const emit = (e: string, p: unknown) => act(() => { handlers.get(e)?.forEach((cb) => cb(p)) })
  return { api: api as unknown as AuroraApi & typeof api, emit, calls }
}

export function resetStores() {
  usePlayer.setState(usePlayer.getInitialState())
  useLibrary.setState(useLibrary.getInitialState())
  useUi.setState(useUi.getInitialState())
}

/** Renders the real route tree at `path` with the stores connected to `api`. */
export async function renderAt(path: string, api: AuroraApi = fakeApi().api) {
  resetStores()
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }))
  usePlayer.getState().connect(api)
  useLibrary.getState().connect(api)
  await act(async () => { render(<RouterProvider router={router} />) })
  return { router }
}
```

`app/test/library-store.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useLibrary } from '../src/renderer/stores/library'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, resetStores } from './helpers'

describe('library store', () => {
  beforeEach(resetStores)

  it('loads folders and playlists once the engine is ready', async () => {
    const { api } = fakeApi({ 'library.folders': () => [{ id: 1, path: '/m', trackCount: 3, lastScanAt: 1, available: true }], 'playlists.list': () => [{ id: 4, name: 'Mix', trackCount: 0, durationMs: 0, updatedAt: 0 }] })
    useLibrary.getState().connect(api)
    await vi.waitFor(() => expect(useLibrary.getState().folders).toHaveLength(1))
    expect(useLibrary.getState().playlists[0].name).toBe('Mix')
  })

  it('bumps scope versions and reloads what changed', async () => {
    const { api, emit, calls } = fakeApi()
    useLibrary.getState().connect(api)
    await vi.waitFor(() => expect(calls.some(([m]) => m === 'playlists.list')).toBe(true))
    calls.length = 0
    emit('library.changed', { scopes: ['playlists', 'tracks'] })
    expect(useLibrary.getState().versions.playlists).toBe(1)
    expect(useLibrary.getState().versions.tracks).toBe(1)
    expect(useLibrary.getState().versions.folders).toBe(0)
    await vi.waitFor(() => expect(calls.some(([m]) => m === 'playlists.list')).toBe(true))
  })

  it('tracks scan progress', () => {
    const { api, emit } = fakeApi()
    useLibrary.getState().connect(api)
    emit('library.scanProgress', { folderId: 1, phase: 'reading', processed: 5, total: 10, added: 5, updated: 0, missing: 0 })
    expect(useLibrary.getState().scan?.processed).toBe(5)
  })

  it('adds the picked folder, and does nothing when the picker is cancelled', async () => {
    const { api, calls } = fakeApi({ 'library.addFolder': (p) => ({ id: 2, path: p.path, trackCount: 0, lastScanAt: null, available: true }) })
    useLibrary.getState().connect(api)
    await useLibrary.getState().addFolder()
    expect(calls.some(([m]) => m === 'library.addFolder')).toBe(false)
    api.openFolder.mockResolvedValueOnce('/music')
    await useLibrary.getState().addFolder()
    expect(calls).toContainEqual(['library.addFolder', { path: '/music' }])
  })

  it('reports failures as error toasts', async () => {
    const { api } = fakeApi({ 'library.addFolder': () => { throw new Error('/x overlaps the library folder /x/y') } })
    useLibrary.getState().connect(api)
    api.openFolder.mockResolvedValueOnce('/x')
    await useLibrary.getState().addFolder()
    expect(useUi.getState().toasts.at(-1)).toMatchObject({ kind: 'error', text: '/x overlaps the library folder /x/y' })
  })
})
```

`app/test/useEngineQuery.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useEngineQuery } from '../src/renderer/lib/useEngineQuery'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, resetStores } from './helpers'

afterEach(cleanup)

function Probe({ fetcher, scopes }: { fetcher: () => Promise<string>; scopes?: ('tracks' | 'playlists')[] }) {
  const q = useEngineQuery(['probe'], fetcher, scopes)
  return <div data-testid="out">{q.data ?? (q.loading ? 'loading' : 'none')}</div>
}

describe('useEngineQuery', () => {
  beforeEach(() => { resetStores(); usePlayer.setState({ api: fakeApi().api, engine: { state: 'ready' } }) })

  it('fetches, then refetches when its scope changes', async () => {
    let n = 0
    const fetcher = vi.fn(async () => `v${++n}`)
    render(<Probe fetcher={fetcher} />)
    expect(await screen.findByText('v1')).toBeTruthy()
    act(() => useLibrary.setState((s) => ({ versions: { ...s.versions, playlists: 1 } })))
    expect(fetcher).toHaveBeenCalledTimes(1)
    act(() => useLibrary.setState((s) => ({ versions: { ...s.versions, tracks: 1 } })))
    expect(await screen.findByText('v2')).toBeTruthy()
  })

  it('drops responses that arrive after a newer request', async () => {
    let resolveFirst: (v: string) => void = () => {}
    const fetcher = vi.fn().mockImplementationOnce(() => new Promise<string>((r) => { resolveFirst = r })).mockResolvedValueOnce('fresh')
    render(<Probe fetcher={fetcher} />)
    act(() => useLibrary.setState((s) => ({ versions: { ...s.versions, tracks: 1 } })))
    expect(await screen.findByText('fresh')).toBeTruthy()
    await act(async () => resolveFirst('stale'))
    expect(screen.getByTestId('out').textContent).toBe('fresh')
  })
})
```

`app/test/ContextMenu.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ContextMenu } from '../src/renderer/components/ContextMenu'
import { useUi } from '../src/renderer/stores/ui'

afterEach(cleanup)

describe('ContextMenu', () => {
  beforeEach(() => useUi.setState(useUi.getInitialState()))

  it('opens at the pointer, selects with Enter and closes', () => {
    const play = vi.fn()
    render(<ContextMenu />)
    act(() => useUi.getState().openMenu(10, 20, [{ label: 'Play next', onSelect: play }, { label: 'Disabled', disabled: true }]))
    const menu = screen.getByRole('menu')
    expect(screen.getAllByRole('menuitem')).toHaveLength(2)
    fireEvent.keyDown(menu, { key: 'Enter' })
    expect(play).toHaveBeenCalled()
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('moves with the arrow keys, skipping disabled items, and closes on Escape', () => {
    const second = vi.fn()
    render(<ContextMenu />)
    act(() => useUi.getState().openMenu(0, 0, [{ label: 'A' }, { label: 'B', disabled: true }, { label: 'C', onSelect: second }]))
    const menu = screen.getByRole('menu')
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    fireEvent.keyDown(menu, { key: 'Enter' })
    expect(second).toHaveBeenCalled()
    act(() => useUi.getState().openMenu(0, 0, [{ label: 'A' }]))
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('opens a submenu with ArrowRight', () => {
    const pick = vi.fn()
    render(<ContextMenu />)
    act(() => useUi.getState().openMenu(0, 0, [{ label: 'Add to playlist', items: [{ label: 'Mix', onSelect: pick }] }]))
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowRight' })
    const menus = screen.getAllByRole('menu')
    expect(menus).toHaveLength(2)
    fireEvent.keyDown(menus[1], { key: 'Enter' })
    expect(pick).toHaveBeenCalled()
  })
})
```

`app/test/Layout.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, renderAt } from './helpers'

afterEach(cleanup)

describe('layout and routing', () => {
  it('routes to the signal path view and marks the nav item current', async () => {
    await renderAt('/signal')
    expect(await screen.findByText('Play something to see its signal path.')).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Signal path' }).getAttribute('aria-current')).toBe('page')
  })

  it('shows a not-found view for unknown routes', async () => {
    await renderAt('/nowhere')
    expect(await screen.findByText('This view does not exist.')).toBeTruthy()
  })

  it('persists the current view and the sidebar state', async () => {
    const { api } = fakeApi()
    const { router } = await renderAt('/', api)
    await act(async () => { await router.navigate({ to: '/signal' }) })
    await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ lastView: '/signal' }))
    fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }))
    expect(useUi.getState().sidebarExpanded).toBe(true)
    expect(api.setSettings).toHaveBeenCalledWith({ sidebarExpanded: true })
  })

  it('turns engine errors into toasts', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ error: { code: -32002, name: 'FILE_MISSING', message: 'gone', path: '/m/a.flac' } }))
    expect(await screen.findByRole('status')).toBeTruthy()
    expect(screen.getByText(/gone/)).toBeTruthy()
  })
})
```

Update `app/test/PlayerBar.test.tsx`: render it through the app's routes instead of bare. Replace each `render(<PlayerBar />)` with `await renderAt('/')`, add `import { renderAt } from './helpers'`, and make both test callbacks `async`. The `beforeEach` that resets `usePlayer` stays; `setState` calls move to *after* `renderAt`. `renderAt` resets the stores, so wrap them in `act`.

```tsx
  it('toggles playback through the store', async () => {
    await renderAt('/')
    const toggle = vi.fn(async () => {})
    act(() => usePlayer.setState({ toggle, state: { ...usePlayer.getState().state, status: 'paused', track: { path: '/a.flac', title: 'a', durationMs: 1000, trackId: null, artist: null, album: null, artworkId: null } } }))
    fireEvent.click(screen.getByRole('button', { name: 'Play' }))
    expect(toggle).toHaveBeenCalled()
    expect(screen.getByTestId('now-title').textContent).toBe('a')
  })

  it('disables software volume in bit-perfect mode', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, mode: 'bitPerfect' } }))
    const vol = screen.getByRole('slider', { name: 'Volume' }) as HTMLInputElement
    expect(vol.disabled).toBe(true)
    expect(vol.title).toBe('Bit-perfect: use your system or DAC volume')
    expect(screen.getByRole('switch', { name: 'Bit-perfect' }).getAttribute('aria-checked')).toBe('true')
  })
```

(Import `act` from `react`.)

- [ ] **Step 3: Run the tests to verify they fail**

Run: `npm --prefix app test`
Expected: the new tests fail to import (`router`, `stores/library`, `stores/ui`, `ContextMenu` don't exist).

- [ ] **Step 4: Implement the libraries and stores**

`app/src/renderer/lib/strings.ts`:

```ts
/** All UI copy added in M2 (spec §1: one module, so i18n can come later). */
export const S = Object.freeze({
  app: 'aurora',
  nav: { home: 'Home', search: 'Search', albums: 'Albums', artists: 'Artists', genres: 'Genres', playlists: 'Playlists', queue: 'Queue', signal: 'Signal path', settings: 'Settings' },
  expandSidebar: 'Expand sidebar',
  collapseSidebar: 'Collapse sidebar',
  unknownArtist: 'Unknown artist',
  unknownAlbum: 'Unknown album',
  notFound: 'This view does not exist.',
  goHome: 'Go home',
  cancel: 'Cancel',
  save: 'Save',
  dismiss: 'Dismiss',
  missingFile: 'File missing',
  unplayable: (reason: string) => `Can't play: ${reason}`,
  scanning: (processed: number, total: number) => (total ? `Scanning ${processed} / ${total}` : 'Scanning…'),
})
```

`app/src/renderer/lib/art.ts`:

```ts
export type ArtSize = 64 | 300 | 1000
/** URL served by the main process's `aurora-art://` handler (which asks the engine for the cache file). */
export const artUrl = (id: number, size: ArtSize): string => `aurora-art://art/${id}/${size}`
```

`app/src/renderer/lib/nav.ts`:

```ts
import { Activity, House, type LucideIcon } from 'lucide-react'
import { S } from './strings'

export interface NavItem { to: string; label: string; icon: LucideIcon }

/** Sidebar entries in display order. Tasks add their views here as their routes land. */
export const NAV: NavItem[] = [
  { to: '/', label: S.nav.home, icon: House },
  { to: '/signal', label: S.nav.signal, icon: Activity },
]

export function isActive(pathname: string, to: string): boolean {
  return to === '/' ? pathname === '/' : pathname === to || pathname.startsWith(`${to}/`)
}
```

`app/src/renderer/stores/ui.ts`:

```ts
import { create } from 'zustand'
import type { LucideIcon } from 'lucide-react'
import type { Settings } from '@shared/settings'
import { usePlayer } from './player'

export interface MenuItem { label: string; icon?: LucideIcon; onSelect?: () => void; disabled?: boolean; items?: MenuItem[]; separator?: boolean }
export interface Toast { id: number; kind: 'info' | 'error'; text: string }
export interface PromptState { title: string; initial: string; confirm: string; resolve: (value: string | null) => void }

export interface UiStore {
  sidebarExpanded: boolean
  toasts: Toast[]
  menu: { x: number; y: number; items: MenuItem[] } | null
  trackInfo: number | null
  prompt: PromptState | null
  searchText: string
  setSidebar(expanded: boolean, persist?: boolean): void
  applySettings(s: Settings): void
  toast(kind: Toast['kind'], text: string): void
  dismissToast(id: number): void
  openMenu(x: number, y: number, items: MenuItem[]): void
  closeMenu(): void
  showTrackInfo(id: number | null): void
  askName(o: { title: string; initial?: string; confirm: string }): Promise<string | null>
  setSearchText(text: string): void
}

let nextToast = 1
export const TOAST_MS = 5000

export const useUi = create<UiStore>()((set, get) => ({
  sidebarExpanded: false,
  toasts: [],
  menu: null,
  trackInfo: null,
  prompt: null,
  searchText: '',
  setSidebar(expanded, persist = true) {
    set({ sidebarExpanded: expanded })
    if (persist) void usePlayer.getState().api?.setSettings({ sidebarExpanded: expanded }).catch(() => {})
  },
  applySettings: (s) => set({ sidebarExpanded: s.sidebarExpanded }),
  toast(kind, text) {
    const id = nextToast++
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, text }] }))
    setTimeout(() => get().dismissToast(id), TOAST_MS)
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  openMenu: (x, y, items) => set({ menu: { x, y, items } }),
  closeMenu: () => set({ menu: null }),
  showTrackInfo: (trackInfo) => set({ trackInfo }),
  askName: ({ title, initial = '', confirm }) =>
    new Promise((resolve) => {
      get().prompt?.resolve(null)
      set({ prompt: { title, initial, confirm, resolve: (v) => { set({ prompt: null }); resolve(v) } } })
    }),
  setSearchText: (searchText) => set({ searchText }),
}))
```

`app/src/renderer/stores/library.ts`:

```ts
import { create } from 'zustand'
import type { Folder } from '@protocol/Folder'
import type { LibraryScope } from '@protocol/LibraryScope'
import type { Playlist } from '@protocol/Playlist'
import type { ScanProgress } from '@protocol/ScanProgress'
import type { AuroraApi } from '@shared/api'
import { useUi } from './ui'

export interface LibraryStore {
  api: AuroraApi | null
  versions: Record<LibraryScope, number>
  folders: Folder[]
  scan: ScanProgress | null
  playlists: Playlist[]
  connect(api: AuroraApi): () => void
  refreshFolders(): Promise<void>
  refreshPlaylists(): Promise<void>
  addFolder(): Promise<void>
  removeFolder(id: number): Promise<void>
  rescan(id?: number): Promise<void>
  createPlaylist(name: string): Promise<Playlist | null>
  addToPlaylist(playlistId: number, trackIds: number[]): Promise<void>
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

export const useLibrary = create<LibraryStore>()((set, get) => {
  /** Runs an engine request; failures become error toasts and resolve `null`. */
  const run = async <T,>(p: (api: AuroraApi) => Promise<T>): Promise<T | null> => {
    const api = get().api
    if (!api) return null
    try { return await p(api) } catch (e) { useUi.getState().toast('error', message(e)); return null }
  }
  return {
    api: null,
    versions: { tracks: 0, folders: 0, playlists: 0, history: 0 },
    folders: [],
    scan: null,
    playlists: [],
    connect(api) {
      set({ api })
      const load = () => { void get().refreshFolders(); void get().refreshPlaylists() }
      const offs = [
        api.on('library.changed', ({ scopes }) => {
          set((s) => {
            const versions = { ...s.versions }
            for (const sc of scopes) versions[sc] += 1
            return { versions }
          })
          if (scopes.includes('folders') || scopes.includes('tracks')) void get().refreshFolders()
          if (scopes.includes('playlists')) void get().refreshPlaylists()
        }),
        api.on('library.scanProgress', (scan) => set({ scan })),
        api.on('engine.status', (s) => { if (s.state === 'ready') load() }),
      ]
      void api.status().then((s) => { if (s.state === 'ready') load() }).catch(() => {})
      return () => offs.forEach((off) => off())
    },
    async refreshFolders() { const f = await run((a) => a.call('library.folders', {})); if (f) set({ folders: f }) },
    async refreshPlaylists() { const p = await run((a) => a.call('playlists.list', {})); if (p) set({ playlists: p }) },
    async addFolder() {
      const api = get().api
      const path = await api?.openFolder().catch(() => null)
      if (!path) return
      if (await run((a) => a.call('library.addFolder', { path }))) await get().refreshFolders()
    },
    async removeFolder(id) { await run((a) => a.call('library.removeFolder', { id })); await get().refreshFolders() },
    async rescan(id) { await run((a) => a.call('library.rescan', id == null ? {} : { folderId: id })) },
    async createPlaylist(name) {
      const p = await run((a) => a.call('playlists.create', { name }))
      if (p) await get().refreshPlaylists()
      return p
    },
    async addToPlaylist(playlistId, trackIds) { await run((a) => a.call('playlists.addTracks', { id: playlistId, trackIds })) },
  }
})
```

`app/src/renderer/lib/useEngineQuery.ts`:

```ts
import { useCallback, useEffect, useRef, useState } from 'react'
import type { LibraryScope } from '@protocol/LibraryScope'
import type { AuroraApi } from '@shared/api'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'

/** Fetches engine data for a view; refetches when `key`, the engine, or a watched library scope changes. */
export function useEngineQuery<T>(key: unknown[], fetcher: (api: AuroraApi) => Promise<T>, scopes: LibraryScope[] = ['tracks']) {
  const api = usePlayer((p) => p.api)
  const ready = usePlayer((p) => p.engine.state === 'ready')
  const version = useLibrary((l) => scopes.reduce((n, s) => n + l.versions[s], 0))
  const [data, setData] = useState<T | undefined>(undefined)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [nonce, setNonce] = useState(0)
  const seq = useRef(0)
  const fetchRef = useRef(fetcher)
  fetchRef.current = fetcher
  const keyStr = JSON.stringify(key)
  useEffect(() => {
    if (!api || !ready) return
    const mine = ++seq.current
    setLoading(true)
    fetchRef.current(api).then(
      (d) => { if (mine === seq.current) { setData(d); setError(null); setLoading(false) } },
      (e: unknown) => { if (mine === seq.current) { setError(e instanceof Error ? e.message : String(e)); setLoading(false) } },
    )
  }, [api, ready, version, keyStr, nonce])
  const reload = useCallback(() => setNonce((n) => n + 1), [])
  return { data, error, loading, reload }
}
```

In `app/src/renderer/stores/player.ts`, remove `panel` and `setPanel` from the interface, the initial state and the actions.

- [ ] **Step 5: Implement the shared components**

`app/src/renderer/components/ui.module.css`:

```css
.art { display: block; object-fit: cover; border-radius: var(--radius-sm); background: var(--raised); }
.artFallback { display: grid; place-items: center; border-radius: var(--radius-sm); color: #17151a; background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
.menu { position: fixed; z-index: 50; min-width: 200px; padding: 4px; background: var(--raised); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: 0 12px 32px rgb(0 0 0 / 0.45); }
.menuItem { display: flex; align-items: center; gap: 10px; width: 100%; padding: 7px 10px; border-radius: var(--radius-sm); text-align: left; }
.menuItem[data-active='true'] { background: var(--raised-2); color: var(--accent); }
.menuItem[aria-disabled='true'] { opacity: 0.4; cursor: default; }
.menuItem .chevron { margin-left: auto; color: var(--text-muted); }
.separator { height: 1px; margin: 4px 6px; background: var(--border); }
.toasts { position: fixed; right: 18px; bottom: calc(var(--bar) + 14px); z-index: 60; display: flex; flex-direction: column; gap: 8px; }
.toast { display: flex; align-items: flex-start; gap: 10px; max-width: 420px; padding: 10px 12px; background: var(--raised); border: 1px solid var(--border); border-left: 3px solid var(--accent); border-radius: var(--radius); }
.toast[data-kind='error'] { border-left-color: var(--danger); }
.backdrop { position: fixed; inset: 0; z-index: 70; display: grid; place-items: center; background: rgb(0 0 0 / 0.5); }
.dialog { width: min(560px, calc(100vw - 48px)); max-height: calc(100vh - 96px); overflow: auto; padding: 20px 22px; background: var(--raised); border: 1px solid var(--border); border-radius: var(--radius); }
.dialog h2 { margin: 0 0 14px; font-size: 16px; }
.field { width: 100%; padding: 8px 10px; color: var(--text); background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius-sm); font: inherit; }
.actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 16px; }
.primary { padding: 7px 14px; border-radius: var(--radius-sm); background: var(--accent); color: #17151a; font-weight: 700; }
.secondary { padding: 7px 14px; border-radius: var(--radius-sm); border: 1px solid var(--border); }
```

`app/src/renderer/components/Artwork.tsx`:

```tsx
import { Disc3 } from 'lucide-react'
import { useState } from 'react'
import { artUrl, type ArtSize } from '../lib/art'
import s from './ui.module.css'

export function Artwork({ id, size, alt, className, px }: { id: number | null | undefined; size: ArtSize; alt: string; className?: string; px?: number }) {
  const [failed, setFailed] = useState<number | null>(null)
  const dim = px ?? size
  if (id == null || failed === id) {
    return <div className={`${s.artFallback} ${className ?? ''}`} style={{ width: dim, height: dim }} role="img" aria-label={alt}><Disc3 size={Math.max(16, dim / 3)} /></div>
  }
  return <img className={`${s.art} ${className ?? ''}`} src={artUrl(id, size)} alt={alt} width={dim} height={dim} loading="lazy" draggable={false} onError={() => setFailed(id)} />
}
```

`app/src/renderer/components/ContextMenu.tsx`:

```tsx
import { ChevronRight } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useUi, type MenuItem } from '../stores/ui'
import s from './ui.module.css'

function enabledIndex(items: MenuItem[], from: number, step: 1 | -1): number {
  for (let i = 1; i <= items.length; i++) {
    const j = (from + step * i + items.length * 2) % items.length
    if (!items[j].disabled && !items[j].separator) return j
  }
  return from
}

/** One menu level. `onClose` closes this level (Escape/ArrowLeft); `closeAll` closes the whole menu after a choice. */
function MenuList({ items, x, y, onClose, closeAll, depth }: { items: MenuItem[]; x: number; y: number; onClose: () => void; closeAll: () => void; depth: number }) {
  const ref = useRef<HTMLDivElement>(null)
  const [active, setActive] = useState(() => enabledIndex(items, -1, 1))
  const [sub, setSub] = useState<number | null>(null)
  const [pos, setPos] = useState({ x, y })
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const r = el.getBoundingClientRect()
    setPos({ x: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)), y: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)) })
    el.focus()
  }, [x, y])
  const choose = (i: number) => {
    const it = items[i]
    if (!it || it.disabled || it.separator) return
    if (it.items) { setSub(i); return }
    closeAll()
    it.onSelect?.()
  }
  const onKey = (e: React.KeyboardEvent) => {
    e.stopPropagation()
    if (e.key === 'ArrowDown') { e.preventDefault(); setActive((a) => enabledIndex(items, a, 1)) }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive((a) => enabledIndex(items, a, -1)) }
    else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); choose(active) }
    else if (e.key === 'ArrowRight' && items[active]?.items) { e.preventDefault(); setSub(active) }
    else if (e.key === 'ArrowLeft' && depth > 0) { e.preventDefault(); onClose() }
    else if (e.key === 'Escape') { e.preventDefault(); onClose() }
  }
  const subRect = sub != null ? ref.current?.children[sub]?.getBoundingClientRect() : undefined
  return (
    <>
      <div ref={ref} role="menu" tabIndex={-1} className={s.menu} style={{ left: pos.x, top: pos.y }} onKeyDown={onKey} onContextMenu={(e) => e.preventDefault()}>
        {items.map((it, i) => it.separator ? <div key={i} className={s.separator} role="separator" /> : (
          <button
            key={i} role="menuitem" type="button" tabIndex={-1} className={s.menuItem} data-active={i === active}
            aria-disabled={it.disabled || undefined} aria-haspopup={it.items ? 'menu' : undefined} aria-expanded={it.items ? sub === i : undefined}
            onMouseEnter={() => { setActive(i); setSub(it.items ? i : null) }} onClick={() => choose(i)}
          >
            {it.icon && <it.icon size={14} aria-hidden />}
            <span>{it.label}</span>
            {it.items && <ChevronRight size={14} className={s.chevron} aria-hidden />}
          </button>
        ))}
      </div>
      {sub != null && items[sub]?.items && (
        <MenuList items={items[sub].items!} x={(subRect?.right ?? pos.x + 200) - 2} y={subRect?.top ?? pos.y} depth={depth + 1}
          closeAll={closeAll} onClose={() => { setSub(null); ref.current?.focus() }} />
      )}
    </>
  )
}

/** The single app-wide context menu, driven by `useUi().openMenu`. */
export function ContextMenu() {
  const menu = useUi((u) => u.menu)
  const close = useUi((u) => u.closeMenu)
  useEffect(() => {
    if (!menu) return
    const onDown = (e: MouseEvent) => { if (!(e.target as Element).closest?.('[role=menu]')) close() }
    window.addEventListener('mousedown', onDown)
    window.addEventListener('blur', close)
    return () => { window.removeEventListener('mousedown', onDown); window.removeEventListener('blur', close) }
  }, [menu, close])
  if (!menu) return null
  return <MenuList key={`${menu.x},${menu.y},${menu.items.length}`} items={menu.items} x={menu.x} y={menu.y} depth={0} onClose={close} closeAll={close} />
}
```

`app/src/renderer/components/Toasts.tsx`:

```tsx
import { X } from 'lucide-react'
import { S } from '../lib/strings'
import { useUi } from '../stores/ui'
import s from './ui.module.css'

export function Toasts() {
  const toasts = useUi((u) => u.toasts)
  const dismiss = useUi((u) => u.dismissToast)
  return (
    <div className={s.toasts} aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={s.toast} data-kind={t.kind} role="status">
          <span style={{ flex: 1 }}>{t.text}</span>
          <button aria-label={S.dismiss} onClick={() => dismiss(t.id)} className="muted"><X size={14} /></button>
        </div>
      ))}
    </div>
  )
}
```

`app/src/renderer/components/Dialog.tsx`:

```tsx
import { useEffect, useId, useRef } from 'react'
import s from './ui.module.css'

/** Modal dialog: Escape and backdrop click close it; focus moves in on open and back on close. */
export function Dialog({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  const id = useId()
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null
    const first = ref.current?.querySelector<HTMLElement>('input, textarea, select, button')
    first?.focus()
    return () => opener?.focus?.()
  }, [])
  return (
    <div className={s.backdrop} onMouseDown={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <div ref={ref} role="dialog" aria-modal="true" aria-labelledby={id} className={s.dialog}
        onKeyDown={(e) => { e.stopPropagation(); if (e.key === 'Escape') { e.preventDefault(); onClose() } }}>
        <h2 id={id}>{title}</h2>
        {children}
      </div>
    </div>
  )
}
```

`app/src/renderer/components/PromptDialog.tsx`:

```tsx
import { useState } from 'react'
import { S } from '../lib/strings'
import { useUi } from '../stores/ui'
import { Dialog } from './Dialog'
import s from './ui.module.css'

/** Text prompt driven by `useUi().askName` (new playlist, rename). */
export function PromptDialog() {
  const prompt = useUi((u) => u.prompt)
  if (!prompt) return null
  return <PromptBody key={prompt.title + prompt.initial} />
}

function PromptBody() {
  const prompt = useUi((u) => u.prompt)!
  const [value, setValue] = useState(prompt.initial)
  const ok = value.trim().length > 0
  return (
    <Dialog title={prompt.title} onClose={() => prompt.resolve(null)}>
      <form onSubmit={(e) => { e.preventDefault(); if (ok) prompt.resolve(value.trim()) }}>
        <input className={s.field} value={value} maxLength={200} aria-label={prompt.title} onChange={(e) => setValue(e.target.value)} />
        <div className={s.actions}>
          <button type="button" className={s.secondary} onClick={() => prompt.resolve(null)}>{S.cancel}</button>
          <button type="submit" className={s.primary} disabled={!ok}>{prompt.confirm}</button>
        </div>
      </form>
    </Dialog>
  )
}
```

`app/src/renderer/components/Sidebar.tsx` (the rail ↔ 220 px sidebar from spec §5.5):

```tsx
import { Link, useRouterState } from '@tanstack/react-router'
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { isActive, NAV } from '../lib/nav'
import { S } from '../lib/strings'
import { useUi } from '../stores/ui'
import s from '../Layout.module.css'

export function Sidebar({ children }: { children?: React.ReactNode }) {
  const expanded = useUi((u) => u.sidebarExpanded)
  const pathname = useRouterState({ select: (r) => r.location.pathname })
  return (
    <nav aria-label="Main" className={s.sidebar} data-expanded={expanded}>
      <button className={s.toggle} aria-label={expanded ? S.collapseSidebar : S.expandSidebar} aria-expanded={expanded}
        onClick={() => useUi.getState().setSidebar(!expanded)}>
        {expanded ? <PanelLeftClose size={18} /> : <PanelLeftOpen size={18} />}
      </button>
      <ul className={s.nav}>
        {NAV.map((n) => {
          const active = isActive(pathname, n.to)
          return (
            <li key={n.to}>
              <Link to={n.to as never} className={s.navLink} aria-current={active ? 'page' : undefined} aria-label={n.label} title={expanded ? undefined : n.label}>
                <n.icon size={18} aria-hidden />
                {expanded && <span>{n.label}</span>}
              </Link>
            </li>
          )
        })}
      </ul>
      {expanded && children}
    </nav>
  )
}
```

- [ ] **Step 6: Implement the views, layout and router**

Move `components/Home.tsx` to `views/HomeView.tsx` (export `HomeView`) and `components/Home.module.css` to `views/HomeView.module.css`, fixing the import paths (`../stores/player`). The content is M1's queue list, unchanged; Task 19 replaces it.

`app/src/renderer/views/SignalView.tsx`:

```tsx
import { SignalPathPanel } from '../components/SignalPathPanel'
import { usePlayer } from '../stores/player'

export function SignalView() {
  const signal = usePlayer((p) => p.signal)
  return <SignalPathPanel signal={signal} />
}
```

`app/src/renderer/views/NotFoundView.tsx`:

```tsx
import { Link } from '@tanstack/react-router'
import { S } from '../lib/strings'

export function NotFoundView() {
  return <div style={{ padding: 40 }}><p>{S.notFound}</p><Link to="/" style={{ color: 'var(--accent)' }}>{S.goHome}</Link></div>
}
```

`app/src/renderer/Layout.module.css`:

```css
.app { --side: var(--rail); display: grid; grid-template-columns: var(--side) 1fr; grid-template-rows: var(--titlebar) 1fr var(--bar); height: 100vh; transition: grid-template-columns var(--dur) var(--ease); }
.app[data-expanded='true'] { --side: 220px; }
.titlebar { grid-column: 1 / -1; display: flex; align-items: center; padding: 0 16px; background: var(--surface); border-bottom: 1px solid var(--border); color: var(--text-faint); font-size: 11px; letter-spacing: 0.12em; }
.app[data-platform='darwin'] .titlebar { padding-left: 84px; }
.main { overflow: auto; padding: 24px 28px; min-width: 0; }
.app > footer { grid-column: 1 / -1; }
.sidebar { display: flex; flex-direction: column; gap: 6px; padding: 10px 12px; background: var(--surface); border-right: 1px solid var(--border); overflow-y: auto; overflow-x: hidden; }
.sidebar[data-expanded='false'] { align-items: center; padding: 10px 0; }
.toggle { width: 40px; height: 36px; display: grid; place-items: center; color: var(--text-muted); border-radius: var(--radius); }
.toggle:hover { color: var(--text); }
.nav { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; width: 100%; }
.sidebar[data-expanded='false'] .nav { align-items: center; }
.navLink { display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 11px; border-radius: var(--radius); color: var(--text-muted); text-decoration: none; white-space: nowrap; }
.sidebar[data-expanded='false'] .navLink { width: 40px; justify-content: center; padding: 0; }
.navLink:hover { color: var(--text); background: var(--raised); }
.navLink[aria-current='page'] { color: var(--accent); background: var(--raised); }
.sectionTitle { margin: 14px 11px 4px; font-size: 10px; letter-spacing: 0.1em; color: var(--text-faint); }
```

`app/src/renderer/Layout.tsx`:

```tsx
import { Outlet, useRouterState } from '@tanstack/react-router'
import { useEffect } from 'react'
import { ContextMenu } from './components/ContextMenu'
import { EngineBanner } from './components/EngineBanner'
import { PlayerBar } from './components/PlayerBar'
import { PromptDialog } from './components/PromptDialog'
import { Sidebar } from './components/Sidebar'
import { Toasts } from './components/Toasts'
import { handleShortcut } from './lib/shortcuts'
import { S } from './lib/strings'
import { usePlayer } from './stores/player'
import { useUi } from './stores/ui'
import s from './Layout.module.css'

export function Layout() {
  const api = usePlayer((p) => p.api)
  const expanded = useUi((u) => u.sidebarExpanded)
  const pathname = useRouterState({ select: (r) => r.location.pathname })
  const error = usePlayer((p) => p.error)

  useEffect(() => { void api?.setSettings({ lastView: pathname }).catch(() => {}) }, [api, pathname])
  useEffect(() => {
    if (!error) return
    useUi.getState().toast('error', `${error.message}${error.path ? ` · ${error.path.split(/[\\/]/).pop()}` : ''}`)
    usePlayer.getState().dismissError()
  }, [error])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { handleShortcut(e, usePlayer.getState(), performance.now()) }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <div className={s.app} data-platform={api?.platform} data-expanded={expanded}>
      <div className={`${s.titlebar} drag`}><span className="mono">{S.app}</span></div>
      <Sidebar />
      <main className={s.main}>
        <EngineBanner />
        <Outlet />
      </main>
      <PlayerBar />
      <ContextMenu />
      <Toasts />
      <PromptDialog />
    </div>
  )
}
```

`EngineBanner` keeps showing engine failures and version mismatches (blocking states). Per-track errors now arrive as toasts, because `Layout` clears `usePlayer.error` immediately.

`app/src/renderer/router.tsx`:

```tsx
import { createHashHistory, createRootRoute, createRoute, createRouter, type RouterHistory } from '@tanstack/react-router'
import { Layout } from './Layout'
import { HomeView } from './views/HomeView'
import { NotFoundView } from './views/NotFoundView'
import { SignalView } from './views/SignalView'

const rootRoute = createRootRoute({ component: Layout, notFoundComponent: NotFoundView })
const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: '/', component: HomeView })
const signalRoute = createRoute({ getParentRoute: () => rootRoute, path: '/signal', component: SignalView })

// Later tasks add their routes to this list.
export const routeTree = rootRoute.addChildren([homeRoute, signalRoute])

export function createAppRouter(history: RouterHistory = createHashHistory()) {
  return createRouter({ routeTree, history, defaultPreload: false })
}

declare module '@tanstack/react-router' {
  interface Register { router: ReturnType<typeof createAppRouter> }
}
```

Replace `app/src/renderer/main.tsx`'s render with a bootstrap that connects the stores, applies settings and restores the last view:

```tsx
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import '@fontsource/inter/700.css'
import '@fontsource/inter/800.css'
import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/700.css'
import './styles/tokens.css'
import './styles/global.css'
import { RouterProvider } from '@tanstack/react-router'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { installDropGuard } from './lib/dropGuard'
import { createAppRouter } from './router'
import { useLibrary } from './stores/library'
import { usePlayer } from './stores/player'
import { useUi } from './stores/ui'

installDropGuard(document)
const api = window.aurora
const router = createAppRouter()
usePlayer.getState().connect(api)
useLibrary.getState().connect(api)
void api.getSettings().then((s) => {
  useUi.getState().applySettings(s)
  const hash = window.location.hash
  if ((!hash || hash === '#/' || hash === '#') && s.lastView !== '/') router.history.replace(s.lastView)
}).catch(() => {})

createRoot(document.getElementById('root')!).render(<StrictMode><RouterProvider router={router} /></StrictMode>)
```

In `PlayerBar.tsx`:
- Replace the queue button's `onClick={() => setPanel('queue')}` with `onClick={() => void navigate({ to: '/' })}`, where `const navigate = useNavigate()` comes from `@tanstack/react-router`. Task 20 points it at `/queue`.
- Drop `setPanel` from the destructuring.

Delete `App.tsx`, `App.module.css`, `components/IconRail.tsx`, `components/Home.tsx` and `components/Home.module.css`.

- [ ] **Step 7: Run the checks**

Run: `npm --prefix app run typecheck && npm --prefix app test`
Expected: typecheck clean. All tests pass, including the new library-store (5), useEngineQuery (2), ContextMenu (3) and Layout (4) tests, plus the updated PlayerBar tests.

Run: `npm run engine:build && npm --prefix app run e2e`
Expected: the M1 smoke test still passes. It uses "Open files" on Home, the queue list, "Signal path" (now a link) and the bit-perfect switch. If its `getByRole('button', { name: 'Signal path' })` no longer matches, change that locator to `getByRole('link', { name: 'Signal path' })`.

- [ ] **Step 8: Commit**

```bash
git add app
git commit -m "feat(app): routed layout with expandable sidebar, library and UI stores, menus, toasts and dialogs"
```

### Task 19: List machinery — paging hook, virtualized TrackTable and AlbumGrid, track menu, queue actions

**Files:**
- Create:
  - `app/src/renderer/lib/usePaged.ts`;
  - `app/src/renderer/lib/useGo.ts`;
  - `app/src/renderer/components/TrackTable.tsx` + `TrackTable.module.css`;
  - `app/src/renderer/components/AlbumGrid.tsx` + `AlbumGrid.module.css`;
  - `app/src/renderer/components/trackMenu.ts`.
- Modify:
  - `app/src/renderer/lib/format.ts`: add `formatDuration`, `trackFormat`, `albumFormat`;
  - `app/src/renderer/lib/strings.ts`;
  - `app/src/renderer/stores/player.ts`: add `playTracks`, `playNext`, `addToQueue`.
- Test: `app/test/helpers.tsx` (add the `track` fixture and `renderInRouter`), `app/test/format.test.ts` (append), `app/test/usePaged.test.tsx`, `app/test/TrackTable.test.tsx`, `app/test/trackMenu.test.ts`, `app/test/store.test.ts` (append)

**Interfaces:**
- Consumes:
  - From Task 18: `useEngineQuery`, `useUi` (menus, toasts, askName), `useLibrary` (playlists, createPlaylist, addToPlaylist), `Artwork` and the test helpers.
  - From Task 13: the `queue.*` methods.
- Produces:
  - **Formatting (`lib/format.ts`):**
    - `formatDuration(ms) -> string`: "42 min", "1 h 05 min", or "0 min" for under a minute.
    - `trackFormat(TrackRow) -> string` and `albumFormat(AlbumRow) -> string`. Examples: "FLAC 24/96", "MP3 320k", "DSD64", and "" when unknown. Both reuse `formatBadge`.
  - **`lib/usePaged.ts`:** `usePaged<T>(key: unknown[], fetchPage: (api, offset, limit) => Promise<Page<T>>, scopes?) -> { total: number; row(i): T | undefined; onRange(start, end): Promise<void>; loading: boolean }`.
    - Pages hold 200 rows; each page loads once.
    - Everything resets when `key` or a watched scope changes.
  - **`lib/useGo.ts`:** `useGo() -> (path: string) => void`, a router navigation helper for menus and cards.
  - **`<TrackTable>`:**
    - Props: `count`, `row(i)`, `onRange?`, `columns?: Column[]`, `onPlay(index)`, `extraMenu?(rows: TrackRow[]) => MenuItem[]`, `label: string`.
    - `Column = 'index' | 'title' | 'artist' | 'album' | 'format' | 'duration' | 'plays' | 'added'`.
    - Rendering: virtualized inside the nearest scrolling `<main>`, 40 px rows, `role="grid"` with `aria-rowcount`.
    - Selection:
      - Click selects; Ctrl/Cmd+click toggles; Shift+click selects a range.
      - Arrow keys move; Enter plays; the ContextMenu key or Shift+F10 opens the menu.
    - Display:
      - The now-playing track is highlighted.
      - Missing or unplayable rows are muted and show ⚠ with the reason in a tooltip (spec §6).
  - **`<VirtualGrid<T>>`:** props `label`, `count`, `row`, `onRange?`, `render(item)`, `keyOf(item)` and `height?`; it's generic. Also `<AlbumCard a={AlbumRow} />`.
  - **`<AlbumGrid>`:** a `VirtualGrid` of `AlbumCard`s, with props `count`, `row(i)`, `onRange?`, `label`. It's virtualized into responsive rows of 180 px cards. Clicking a card opens the album. The square amber button plays it. Right-click opens the album menu.
  - **`trackMenu(tracks: TrackRow[], go, extra?) -> MenuItem[]`.** The items are:
    1. Play next;
    2. Add to queue;
    3. Add to playlist ▸ (New playlist…, then each playlist);
    4. a separator;
    5. Go to album and Go to artist, enabled only when known and for a single track;
    6. Show in folder;
    7. Track info.
  - **`albumMenu(album: AlbumRow, go) -> MenuItem[]`:** Play, Play next, Add to queue, Add to playlist ▸, Go to artist.
  - **`usePlayer` gains:**
    - `playTracks(ids: number[], start?: number, opts?: { shuffle?: boolean })`: `queue.setShuffle` when `opts` is given, then `queue.set`, then `player.play`.
    - `playNext(ids)` (`queue.insertNext` plus a toast) and `addToQueue(ids)` (`queue.add` plus a toast).

- [ ] **Step 1: Write the failing tests**

Add a track fixture and a bare-router render helper to `app/test/helpers.tsx`. Test files must never import each other: an imported test file re-runs its own suites.

```tsx
import { createRootRoute, createRouter } from '@tanstack/react-router'

export const track = (id: number, title: string, extra: Record<string, unknown> = {}) => ({
  id, path: `/m/${title}.flac`, title, artist: 'Alpha', artistId: 1, album: 'Light', albumId: 2, albumArtist: null, trackNo: id, discNo: 1,
  year: 2001, genre: null, durationMs: 61_000, codec: 'flac', container: 'flac', bitrateKbps: null, sampleRate: 44100, bitDepth: 16,
  channels: 2, artworkId: null, missing: false, unplayable: null, playCount: 0, addedAt: 0, lastPlayedAt: null, ...extra,
}) as never

/** Renders `ui` alone inside a router (components that navigate need router context). */
export async function renderInRouter(ui: React.ReactNode) {
  const root = createRootRoute({ component: () => <>{ui}</> })
  const router = createRouter({ routeTree: root, history: createMemoryHistory({ initialEntries: ['/'] }) })
  await act(async () => { render(<RouterProvider router={router as never} />) })
  return { router }
}
```

Merge the `createRootRoute, createRouter` import into the existing `@tanstack/react-router` import line.

Append to `app/test/format.test.ts`:

```ts
import { albumFormat, formatDuration, trackFormat } from '../src/renderer/lib/format'

const row = (o: Partial<Parameters<typeof trackFormat>[0]>) => ({ codec: 'flac', container: 'flac', bitrateKbps: null, sampleRate: 96000, bitDepth: 24, channels: 2, ...o }) as Parameters<typeof trackFormat>[0]

describe('library formats', () => {
  it('formats track formats like the player badge', () => {
    expect(trackFormat(row({}))).toBe('FLAC 24/96')
    expect(trackFormat(row({ codec: 'mp3', container: 'mp3', bitrateKbps: 320, bitDepth: null, sampleRate: 44100 }))).toBe('MP3 320k')
    expect(trackFormat(row({ codec: 'dsd', container: 'dsf', sampleRate: 2822400, bitDepth: null }))).toBe('DSD64')
    expect(trackFormat(row({ codec: null, sampleRate: null }))).toBe('')
  })
  it('formats album formats from the most common codec and best resolution', () => {
    expect(albumFormat({ codec: 'alac', maxSampleRate: 44100, maxBitDepth: 16 } as never)).toBe('ALAC 16/44.1')
    expect(albumFormat({ codec: null, maxSampleRate: null, maxBitDepth: null } as never)).toBe('')
  })
  it('formats long durations', () => {
    expect(formatDuration(42 * 60_000 + 5_000)).toBe('42 min')
    expect(formatDuration(65 * 60_000)).toBe('1 h 05 min')
    expect(formatDuration(10_000)).toBe('0 min')
  })
})
```

`app/test/usePaged.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { usePaged } from '../src/renderer/lib/usePaged'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, resetStores } from './helpers'

afterEach(cleanup)

let hook: ReturnType<typeof usePaged<number>>
function Probe({ fetchPage }: { fetchPage: (api: unknown, o: number, l: number) => Promise<{ items: number[]; total: number }> }) {
  hook = usePaged<number>(['n'], fetchPage as never)
  return <div data-testid="total">{hook.total}</div>
}

describe('usePaged', () => {
  beforeEach(() => { resetStores(); usePlayer.setState({ api: fakeApi().api, engine: { state: 'ready' } }) })

  it('loads the first page, then only missing pages', async () => {
    const fetchPage = vi.fn(async (_a: unknown, offset: number, limit: number) => ({ items: Array.from({ length: Math.min(limit, 450 - offset) }, (_, i) => offset + i), total: 450 }))
    render(<Probe fetchPage={fetchPage} />)
    expect(await screen.findByText('450')).toBeTruthy()
    expect(hook.row(5)).toBe(5)
    expect(hook.row(300)).toBeUndefined()
    await act(async () => { await hook.onRange(250, 420) })
    expect(fetchPage).toHaveBeenCalledWith(expect.anything(), 200, 200)
    expect(fetchPage).toHaveBeenCalledWith(expect.anything(), 400, 200)
    expect(hook.row(449)).toBe(449)
    await act(async () => { await hook.onRange(0, 420) })
    expect(fetchPage).toHaveBeenCalledTimes(3)
  })
})
```

`app/test/TrackTable.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TrackTable } from '../src/renderer/components/TrackTable'
import { ContextMenu } from '../src/renderer/components/ContextMenu'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, renderInRouter, resetStores, track } from './helpers'

afterEach(cleanup)

describe('TrackTable', () => {
  beforeEach(() => { resetStores(); usePlayer.setState({ api: fakeApi().api }) })

  const rows = [track(1, 'Dawn'), track(2, 'Noon', { unplayable: 'DECODE_FAILED' }), track(3, 'Dusk', { missing: true })]
  const table = (onPlay = vi.fn()) => renderInRouter(<><TrackTable label="Tracks" count={3} row={(i) => rows[i]} onPlay={onPlay} /><ContextMenu /></>)

  it('renders rows with format and duration and plays on double-click and Enter', async () => {
    const onPlay = vi.fn()
    await table(onPlay)
    const grid = screen.getByRole('grid', { name: 'Tracks' })
    expect(grid.getAttribute('aria-rowcount')).toBe('4')
    const dawn = screen.getByText('Dawn').closest('[role=row]')!
    expect(within(dawn as HTMLElement).getByText('FLAC 16/44.1')).toBeTruthy()
    expect(within(dawn as HTMLElement).getByText('1:01')).toBeTruthy()
    fireEvent.doubleClick(dawn)
    expect(onPlay).toHaveBeenCalledWith(0)
    fireEvent.click(screen.getByText('Dusk'))
    fireEvent.keyDown(grid, { key: 'ArrowUp' })
    fireEvent.keyDown(grid, { key: 'Enter' })
    expect(onPlay).toHaveBeenLastCalledWith(1)
  })

  it('marks unplayable and missing rows', async () => {
    await table()
    expect(screen.getByTitle("Can't play: DECODE_FAILED")).toBeTruthy()
    expect(screen.getByTitle('File missing')).toBeTruthy()
  })

  it('selects ranges with shift and opens the menu for the selection', async () => {
    await table()
    fireEvent.click(screen.getByText('Dawn'))
    fireEvent.click(screen.getByText('Dusk'), { shiftKey: true })
    expect(screen.getAllByRole('row', { selected: true })).toHaveLength(3)
    fireEvent.contextMenu(screen.getByText('Noon'))
    expect(screen.getByRole('menuitem', { name: 'Play next' })).toBeTruthy()
    expect(screen.getByRole('menuitem', { name: 'Go to album' }).getAttribute('aria-disabled')).toBe('true')
  })

  it('highlights the playing track', async () => {
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, track: { path: '/m/Noon.flac', title: 'Noon', durationMs: 1, trackId: 2, artist: null, album: null, artworkId: null } } }))
    await table()
    expect(screen.getByText('Noon').closest('[role=row]')!.getAttribute('data-playing')).toBe('true')
  })
})
```

`app/test/trackMenu.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { trackMenu } from '../src/renderer/components/trackMenu'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, resetStores, track } from './helpers'

describe('trackMenu', () => {
  beforeEach(resetStores)

  it('offers queue, playlist, navigation and info actions', () => {
    useLibrary.setState({ playlists: [{ id: 9, name: 'Mix', trackCount: 0, durationMs: 0, updatedAt: 0 }] })
    const go = vi.fn()
    const items = trackMenu([track(1, 'Dawn')], go)
    expect(items.filter((i) => !i.separator).map((i) => i.label)).toEqual(['Play next', 'Add to queue', 'Add to playlist', 'Go to album', 'Go to artist', 'Show in folder', 'Track info'])
    expect(items[2].items!.map((i) => i.label)).toEqual(['New playlist…', 'Mix'])
    items.find((i) => i.label === 'Go to album')!.onSelect!()
    expect(go).toHaveBeenCalledWith('/albums/2')
    items.find((i) => i.label === 'Track info')!.onSelect!()
    expect(useUi.getState().trackInfo).toBe(1)
  })

  it('disables single-track actions for a multi-selection', () => {
    const items = trackMenu([track(1, 'Dawn'), track(2, 'Noon')], vi.fn())
    for (const label of ['Go to album', 'Go to artist', 'Show in folder', 'Track info']) expect(items.find((i) => i.label === label)!.disabled).toBe(true)
  })

  it('creates a playlist from the selection', async () => {
    const { api, calls } = fakeApi({ 'playlists.create': (p) => ({ id: 3, name: p.name, trackCount: 0, durationMs: 0, updatedAt: 0 }) })
    usePlayer.setState({ api }); useLibrary.setState({ api })
    const items = trackMenu([track(1, 'Dawn'), track(2, 'Noon')], vi.fn())
    items[2].items![0].onSelect!()
    useUi.getState().prompt!.resolve('Road trip')
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.addTracks', { id: 3, trackIds: [1, 2] }]))
  })
})
```

Append to `app/test/store.test.ts`:

```ts
describe('queue actions', () => {
  beforeEach(() => usePlayer.setState(usePlayer.getInitialState()))

  it('plays tracks by id, optionally shuffled', async () => {
    const { api } = fakeApi()
    usePlayer.getState().connect(api as any)
    await usePlayer.getState().playTracks([4, 5, 6], 1)
    expect(api.call).toHaveBeenCalledWith('queue.set', { trackIds: [4, 5, 6], startIndex: 1 })
    expect(api.call).toHaveBeenLastCalledWith('player.play', {})
    await usePlayer.getState().playTracks([4, 5, 6], 0, { shuffle: true })
    expect(api.call).toHaveBeenCalledWith('queue.setShuffle', { shuffle: true })
  })

  it('queues next and last', async () => {
    const { api } = fakeApi()
    usePlayer.getState().connect(api as any)
    await usePlayer.getState().playNext([7])
    await usePlayer.getState().addToQueue([8, 9])
    expect(api.call).toHaveBeenCalledWith('queue.insertNext', { trackIds: [7] })
    expect(api.call).toHaveBeenCalledWith('queue.add', { trackIds: [8, 9] })
  })
})
```

(`store.test.ts` has its own local `fakeApi`, which works here because it returns `null` for unknown methods.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm --prefix app test`
Expected: the new tests fail on imports (`usePaged`, `TrackTable`, `trackMenu`, `trackFormat`, …) and on `playTracks` not being a function.

- [ ] **Step 3: Implement the helpers and the store actions**

Append to `app/src/renderer/lib/format.ts`:

```ts
import type { AlbumRow } from '@protocol/AlbumRow'
import type { TrackRow } from '@protocol/TrackRow'

export function formatDuration(ms: number): string {
  const min = Math.floor(Math.max(0, ms) / 60_000)
  return min >= 60 ? `${Math.floor(min / 60)} h ${String(min % 60).padStart(2, '0')} min` : `${min} min`
}

type Fmt = Pick<TrackRow, 'codec' | 'container' | 'bitrateKbps' | 'sampleRate' | 'bitDepth' | 'channels'>

/** Library format label; DSD rows store the DSD bit rate as their sample rate. */
export function trackFormat(t: Fmt): string {
  if (!t.codec || !t.sampleRate) return ''
  return formatBadge({ container: t.container, codec: t.codec, bitrateKbps: t.bitrateKbps, sampleRate: t.sampleRate, bitDepth: t.bitDepth, channels: t.channels ?? 2, dsdRate: t.codec === 'dsd' ? t.sampleRate : null })
}

export function albumFormat(a: Pick<AlbumRow, 'codec' | 'maxSampleRate' | 'maxBitDepth'>): string {
  return trackFormat({ codec: a.codec, container: a.codec ?? '', bitrateKbps: null, sampleRate: a.maxSampleRate, bitDepth: a.maxBitDepth, channels: 2 })
}
```

Put the two type imports with the file's other imports at the top.

`albumFormat` passes `container: codec`. Only `pcm` uses the container in its label, and for a PCM album that reads "PCM 16/44.1", which is acceptable. Lossy albums have no bitrate at album level, so they show just the codec and rate (e.g. "MP3 44.1").

Add to `S` in `lib/strings.ts`:

```ts
  playNext: 'Play next',
  addToQueue: 'Add to queue',
  addToPlaylist: 'Add to playlist',
  newPlaylist: 'New playlist…',
  newPlaylistTitle: 'New playlist',
  create: 'Create',
  goToAlbum: 'Go to album',
  goToArtist: 'Go to artist',
  showInFolder: 'Show in folder',
  trackInfo: 'Track info',
  play: 'Play',
  shuffle: 'Shuffle',
  queuedNext: (n: number) => (n === 1 ? 'Playing next' : `${n} tracks playing next`),
  queuedLast: (n: number) => (n === 1 ? 'Added to queue' : `${n} tracks added to queue`),
  addedToPlaylist: (name: string) => `Added to ${name}`,
  col: { index: '#', title: 'Title', artist: 'Artist', album: 'Album', format: 'Format', duration: 'Time', plays: 'Plays', added: 'Added' },
```

`app/src/renderer/lib/useGo.ts`:

```ts
import { useRouter } from '@tanstack/react-router'
import { useCallback } from 'react'

/** Navigates to a concrete path such as `/albums/12` (menus and cards build paths, not route ids). */
export function useGo() {
  const router = useRouter()
  return useCallback((path: string) => { void router.navigate({ to: path as never }) }, [router])
}
```

`app/src/renderer/lib/usePaged.ts`:

```ts
import { useCallback, useEffect, useRef, useState } from 'react'
import type { LibraryScope } from '@protocol/LibraryScope'
import type { Page } from '@protocol/Page'
import type { AuroraApi } from '@shared/api'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'

export const PAGE = 200

/** Sparse, page-at-a-time loading for virtualized lists over `{ items, total }` queries. */
export function usePaged<T>(key: unknown[], fetchPage: (api: AuroraApi, offset: number, limit: number) => Promise<Page<T>>, scopes: LibraryScope[] = ['tracks']) {
  const api = usePlayer((p) => p.api)
  const ready = usePlayer((p) => p.engine.state === 'ready')
  const version = useLibrary((l) => scopes.reduce((n, s) => n + l.versions[s], 0))
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [, bump] = useState(0)
  const rows = useRef(new Map<number, T>())
  const requested = useRef(new Set<number>())
  const generation = useRef(0)
  const fetchRef = useRef(fetchPage)
  fetchRef.current = fetchPage
  const keyStr = JSON.stringify(key)

  const load = useCallback((page: number) => {
    if (!api || requested.current.has(page)) return Promise.resolve()
    requested.current.add(page)
    const gen = generation.current
    return fetchRef.current(api, page * PAGE, PAGE).then((p) => {
      if (gen !== generation.current) return
      p.items.forEach((item, i) => rows.current.set(page * PAGE + i, item))
      setTotal(p.total)
      setLoading(false)
      bump((n) => n + 1)
    }, () => { requested.current.delete(page); setLoading(false) })
  }, [api])

  useEffect(() => {
    if (!api || !ready) return
    generation.current++
    rows.current = new Map()
    requested.current = new Set()
    setLoading(true)
    void load(0)
  }, [api, ready, version, keyStr, load])

  const onRange = useCallback((start: number, end: number) => {
    const pages = []
    for (let p = Math.floor(start / PAGE); p <= Math.floor(Math.max(start, end) / PAGE); p++) pages.push(load(p))
    return Promise.all(pages).then(() => {})
  }, [load])

  return { total, row: (i: number) => rows.current.get(i), onRange, loading }
}
```

`onRange` returns a promise, so tests can `await` it; components ignore the return value.

In `app/src/renderer/stores/player.ts`:
- Add to the interface:

  ```ts
  playTracks(ids: number[], start?: number, opts?: { shuffle?: boolean }): Promise<void>
  playNext(ids: number[]): Promise<void>
  addToQueue(ids: number[]): Promise<void>
  ```

- Add the implementations. Import `useUi` from `./ui`; the cycle is safe because both stores only reach each other inside functions.

```ts
    async playTracks(ids, start = 0, opts) {
      if (!ids.length) return
      if (opts?.shuffle !== undefined && !(await safe(call('queue.setShuffle', { shuffle: opts.shuffle })))) return
      if (!(await safe(call('queue.set', { trackIds: ids, startIndex: Math.min(start, ids.length - 1) })))) return
      set({ error: null })
      await safe(call('player.play', {}))
    },
    async playNext(ids) {
      if (ids.length && (await safe(call('queue.insertNext', { trackIds: ids })))) useUi.getState().toast('info', S.queuedNext(ids.length))
    },
    async addToQueue(ids) {
      if (ids.length && (await safe(call('queue.add', { trackIds: ids })))) useUi.getState().toast('info', S.queuedLast(ids.length))
    },
```

(`import { S } from '../lib/strings'`.)

- [ ] **Step 4: Implement the track/album menus**

`app/src/renderer/components/trackMenu.ts`:

```ts
import type { AlbumRow } from '@protocol/AlbumRow'
import type { TrackRow } from '@protocol/TrackRow'
import { FolderOpen, Info, ListEnd, ListPlus, ListStart, Play, User, Disc3 } from 'lucide-react'
import { S } from '../lib/strings'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi, type MenuItem } from '../stores/ui'

async function newPlaylistWith(trackIds: () => Promise<number[]>) {
  const name = await useUi.getState().askName({ title: S.newPlaylistTitle, confirm: S.create })
  if (!name) return
  const lib = useLibrary.getState()
  const p = await lib.createPlaylist(name)
  if (!p) return
  await lib.addToPlaylist(p.id, await trackIds())
  useUi.getState().toast('info', S.addedToPlaylist(p.name))
}

/** "Add to playlist ▸" for tracks resolved lazily (an album menu fetches its tracks only when used). */
export function playlistSubmenu(trackIds: () => Promise<number[]>): MenuItem {
  const playlists = useLibrary.getState().playlists
  return {
    label: S.addToPlaylist, icon: ListPlus,
    items: [
      { label: S.newPlaylist, onSelect: () => void newPlaylistWith(trackIds) },
      ...playlists.map((p) => ({
        label: p.name,
        onSelect: () => void (async () => {
          await useLibrary.getState().addToPlaylist(p.id, await trackIds())
          useUi.getState().toast('info', S.addedToPlaylist(p.name))
        })(),
      })),
    ],
  }
}

export function trackMenu(tracks: TrackRow[], go: (path: string) => void, extra: MenuItem[] = []): MenuItem[] {
  const ids = tracks.map((t) => t.id)
  const one = tracks.length === 1 ? tracks[0] : null
  const player = usePlayer.getState()
  return [
    { label: S.playNext, icon: ListStart, onSelect: () => void player.playNext(ids) },
    { label: S.addToQueue, icon: ListEnd, onSelect: () => void player.addToQueue(ids) },
    playlistSubmenu(async () => ids),
    ...extra,
    { label: '', separator: true },
    { label: S.goToAlbum, icon: Disc3, disabled: one?.albumId == null, onSelect: () => one?.albumId != null && go(`/albums/${one.albumId}`) },
    { label: S.goToArtist, icon: User, disabled: one?.artistId == null, onSelect: () => one?.artistId != null && go(`/artists/${one.artistId}`) },
    { label: S.showInFolder, icon: FolderOpen, disabled: !one || one.missing, onSelect: () => one && void player.api?.showItemInFolder(one.path) },
    { label: S.trackInfo, icon: Info, disabled: !one, onSelect: () => one && useUi.getState().showTrackInfo(one.id) },
  ]
}

async function albumTrackIds(id: number): Promise<number[]> {
  const api = usePlayer.getState().api
  if (!api) return []
  return (await api.call('library.album', { id })).tracks.map((t) => t.id)
}

export function albumMenu(album: AlbumRow, go: (path: string) => void): MenuItem[] {
  const player = usePlayer.getState()
  return [
    { label: S.play, icon: Play, onSelect: () => void albumTrackIds(album.id).then((ids) => player.playTracks(ids)) },
    { label: S.playNext, icon: ListStart, onSelect: () => void albumTrackIds(album.id).then((ids) => player.playNext(ids)) },
    { label: S.addToQueue, icon: ListEnd, onSelect: () => void albumTrackIds(album.id).then((ids) => player.addToQueue(ids)) },
    playlistSubmenu(() => albumTrackIds(album.id)),
    { label: '', separator: true },
    { label: S.goToArtist, icon: User, disabled: album.artistId == null, onSelect: () => album.artistId != null && go(`/artists/${album.artistId}`) },
  ]
}
```

- [ ] **Step 5: Implement `TrackTable`**

`app/src/renderer/components/TrackTable.module.css`:

```css
.grid { position: relative; width: 100%; outline: none; }
.head, .row { display: grid; grid-template-columns: var(--cols); align-items: center; gap: 12px; padding: 0 10px; }
.head { position: sticky; top: -24px; z-index: 2; height: 34px; background: var(--bg); border-bottom: 1px solid var(--border); color: var(--text-faint); font-size: 10.5px; letter-spacing: 0.08em; text-transform: uppercase; }
.body { position: relative; }
.row { position: absolute; left: 0; right: 0; height: 40px; border-radius: var(--radius-sm); cursor: default; }
.row:hover { background: var(--raised); }
.row[aria-selected='true'] { background: var(--raised-2); }
.row[data-playing='true'] .title { color: var(--accent); }
.row[data-dim='true'] { color: var(--text-faint); }
.cell { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.title { font-weight: 500; }
.num { text-align: right; font-family: var(--font-mono); font-size: 11.5px; color: var(--text-muted); }
.fmt { font-family: var(--font-mono); font-size: 10.5px; color: var(--text-muted); }
.warn { color: var(--danger); margin-right: 6px; }
.focus { box-shadow: inset 0 0 0 1px var(--accent); }
```

`app/src/renderer/components/TrackTable.tsx`:

```tsx
import type { TrackRow } from '@protocol/TrackRow'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { formatTime, trackFormat } from '../lib/format'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { usePlayer } from '../stores/player'
import { useUi, type MenuItem } from '../stores/ui'
import { trackMenu } from './trackMenu'
import s from './TrackTable.module.css'

export type Column = 'index' | 'title' | 'artist' | 'album' | 'format' | 'duration' | 'plays' | 'added'
const WIDTHS: Record<Column, string> = { index: '40px', title: 'minmax(160px, 2fr)', artist: 'minmax(120px, 1.2fr)', album: 'minmax(120px, 1.2fr)', format: '96px', duration: '56px', plays: '52px', added: '92px' }
const ROW = 40

export interface TrackTableProps {
  label: string
  count: number
  row: (i: number) => TrackRow | undefined
  onRange?: (start: number, end: number) => void
  columns?: Column[]
  onPlay: (index: number) => void
  extraMenu?: (rows: TrackRow[]) => MenuItem[]
}

function cell(c: Column, t: TrackRow, i: number): React.ReactNode {
  switch (c) {
    case 'index': return <span className={s.num}>{t.trackNo ?? i + 1}</span>
    case 'title': return (
      <span className={`${s.cell} ${s.title}`}>
        {(t.missing || t.unplayable) && <span className={s.warn} title={t.missing ? S.missingFile : S.unplayable(t.unplayable!)} aria-label={t.missing ? S.missingFile : S.unplayable(t.unplayable!)}>⚠</span>}
        {t.title}
      </span>
    )
    case 'artist': return <span className={`${s.cell} muted`}>{t.artist || S.unknownArtist}</span>
    case 'album': return <span className={`${s.cell} muted`}>{t.album || S.unknownAlbum}</span>
    case 'format': return <span className={`${s.cell} ${s.fmt}`}>{trackFormat(t)}</span>
    case 'duration': return <span className={s.num}>{t.durationMs != null ? formatTime(t.durationMs) : ''}</span>
    case 'plays': return <span className={s.num}>{t.playCount || ''}</span>
    case 'added': return <span className={`${s.cell} ${s.fmt}`}>{new Date(t.addedAt * 1000).toLocaleDateString()}</span>
  }
}

export function TrackTable({ label, count, row, onRange, columns = ['index', 'title', 'artist', 'album', 'format', 'duration'], onPlay, extraMenu }: TrackTableProps) {
  const ref = useRef<HTMLDivElement>(null)
  const [scrollEl, setScrollEl] = useState<HTMLElement | null>(null)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [focus, setFocus] = useState(0)
  const anchor = useRef(0)
  const go = useGo()
  const playingId = usePlayer((p) => p.state.track?.trackId ?? null)
  useLayoutEffect(() => { setScrollEl((ref.current?.closest('main') as HTMLElement | null) ?? ref.current) }, [])
  const v = useVirtualizer({ count, getScrollElement: () => scrollEl, estimateSize: () => ROW, overscan: 12, scrollMargin: ref.current?.offsetTop ?? 0 })
  const items = v.getVirtualItems()
  const first = items[0]?.index ?? 0
  const last = items.at(-1)?.index ?? 0
  useEffect(() => { if (count) onRange?.(first, last) }, [first, last, count, onRange])
  useEffect(() => { setSelected(new Set()); setFocus(0) }, [count])

  const select = (i: number, e: { shiftKey?: boolean; ctrlKey?: boolean; metaKey?: boolean }) => {
    setFocus(i)
    if (e.shiftKey) {
      const [a, b] = [Math.min(anchor.current, i), Math.max(anchor.current, i)]
      setSelected(new Set(Array.from({ length: b - a + 1 }, (_, k) => a + k)))
      return
    }
    anchor.current = i
    if (e.ctrlKey || e.metaKey) setSelected((prev) => { const n = new Set(prev); if (n.has(i)) n.delete(i); else n.add(i); return n })
    else setSelected(new Set([i]))
  }

  const openMenu = (i: number, x: number, y: number) => {
    const indices = selected.has(i) ? [...selected].sort((a, b) => a - b) : [i]
    if (!selected.has(i)) select(i, {})
    const rows = indices.map(row).filter((r): r is TrackRow => !!r)
    if (rows.length) useUi.getState().openMenu(x, y, trackMenu(rows, go, extraMenu?.(rows)))
  }

  const onKey = (e: React.KeyboardEvent) => {
    if (!count) return
    const move = (to: number) => { e.preventDefault(); const n = Math.max(0, Math.min(count - 1, to)); select(n, { shiftKey: e.shiftKey }); v.scrollToIndex(n) }
    if (e.key === 'ArrowDown') move(focus + 1)
    else if (e.key === 'ArrowUp') move(focus - 1)
    else if (e.key === 'Home') move(0)
    else if (e.key === 'End') move(count - 1)
    else if (e.key === 'Enter') { e.preventDefault(); onPlay(focus) }
    else if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
      e.preventDefault()
      const r = (ref.current?.querySelector(`[data-index="${focus}"]`) as HTMLElement | null)?.getBoundingClientRect()
      openMenu(focus, r ? r.left + 40 : 100, r ? r.bottom : 100)
    }
  }

  const cols = columns.map((c) => WIDTHS[c]).join(' ')
  return (
    <div ref={ref} className={s.grid} role="grid" aria-label={label} aria-rowcount={count + 1} aria-multiselectable tabIndex={0} onKeyDown={onKey} style={{ ['--cols' as string]: cols }}>
      <div className={s.head} role="row" aria-rowindex={1}>
        {columns.map((c) => <span key={c} role="columnheader" className={c === 'duration' || c === 'plays' || c === 'index' ? s.num : undefined}>{S.col[c]}</span>)}
      </div>
      <div className={s.body} style={{ height: v.getTotalSize() }}>
        {items.map((it) => {
          const t = row(it.index)
          const top = it.start - v.options.scrollMargin
          if (!t) return <div key={it.key} className={s.row} style={{ transform: `translateY(${top}px)` }} role="row" aria-rowindex={it.index + 2} aria-busy="true" />
          return (
            <div
              key={it.key} data-index={it.index} role="row" aria-rowindex={it.index + 2} aria-selected={selected.has(it.index)}
              data-playing={t.id === playingId} data-dim={t.missing || !!t.unplayable}
              className={`${s.row} ${focus === it.index ? s.focus : ''}`} style={{ transform: `translateY(${top}px)` }}
              onClick={(e) => select(it.index, e)} onDoubleClick={() => onPlay(it.index)}
              onContextMenu={(e) => { e.preventDefault(); openMenu(it.index, e.clientX, e.clientY) }}
            >
              {columns.map((c) => <span key={c} role="gridcell" className={s.cell}>{cell(c, t, it.index)}</span>)}
            </div>
          )
        })}
      </div>
    </div>
  )
}
```

- [ ] **Step 6: Implement `AlbumGrid`**

`app/src/renderer/components/AlbumGrid.module.css`:

```css
.grid { position: relative; width: 100%; }
.row { position: absolute; left: 0; right: 0; display: grid; gap: 20px; }
.card { position: relative; display: flex; flex-direction: column; gap: 8px; padding: 10px; border-radius: var(--radius); text-align: left; transition: background var(--dur) var(--ease); }
.card:hover, .card:focus-within { background: var(--raised); }
.cover { position: relative; aspect-ratio: 1; }
.cover > :first-child { width: 100% !important; height: 100% !important; }
.play { position: absolute; right: 8px; bottom: 8px; width: 38px; height: 38px; display: grid; place-items: center; border-radius: var(--radius); background: var(--accent); color: #17151a; opacity: 0; transform: translateY(4px); transition: opacity var(--dur) var(--ease), transform var(--dur) var(--ease); }
.card:hover .play, .card:focus-within .play { opacity: 1; transform: none; }
.title { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sub { color: var(--text-muted); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.fmt { font-family: var(--font-mono); font-size: 10px; color: var(--text-faint); }
```

`app/src/renderer/components/AlbumGrid.tsx`:

```tsx
import type { AlbumRow } from '@protocol/AlbumRow'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Play } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { albumFormat } from '../lib/format'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import { Artwork } from './Artwork'
import { albumMenu } from './trackMenu'
import s from './AlbumGrid.module.css'

const CARD = 180
const GAP = 20
const ROW_H = CARD + 76

export function AlbumCard({ a }: { a: AlbumRow }) {
  const go = useGo()
  const play = async (e: React.MouseEvent) => {
    e.stopPropagation()
    const api = usePlayer.getState().api
    if (!api) return
    const d = await api.call('library.album', { id: a.id })
    await usePlayer.getState().playTracks(d.tracks.map((t) => t.id))
  }
  return (
    <div className={s.card} role="listitem" onClick={() => go(`/albums/${a.id}`)}
      onContextMenu={(e) => { e.preventDefault(); useUi.getState().openMenu(e.clientX, e.clientY, albumMenu(a, go)) }}>
      <div className={s.cover}>
        <Artwork id={a.artworkId} size={300} px={CARD} alt="" />
        <button className={s.play} aria-label={`${S.play} ${a.title || S.unknownAlbum}`} onClick={(e) => void play(e)}><Play size={16} fill="currentColor" /></button>
      </div>
      <a href={`#/albums/${a.id}`} className={s.title} onClick={(e) => { e.preventDefault(); go(`/albums/${a.id}`) }} style={{ color: 'inherit', textDecoration: 'none' }}>{a.title || S.unknownAlbum}</a>
      <div className={s.sub}>{a.artist || S.unknownArtist}{a.year ? ` · ${a.year}` : ''}</div>
      <div className={s.fmt}>{albumFormat(a)}</div>
    </div>
  )
}

export interface VirtualGridProps<T> {
  label: string
  count: number
  row: (i: number) => T | undefined
  onRange?: (start: number, end: number) => void
  render: (item: T) => React.ReactNode
  keyOf: (item: T) => React.Key
  /** Card height in px, excluding the gap. */
  height?: number
}

/** Responsive rows of 180 px cards, virtualized inside the nearest scrolling `<main>`. */
export function VirtualGrid<T>({ label, count, row, onRange, render, keyOf, height = ROW_H }: VirtualGridProps<T>) {
  const ref = useRef<HTMLDivElement>(null)
  const [scrollEl, setScrollEl] = useState<HTMLElement | null>(null)
  const [width, setWidth] = useState(1000)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    setScrollEl((el.closest('main') as HTMLElement | null) ?? el)
    setWidth(el.getBoundingClientRect().width)
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const cols = Math.max(1, Math.floor((width + GAP) / (CARD + GAP)))
  const rowCount = Math.ceil(count / cols)
  const v = useVirtualizer({ count: rowCount, getScrollElement: () => scrollEl, estimateSize: () => height + GAP, overscan: 3, scrollMargin: ref.current?.offsetTop ?? 0 })
  const vis = v.getVirtualItems()
  const first = (vis[0]?.index ?? 0) * cols
  const last = Math.min(count - 1, ((vis.at(-1)?.index ?? 0) + 1) * cols - 1)
  useEffect(() => { if (count) onRange?.(first, last) }, [first, last, count, onRange])
  return (
    <div ref={ref} className={s.grid} role="list" aria-label={label} style={{ height: v.getTotalSize() }}>
      {vis.map((r) => (
        <div key={r.key} className={s.row} style={{ transform: `translateY(${r.start - v.options.scrollMargin}px)`, gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}>
          {Array.from({ length: cols }, (_, c) => {
            const i = r.index * cols + c
            if (i >= count) return null
            const item = row(i)
            return item ? <div key={keyOf(item)}>{render(item)}</div> : <div key={`p${i}`} className={s.card} aria-busy="true" />
          })}
        </div>
      ))}
    </div>
  )
}

export function AlbumGrid(props: { label: string; count: number; row: (i: number) => AlbumRow | undefined; onRange?: (start: number, end: number) => void }) {
  return <VirtualGrid {...props} render={(a) => <AlbumCard a={a} />} keyOf={(a) => a.id} />
}
```

- [ ] **Step 7: Run the checks**

Run: `npm --prefix app run typecheck && npm --prefix app test`
Expected: all pass, including:
- format (3 new);
- usePaged (1);
- TrackTable (4);
- trackMenu (3);
- the store queue actions (2).

- [ ] **Step 8: Commit**

```bash
git add app
git commit -m "feat(app): paged virtualized track table and album grid, track/album menus and queue actions"
```

### Task 20: Library views — Home, Albums, Album, Artists, Artist, Genres, Genre

**Files:**
- Modify:
  - `app/src/renderer/router.tsx`: six routes;
  - `app/src/renderer/lib/nav.ts`: Albums, Artists, Genres;
  - `app/src/renderer/lib/strings.ts`;
  - `app/test/helpers.tsx`: an `album` fixture.
- Replace: `app/src/renderer/views/HomeView.tsx` and `HomeView.module.css`.
- Create:
  - `app/src/renderer/views/views.module.css`, the shared view styles;
  - views `AlbumsView.tsx`, `AlbumView.tsx`, `ArtistsView.tsx`, `ArtistView.tsx`, `GenresView.tsx`, `GenreView.tsx`.
- Test: `app/test/views-library.test.tsx`

**Interfaces:**
- Consumes:
  - From Task 19: `usePaged`, `TrackTable`, `VirtualGrid`, `AlbumGrid`, `AlbumCard`, `albumMenu`, `useGo`, `formatDuration`, `albumFormat` and `usePlayer().playTracks`.
  - From Task 18: `useEngineQuery`, `useLibrary` (folders, scan, addFolder) and `Artwork`.
  - From Task 13: `library.*` methods.
- Produces:
  - **Routes:** `/albums`, `/albums/$albumId`, `/artists`, `/artists/$artistId`, `/genres`, `/genres/$genreId`. `NAV` order becomes Home, Albums, Artists, Genres, Signal path.
  - **Shared view classes in `views/views.module.css`:** `view`, `head`, `kicker`, `title`, `meta`, `actions`, `primary`, `ghost`, `section`, `sectionTitle`, `row`, `empty`, `select`. Tasks 21–22 reuse them.
  - **Home:**
    - With no folders it shows the empty state: "Add a music folder", "Open files", and a drop zone for M1-style direct playback.
    - Otherwise it shows Recently added (12 albums), Recently played (8 tracks), Most played (8 tracks) and scan progress while a scan runs.
  - **Album detail** (spec §5.3 and the §5.5 dynamic colour): the header gradient runs from the album palette's muted colour, mixed 55 % into `--bg`, down to `--bg`. It has Play, Shuffle and Add to queue, plus the format/bit-depth/rate readout in mono.
  - **Artist detail:** Play (all the artist's tracks, ordered by album), Top tracks (by play count) and an Albums grid.
  - **Test fixture:** `album(id, title, extra?)` in `test/helpers.tsx`.

- [ ] **Step 1: Write the failing tests**

Add to `app/test/helpers.tsx`:

```tsx
export const album = (id: number, title: string, extra: Record<string, unknown> = {}) => ({
  id, title, artist: 'Alpha', artistId: 1, year: 2001, trackCount: 2, durationMs: 122_000, artworkId: 5, addedAt: 0,
  codec: 'flac', maxSampleRate: 96000, maxBitDepth: 24, ...extra,
}) as never
```

`app/test/views-library.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { album, fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

const folder = { id: 1, path: '/music', trackCount: 3, lastScanAt: 1, available: true }

describe('home', () => {
  it('invites the user to add a folder when the library is empty', async () => {
    const { api } = fakeApi()
    await renderAt('/', api)
    fireEvent.click(await screen.findByRole('button', { name: 'Add a music folder' }))
    await vi.waitFor(() => expect(api.openFolder).toHaveBeenCalled())
    expect(screen.getByRole('button', { name: 'Open files' })).toBeTruthy()
  })

  it('shows recently added albums and plays recently played tracks', async () => {
    const { api, calls } = fakeApi({
      'library.folders': () => [folder],
      'library.recent': (p) => (p.kind === 'added' ? { albums: [album(5, 'First Light')], tracks: [] } : { albums: [], tracks: p.kind === 'played' ? [track(7, 'Dawn'), track(8, 'Noon')] : [] }),
    })
    await renderAt('/', api)
    expect(await screen.findByText('First Light')).toBeTruthy()
    const played = await screen.findByRole('grid', { name: 'Recently played' })
    fireEvent.doubleClick(within(played).getByText('Noon'))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [7, 8], startIndex: 1 }]))
  })

  it('shows scan progress while scanning', async () => {
    const { api, emit } = fakeApi({ 'library.folders': () => [folder] })
    await renderAt('/', api)
    emit('library.scanProgress', { folderId: 1, phase: 'reading', processed: 40, total: 100, added: 40, updated: 0, missing: 0 })
    expect(await screen.findByText('Scanning 40 / 100')).toBeTruthy()
  })
})

describe('albums', () => {
  it('lists albums and re-queries when the sort changes', async () => {
    const { api, calls } = fakeApi({ 'library.albums': () => ({ items: [album(5, 'First Light'), album(6, 'Zenith', { artworkId: null })], total: 2 }) })
    await renderAt('/albums', api)
    expect(await screen.findByText('Zenith')).toBeTruthy()
    expect(screen.getByText('2 albums')).toBeTruthy()
    fireEvent.change(screen.getByRole('combobox', { name: 'Sort albums' }), { target: { value: 'artist' } })
    await vi.waitFor(() => expect(calls.some(([m, p]) => m === 'library.albums' && p.sort === 'artist')).toBe(true))
  })

  it('opens an album from its card', async () => {
    const { api } = fakeApi({ 'library.albums': () => ({ items: [album(5, 'First Light')], total: 1 }), 'library.album': () => ({ album: album(5, 'First Light'), tracks: [], genres: [] }) })
    const { router } = await renderAt('/albums', api)
    fireEvent.click(await screen.findByText('First Light'))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/albums/5'))
  })
})

describe('album detail', () => {
  const detail = { album: album(5, 'First Light'), tracks: [track(1, 'Dawn'), track(2, 'Noon')], genres: [{ id: 3, name: 'Ambient' }] }

  it('shows the header with format readout and dynamic colour, and plays the album', async () => {
    const { api, calls } = fakeApi({ 'library.album': () => detail, 'library.artwork': () => ({ path: '/a.webp', palette: { dominant: '#112233', vibrant: '#ff0000', muted: '#445566' } }) })
    await renderAt('/albums/5', api)
    expect(await screen.findByRole('heading', { name: 'First Light' })).toBeTruthy()
    expect(screen.getByText('FLAC 24/96')).toBeTruthy()
    await vi.waitFor(() => expect(screen.getByTestId('album-head').getAttribute('style')).toContain('#445566'))
    fireEvent.click(screen.getByRole('button', { name: 'Play album' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [1, 2], startIndex: 0 }]))
    fireEvent.click(screen.getByRole('button', { name: 'Shuffle album' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setShuffle', { shuffle: true }]))
  })

  it('reports an unknown album', async () => {
    const { api } = fakeApi({ 'library.album': () => { throw new Error('album 99 not found') } })
    await renderAt('/albums/99', api)
    expect(await screen.findByText('album 99 not found')).toBeTruthy()
  })
})

describe('artists and genres', () => {
  it('shows an artist with top tracks and albums, and plays everything', async () => {
    const { api, calls } = fakeApi({
      'library.artist': () => ({ artist: { id: 1, name: 'Alpha', albumCount: 1, trackCount: 2, artworkId: null }, albums: [album(5, 'First Light')], topTracks: [track(1, 'Dawn', { playCount: 4 })] }),
      'library.tracks': () => ({ items: [track(1, 'Dawn'), track(2, 'Noon')], total: 2 }),
    })
    await renderAt('/artists/1', api)
    expect(await screen.findByRole('heading', { name: 'Alpha' })).toBeTruthy()
    expect(within(screen.getByRole('grid', { name: 'Top tracks' })).getByText('Dawn')).toBeTruthy()
    expect(screen.getByText('First Light')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Play artist' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [1, 2], startIndex: 0 }]))
    expect(calls.find(([m]) => m === 'library.tracks')![1]).toMatchObject({ filter: { artistId: 1 }, sort: 'album' })
  })

  it('lists artists', async () => {
    const { api } = fakeApi({ 'library.artists': () => ({ items: [{ id: 1, name: 'Alpha', albumCount: 2, trackCount: 9, artworkId: null }], total: 1 }) })
    await renderAt('/artists', api)
    expect(await screen.findByText('Alpha')).toBeTruthy()
    expect(screen.getByText('2 albums · 9 tracks')).toBeTruthy()
  })

  it('goes from a genre to its albums', async () => {
    const { api, calls } = fakeApi({
      'library.genres': () => ({ items: [{ id: 3, name: 'Ambient', albumCount: 1, trackCount: 2 }], total: 1 }),
      'library.genre': () => ({ id: 3, name: 'Ambient', albumCount: 1, trackCount: 2 }),
      'library.albums': () => ({ items: [album(5, 'First Light')], total: 1 }),
    })
    const { router } = await renderAt('/genres', api)
    fireEvent.click(await screen.findByText('Ambient'))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/genres/3'))
    expect(await screen.findByText('First Light')).toBeTruthy()
    expect(calls.some(([m, p]) => m === 'library.albums' && p.filter?.genreId === 3)).toBe(true)
  })

  it('adds the library sections to the sidebar', async () => {
    await renderAt('/albums')
    for (const name of ['Home', 'Albums', 'Artists', 'Genres', 'Signal path']) expect(screen.getByRole('link', { name })).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Albums' }).getAttribute('aria-current')).toBe('page')
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm --prefix app test -- views-library`
Expected: failures. `/albums` and the other new routes render the not-found view, and Home has no "Add a music folder" button.

- [ ] **Step 3: Add the strings, nav entries and shared styles**

Add to `S`:

```ts
  homeTitle: 'Home',
  emptyLibrary: 'Your library is empty',
  emptyLibraryHint: 'Add the folders where your music lives. Aurora watches them and keeps the library up to date.',
  addFolder: 'Add a music folder',
  openFiles: 'Open files',
  dropHint: 'Or drop audio files here to play them directly.',
  recentlyAdded: 'Recently added',
  recentlyPlayed: 'Recently played',
  mostPlayed: 'Most played',
  albumsTitle: 'Albums',
  artistsTitle: 'Artists',
  genresTitle: 'Genres',
  albumCount: (n: number) => `${n} ${n === 1 ? 'album' : 'albums'}`,
  trackCount: (n: number) => `${n} ${n === 1 ? 'track' : 'tracks'}`,
  sortAlbums: 'Sort albums',
  sortBy: { title: 'Title', artist: 'Artist', year: 'Year', addedAt: 'Date added' },
  ascending: 'Ascending',
  descending: 'Descending',
  album: 'Album',
  artist: 'Artist',
  genre: 'Genre',
  topTracks: 'Top tracks',
  nothingYet: 'Nothing here yet.',
  playAlbum: 'Play album',
  shuffleAlbum: 'Shuffle album',
  playArtist: 'Play artist',
```

In `lib/nav.ts`:
- Import `Disc3, Tags, Users` from `lucide-react`.
- Make `NAV` this:

```ts
export const NAV: NavItem[] = [
  { to: '/', label: S.nav.home, icon: House },
  { to: '/albums', label: S.nav.albums, icon: Disc3 },
  { to: '/artists', label: S.nav.artists, icon: Users },
  { to: '/genres', label: S.nav.genres, icon: Tags },
  { to: '/signal', label: S.nav.signal, icon: Activity },
]
```

`app/src/renderer/views/views.module.css`:

```css
.view { display: flex; flex-direction: column; gap: 26px; }
.head { display: flex; align-items: flex-end; gap: 22px; padding: 22px; margin: -24px -28px 0; }
.kicker { font-family: var(--font-mono); font-size: 10px; letter-spacing: 0.12em; color: var(--text-muted); }
.title { margin: 4px 0 6px; font-size: 32px; font-weight: 800; letter-spacing: -0.015em; line-height: 1.1; }
.meta { font-family: var(--font-mono); font-size: 11px; color: var(--text-muted); }
.meta a { color: var(--text); text-decoration: none; }
.meta a:hover { color: var(--accent); }
.actions { display: flex; align-items: center; gap: 10px; }
.primary { display: inline-flex; align-items: center; gap: 8px; height: 38px; padding: 0 16px; border-radius: var(--radius); background: var(--accent); color: #17151a; font-weight: 700; }
.ghost { display: inline-flex; align-items: center; gap: 8px; height: 38px; padding: 0 14px; border-radius: var(--radius); border: 1px solid var(--border); color: var(--text); }
.ghost:hover { border-color: var(--text-muted); }
.section { display: flex; flex-direction: column; gap: 10px; }
.sectionTitle { margin: 0; font-size: 18px; font-weight: 700; }
.row { display: flex; gap: 4px; overflow-x: auto; padding-bottom: 6px; }
.row > * { flex: 0 0 200px; }
.empty { display: flex; flex-direction: column; align-items: flex-start; gap: 12px; padding: 40px; border: 1px dashed var(--border); border-radius: var(--radius); }
.empty h2 { margin: 0; font-size: 20px; }
.select { height: 32px; padding: 0 8px; color: var(--text); background: var(--raised); border: 1px solid var(--border); border-radius: var(--radius-sm); font: inherit; }
.toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.tile { display: flex; flex-direction: column; gap: 6px; padding: 16px; height: 96px; border-radius: var(--radius); background: var(--raised); text-align: left; width: 100%; }
.tile:hover { background: var(--raised-2); }
.tileName { font-weight: 700; font-size: 15px; }
.error { color: var(--danger); }
```

- [ ] **Step 4: Implement the views**

`app/src/renderer/views/HomeView.tsx` (replaces M1's queue screen; the queue gets its own view in Task 21):

```tsx
import { FolderOpen, FolderPlus } from 'lucide-react'
import { AlbumCard } from '../components/AlbumGrid'
import { TrackTable } from '../components/TrackTable'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import v from './views.module.css'

function RecentTracks({ kind, title }: { kind: 'played' | 'mostPlayed'; title: string }) {
  const q = useEngineQuery(['recent', kind], (a) => a.call('library.recent', { kind, limit: 8 }), ['tracks', 'history'])
  const tracks = q.data?.tracks ?? []
  if (!tracks.length) return null
  return (
    <section className={v.section}>
      <h2 className={v.sectionTitle}>{title}</h2>
      <TrackTable label={title} count={tracks.length} row={(i) => tracks[i]} columns={kind === 'mostPlayed' ? ['title', 'artist', 'album', 'plays', 'duration'] : ['title', 'artist', 'album', 'duration']}
        onPlay={(i) => void usePlayer.getState().playTracks(tracks.map((t) => t.id), i)} />
    </section>
  )
}

export function HomeView() {
  const folders = useLibrary((l) => l.folders)
  const scan = useLibrary((l) => l.scan)
  const { openFiles, playPaths, api } = usePlayer.getState()
  const added = useEngineQuery(['recent', 'added'], (a) => a.call('library.recent', { kind: 'added', limit: 12 }))
  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const paths = api?.pathsForFiles([...e.dataTransfer.files]) ?? []
    if (paths.length) void playPaths(paths)
  }
  const scanning = scan && scan.phase !== 'done'
  return (
    <div className={v.view} onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <div className={v.toolbar}>
        <h1 className={v.title}>{S.homeTitle}</h1>
        {scanning && <span className={v.meta} role="status">{S.scanning(scan.processed, scan.total)}</span>}
      </div>
      {folders.length === 0 ? (
        <div className={v.empty}>
          <h2>{S.emptyLibrary}</h2>
          <p className="muted">{S.emptyLibraryHint}</p>
          <div className={v.actions}>
            <button className={v.primary} onClick={() => void useLibrary.getState().addFolder()}><FolderPlus size={16} /> {S.addFolder}</button>
            <button className={v.ghost} onClick={() => void openFiles()}><FolderOpen size={16} /> {S.openFiles}</button>
          </div>
          <p className="muted">{S.dropHint}</p>
        </div>
      ) : (
        <>
          {!!added.data?.albums.length && (
            <section className={v.section}>
              <h2 className={v.sectionTitle}>{S.recentlyAdded}</h2>
              <div className={v.row} role="list" aria-label={S.recentlyAdded}>{added.data.albums.map((a) => <AlbumCard key={a.id} a={a} />)}</div>
            </section>
          )}
          <RecentTracks kind="played" title={S.recentlyPlayed} />
          <RecentTracks kind="mostPlayed" title={S.mostPlayed} />
        </>
      )}
    </div>
  )
}
```

Delete `views/HomeView.module.css`; the shared `views.module.css` replaces it.

`app/src/renderer/views/AlbumsView.tsx`:

```tsx
import type { SortKey } from '@protocol/SortKey'
import { ArrowDownWideNarrow, ArrowUpNarrowWide } from 'lucide-react'
import { useState } from 'react'
import { AlbumGrid } from '../components/AlbumGrid'
import { S } from '../lib/strings'
import { usePaged } from '../lib/usePaged'
import v from './views.module.css'

const SORTS: SortKey[] = ['title', 'artist', 'year', 'addedAt']

export function AlbumsView() {
  const [sort, setSort] = useState<SortKey>('title')
  const [desc, setDesc] = useState(false)
  const paged = usePaged(['albums', sort, desc], (a, offset, limit) => a.call('library.albums', { sort, desc, offset, limit }))
  return (
    <div className={v.view}>
      <div className={v.toolbar}>
        <div>
          <h1 className={v.title}>{S.albumsTitle}</h1>
          <div className={v.meta}>{S.albumCount(paged.total)}</div>
        </div>
        <div className={v.actions}>
          <select className={v.select} aria-label={S.sortAlbums} value={sort}
            onChange={(e) => { const k = e.target.value as SortKey; setSort(k); setDesc(k === 'year' || k === 'addedAt') }}>
            {SORTS.map((k) => <option key={k} value={k}>{S.sortBy[k as keyof typeof S.sortBy]}</option>)}
          </select>
          <button className={v.ghost} aria-label={desc ? S.descending : S.ascending} title={desc ? S.descending : S.ascending} onClick={() => setDesc(!desc)}>
            {desc ? <ArrowDownWideNarrow size={16} /> : <ArrowUpNarrowWide size={16} />}
          </button>
        </div>
      </div>
      <AlbumGrid label={S.albumsTitle} count={paged.total} row={paged.row} onRange={paged.onRange} />
    </div>
  )
}
```

`app/src/renderer/views/AlbumView.tsx`:

```tsx
import { useParams } from '@tanstack/react-router'
import { ListEnd, MoreHorizontal, Play, Shuffle } from 'lucide-react'
import { Artwork } from '../components/Artwork'
import { TrackTable, type Column } from '../components/TrackTable'
import { albumMenu } from '../components/trackMenu'
import { albumFormat, formatDuration } from '../lib/format'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useGo } from '../lib/useGo'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export function AlbumView() {
  const { albumId } = useParams({ strict: false }) as { albumId?: string }
  const id = Number(albumId)
  const go = useGo()
  const q = useEngineQuery(['album', id], (a) => a.call('library.album', { id }))
  const artId = q.data?.album.artworkId ?? null
  const art = useEngineQuery(['artwork', artId], (a) => (artId == null ? Promise.resolve(null) : a.call('library.artwork', { id: artId, size: 300 })), [])
  if (q.error) return <p className={v.error}>{q.error}</p>
  if (!q.data) return null
  const { album: al, tracks } = q.data
  const ids = tracks.map((t) => t.id)
  const muted = art.data?.palette?.muted
  const various = tracks.some((t) => t.artist !== al.artist)
  const columns: Column[] = various ? ['index', 'title', 'artist', 'format', 'duration'] : ['index', 'title', 'format', 'duration']
  const player = usePlayer.getState()
  return (
    <div className={v.view}>
      <header data-testid="album-head" className={v.head}
        style={muted ? { background: `linear-gradient(180deg, color-mix(in srgb, ${muted} 55%, var(--bg)) 0%, var(--bg) 100%)` } : undefined}>
        <Artwork id={al.artworkId} size={1000} px={220} alt={al.title} />
        <div>
          <div className={v.kicker}>{S.album.toUpperCase()}</div>
          <h1 className={v.title}>{al.title || S.unknownAlbum}</h1>
          <div className={v.meta}>
            {al.artistId != null ? <a href={`#/artists/${al.artistId}`} onClick={(e) => { e.preventDefault(); go(`/artists/${al.artistId}`) }}>{al.artist}</a> : (al.artist || S.unknownArtist)}
            {al.year ? ` · ${al.year}` : ''} · {S.trackCount(al.trackCount)} · {formatDuration(al.durationMs)}
          </div>
          <div className={v.meta} style={{ marginTop: 4 }}>{albumFormat(al)}</div>
          <div className={v.actions} style={{ marginTop: 16 }}>
            <button className={v.primary} aria-label={S.playAlbum} onClick={() => void player.playTracks(ids, 0)}><Play size={16} fill="currentColor" /> {S.play}</button>
            <button className={v.ghost} aria-label={S.shuffleAlbum} onClick={() => void player.playTracks(ids, Math.floor(Math.random() * ids.length), { shuffle: true })}><Shuffle size={16} /> {S.shuffle}</button>
            <button className={v.ghost} onClick={() => void player.addToQueue(ids)}><ListEnd size={16} /> {S.addToQueue}</button>
            <button className={v.ghost} aria-label="More" onClick={(e) => { const r = e.currentTarget.getBoundingClientRect(); useUi.getState().openMenu(r.left, r.bottom + 4, albumMenu(al, go)) }}><MoreHorizontal size={16} /></button>
          </div>
        </div>
      </header>
      <TrackTable label={al.title || S.unknownAlbum} count={tracks.length} row={(i) => tracks[i]} columns={columns} onPlay={(i) => void player.playTracks(ids, i)} />
    </div>
  )
}
```

`app/src/renderer/views/ArtistsView.tsx`:

```tsx
import type { ArtistRow } from '@protocol/ArtistRow'
import { VirtualGrid } from '../components/AlbumGrid'
import { Artwork } from '../components/Artwork'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { usePaged } from '../lib/usePaged'
import g from '../components/AlbumGrid.module.css'
import v from './views.module.css'

function ArtistCard({ a }: { a: ArtistRow }) {
  const go = useGo()
  return (
    <div className={g.card} role="listitem" onClick={() => go(`/artists/${a.id}`)}>
      <div className={g.cover}><Artwork id={a.artworkId} size={300} px={180} alt="" /></div>
      <a href={`#/artists/${a.id}`} className={g.title} style={{ color: 'inherit', textDecoration: 'none' }} onClick={(e) => { e.preventDefault(); go(`/artists/${a.id}`) }}>{a.name}</a>
      <div className={g.sub}>{S.albumCount(a.albumCount)} · {S.trackCount(a.trackCount)}</div>
    </div>
  )
}

export function ArtistsView() {
  const paged = usePaged(['artists'], (a, offset, limit) => a.call('library.artists', { offset, limit }))
  return (
    <div className={v.view}>
      <div>
        <h1 className={v.title}>{S.artistsTitle}</h1>
        <div className={v.meta}>{paged.total} {S.artistsTitle.toLowerCase()}</div>
      </div>
      <VirtualGrid label={S.artistsTitle} count={paged.total} row={paged.row} onRange={paged.onRange} render={(a) => <ArtistCard a={a} />} keyOf={(a) => a.id} height={240} />
    </div>
  )
}
```

`app/src/renderer/views/ArtistView.tsx`:

```tsx
import { useParams } from '@tanstack/react-router'
import { Play } from 'lucide-react'
import { AlbumCard } from '../components/AlbumGrid'
import { TrackTable } from '../components/TrackTable'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { usePlayer } from '../stores/player'
import v from './views.module.css'

export function ArtistView() {
  const { artistId } = useParams({ strict: false }) as { artistId?: string }
  const id = Number(artistId)
  const q = useEngineQuery(['artist', id], (a) => a.call('library.artist', { id }), ['tracks', 'history'])
  if (q.error) return <p className={v.error}>{q.error}</p>
  if (!q.data) return null
  const { artist, albums, topTracks } = q.data
  const playAll = async () => {
    const api = usePlayer.getState().api
    if (!api) return
    const all = await api.call('library.tracks', { filter: { artistId: id }, sort: 'album', limit: 1000 })
    await usePlayer.getState().playTracks(all.items.map((t) => t.id))
  }
  return (
    <div className={v.view}>
      <header className={v.head} style={{ alignItems: 'center' }}>
        <div>
          <div className={v.kicker}>{S.artist.toUpperCase()}</div>
          <h1 className={v.title}>{artist.name}</h1>
          <div className={v.meta}>{S.albumCount(artist.albumCount)} · {S.trackCount(artist.trackCount)}</div>
          <div className={v.actions} style={{ marginTop: 14 }}>
            <button className={v.primary} aria-label={S.playArtist} onClick={() => void playAll()}><Play size={16} fill="currentColor" /> {S.play}</button>
          </div>
        </div>
      </header>
      {topTracks.length > 0 && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.topTracks}</h2>
          <TrackTable label={S.topTracks} count={topTracks.length} row={(i) => topTracks[i]} columns={['title', 'album', 'plays', 'duration']}
            onPlay={(i) => void usePlayer.getState().playTracks(topTracks.map((t) => t.id), i)} />
        </section>
      )}
      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.albumsTitle}</h2>
        <div className={v.row} role="list" aria-label={S.albumsTitle} style={{ flexWrap: 'wrap' }}>{albums.map((a) => <AlbumCard key={a.id} a={a} />)}</div>
      </section>
    </div>
  )
}
```

`app/src/renderer/views/GenresView.tsx`:

```tsx
import type { GenreRow } from '@protocol/GenreRow'
import { VirtualGrid } from '../components/AlbumGrid'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { usePaged } from '../lib/usePaged'
import v from './views.module.css'

function GenreTile({ g }: { g: GenreRow }) {
  const go = useGo()
  return (
    <button className={v.tile} role="listitem" onClick={() => go(`/genres/${g.id}`)}>
      <span className={v.tileName}>{g.name}</span>
      <span className={v.meta}>{S.albumCount(g.albumCount)} · {S.trackCount(g.trackCount)}</span>
    </button>
  )
}

export function GenresView() {
  const paged = usePaged(['genres'], (a, offset, limit) => a.call('library.genres', { offset, limit }))
  return (
    <div className={v.view}>
      <h1 className={v.title}>{S.genresTitle}</h1>
      <VirtualGrid label={S.genresTitle} count={paged.total} row={paged.row} onRange={paged.onRange} render={(g) => <GenreTile g={g} />} keyOf={(g) => g.id} height={96} />
    </div>
  )
}
```

`app/src/renderer/views/GenreView.tsx`:

```tsx
import { useParams } from '@tanstack/react-router'
import { AlbumGrid } from '../components/AlbumGrid'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { usePaged } from '../lib/usePaged'
import v from './views.module.css'

export function GenreView() {
  const { genreId } = useParams({ strict: false }) as { genreId?: string }
  const id = Number(genreId)
  const genre = useEngineQuery(['genre', id], (a) => a.call('library.genre', { id }))
  const paged = usePaged(['genre-albums', id], (a, offset, limit) => a.call('library.albums', { filter: { genreId: id }, offset, limit }))
  if (genre.error) return <p className={v.error}>{genre.error}</p>
  return (
    <div className={v.view}>
      <div>
        <div className={v.kicker}>{S.genre.toUpperCase()}</div>
        <h1 className={v.title}>{genre.data?.name ?? ''}</h1>
        <div className={v.meta}>{S.albumCount(paged.total)}</div>
      </div>
      <AlbumGrid label={genre.data?.name ?? S.genresTitle} count={paged.total} row={paged.row} onRange={paged.onRange} />
    </div>
  )
}
```

- [ ] **Step 5: Register the routes**

In `app/src/renderer/router.tsx`, import the six views and add:

```tsx
const albumsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/albums', component: AlbumsView })
const albumRoute = createRoute({ getParentRoute: () => rootRoute, path: '/albums/$albumId', component: AlbumView })
const artistsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/artists', component: ArtistsView })
const artistRoute = createRoute({ getParentRoute: () => rootRoute, path: '/artists/$artistId', component: ArtistView })
const genresRoute = createRoute({ getParentRoute: () => rootRoute, path: '/genres', component: GenresView })
const genreRoute = createRoute({ getParentRoute: () => rootRoute, path: '/genres/$genreId', component: GenreView })

export const routeTree = rootRoute.addChildren([homeRoute, albumsRoute, albumRoute, artistsRoute, artistRoute, genresRoute, genreRoute, signalRoute])
```

- [ ] **Step 6: Run the checks**

Run: `npm --prefix app run typecheck && npm --prefix app test`
Expected: all pass, including the 11 `views-library` tests.

Run: `npm run engine:build && npm --prefix app run e2e`
Expected: the M1 smoke test still passes. Home now shows the empty-library state, which still has the "Open files" button the smoke test clicks.

- [ ] **Step 7: Commit**

```bash
git add app
git commit -m "feat(app): home, album, artist and genre views with dynamic album colour"
```

### Task 21: Playlists (drag-to-reorder), Search, Queue view, Track info dialog

**Files:**
- Create:
  - `app/src/renderer/components/SortableList.tsx` + `SortableList.module.css`;
  - `app/src/renderer/components/TrackInfoDialog.tsx`;
  - `app/src/renderer/components/ConfirmDialog.tsx`;
  - views `PlaylistsView.tsx`, `PlaylistView.tsx`, `SearchView.tsx`, `QueueView.tsx`.
- Modify:
  - `app/src/renderer/router.tsx`: four routes;
  - `app/src/renderer/lib/nav.ts`: Search, Playlists, Queue;
  - `app/src/renderer/lib/strings.ts`;
  - `app/src/renderer/Layout.tsx`: the TrackInfoDialog host and the sidebar playlists;
  - `app/src/renderer/components/Sidebar.tsx`: the `SidebarPlaylists` export;
  - `app/src/renderer/views/HomeView.tsx`: the Playlists section;
  - `app/src/renderer/components/PlayerBar.tsx`: the queue button goes to `/queue`.
- Test: `app/test/views-playlists.test.tsx`, `app/test/views-queue-search.test.tsx`, `app/test/TrackInfoDialog.test.tsx`

**Interfaces:**
- Consumes:
  - From Task 19: `TrackTable`, `trackMenu`, `AlbumCard`, `useGo`, `formatDuration` and `trackFormat`.
  - From Task 18: `useEngineQuery`, `useUi` (askName, openMenu, toast, trackInfo, searchText), `useLibrary` (playlists, createPlaylist) and `Dialog`.
  - From Task 17: `api.pickM3u` and `api.pickM3uSave`.
  - From Task 13: the `playlists.*`, `queue.*`, `library.search` and `library.track` methods.
- Produces:
  - **Routes:** `/playlists`, `/playlists/$playlistId`, `/search`, `/queue`.
  - **`NAV`:** Home, Search, Albums, Artists, Genres, Playlists, Queue, Signal path.
  - **`<SortableList<T>>`:** props `label`, `items: T[]`, `getId(item) -> number`, `onMove(from, to)`, and `renderRow(item, index, handle: React.HTMLAttributes<HTMLElement>)`.
    - It uses dnd-kit with pointer and keyboard sensors. The keyboard sequence is Space, then the arrow keys, then Space.
    - Rows also offer "Move up"/"Move down" in their context menus, as the non-drag path to reorder (spec §5.7 keyboard navigation).
  - **`<ConfirmDialog title body confirm onConfirm onCancel />`.**
  - **`<TrackInfoDialog />`:** opens when `useUi.trackInfo` is set and shows every `TrackDetail` field (spec §5.3 "full technical metadata dialog"), with technical values in mono.
  - **`SidebarPlaylists`:** when the sidebar is expanded, a "PLAYLISTS" section linking every playlist.
  - **Search** (spec §5.3):
    - Instant search on input, debounced 150 ms, needing at least 2 characters.
    - Results are grouped as Artists, Albums and Tracks.
    - The text lives in `useUi.searchText`, so it survives navigation.
  - **Queue view** (spec §5.3):
    - It shows "Playing from \<album\>" and highlights the current item.
    - Rows can be reordered, removed one by one, or cleared all at once. Double-clicking a row plays it.

- [ ] **Step 1: Write the failing tests**

`app/test/views-playlists.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

const mix = { id: 4, name: 'Mix', trackCount: 2, durationMs: 122_000, updatedAt: 0 }
const detail = { playlist: mix, items: [{ itemId: 10, track: track(1, 'Dawn') }, { itemId: 11, track: track(2, 'Noon') }] }

describe('playlists', () => {
  it('lists playlists, creates one, and imports an M3U', async () => {
    const { api, calls } = fakeApi({
      'playlists.list': () => [mix],
      'playlists.create': (p) => ({ ...mix, id: 5, name: p.name, trackCount: 0 }),
      'playlists.importM3u': () => ({ playlist: { ...mix, id: 6, name: 'Road' }, skipped: 2 }),
    })
    await renderAt('/playlists', api)
    expect(await screen.findByText('Mix')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'New playlist' }))
    act(() => useUi.getState().prompt!.resolve('Chill'))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.create', { name: 'Chill' }]))
    api.pickM3u.mockResolvedValueOnce('/lists/Road.m3u8')
    fireEvent.click(screen.getByRole('button', { name: 'Import M3U' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.importM3u', { path: '/lists/Road.m3u8' }]))
    expect(await screen.findByText('Imported Road · 2 entries not found in your library')).toBeTruthy()
  })

  it('shows a playlist, plays from a row, reorders, removes, renames, exports and deletes', async () => {
    const { api, calls } = fakeApi({ 'playlists.get': () => detail, 'playlists.list': () => [mix], 'playlists.rename': (p) => ({ ...mix, name: p.name }) })
    const { router } = await renderAt('/playlists/4', api)
    expect(await screen.findByRole('heading', { name: 'Mix' })).toBeTruthy()
    const list = screen.getByRole('list', { name: 'Mix' })
    fireEvent.doubleClick(within(list).getByText('Noon'))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [1, 2], startIndex: 1 }]))

    fireEvent.contextMenu(within(list).getByText('Dawn'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move down' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.reorder', { id: 4, from: 0, to: 1 }]))

    fireEvent.contextMenu(within(list).getByText('Noon'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Remove from playlist' }))
    await vi.waitFor(() => expect(calls.some(([m, p]) => m === 'playlists.removeItems' && p.id === 4 && p.itemIds.length === 1)).toBe(true))

    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    act(() => useUi.getState().prompt!.resolve('Better Mix'))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.rename', { id: 4, name: 'Better Mix' }]))

    api.pickM3uSave.mockResolvedValueOnce('/out/Mix.m3u8')
    fireEvent.click(screen.getByRole('button', { name: 'Export M3U' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.exportM3u', { id: 4, path: '/out/Mix.m3u8' }]))

    fireEvent.click(screen.getByRole('button', { name: 'Delete playlist' }))
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.delete', { id: 4 }]))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/playlists'))
  })

  it('lists playlists in the expanded sidebar', async () => {
    await renderAt('/', fakeApi({ 'playlists.list': () => [mix] }).api)
    act(() => useUi.getState().setSidebar(true, false))
    expect(await screen.findByRole('link', { name: 'Mix' })).toBeTruthy()
  })
})
```

`app/test/views-queue-search.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { album, fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

const item = (id: number, title: string) => ({ id, path: `/m/${title}.flac`, title, trackId: id * 10, artist: 'Alpha', album: 'Light', artworkId: null, durationMs: 60_000 })

describe('queue view', () => {
  it('shows the queue with the current item and edits it', async () => {
    const { api, calls } = fakeApi({ 'queue.get': () => ({ items: [item(1, 'Dawn'), item(2, 'Noon'), item(3, 'Dusk')], current: 1 }) })
    await renderAt('/queue', api)
    const list = await screen.findByRole('list', { name: 'Queue' })
    expect(within(list).getByText('Noon').closest('[data-current]')!.getAttribute('data-current')).toBe('true')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, status: 'playing', track: { path: '/m/Noon.flac', title: 'Noon', durationMs: 1, trackId: 20, artist: 'Alpha', album: 'Light', artworkId: null } } }))
    expect(screen.getByText('Playing from Light')).toBeTruthy()
    fireEvent.doubleClick(within(list).getByText('Dusk'))
    await vi.waitFor(() => expect(calls).toContainEqual(['player.play', { queueItemId: 3 }]))
    fireEvent.click(within(list).getAllByRole('button', { name: 'Remove from queue' })[0])
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.remove', { itemIds: [1] }]))
    fireEvent.contextMenu(within(list).getByText('Dawn'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move down' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.move', { from: 0, to: 1 }]))
    fireEvent.click(screen.getByRole('button', { name: 'Clear queue' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.clear', {}]))
  })

  it('opens from the player bar', async () => {
    const { router } = await renderAt('/')
    fireEvent.click(screen.getByRole('button', { name: 'Queue' }))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/queue'))
  })
})

describe('search view', () => {
  it('searches as you type and groups the results', async () => {
    const { api, calls } = fakeApi({
      'library.search': () => ({ tracks: [track(1, 'Dawn')], albums: [album(5, 'First Light')], artists: [{ id: 1, name: 'Alpha', albumCount: 1, trackCount: 3, artworkId: null }] }),
    })
    await renderAt('/search', api)
    const input = screen.getByRole('searchbox', { name: 'Search' })
    expect(document.activeElement).toBe(input)
    fireEvent.change(input, { target: { value: 'a' } })
    expect(screen.getByText('Type at least two characters.')).toBeTruthy()
    fireEvent.change(input, { target: { value: 'alp' } })
    await vi.waitFor(() => expect(calls).toContainEqual(['library.search', { text: 'alp', limit: 20 }]))
    expect(await screen.findByRole('heading', { name: 'Artists' })).toBeTruthy()
    expect(screen.getByText('First Light')).toBeTruthy()
    expect(within(screen.getByRole('grid', { name: 'Tracks' })).getByText('Dawn')).toBeTruthy()
    expect(calls.filter(([m]) => m === 'library.search').every(([, p]) => p.text.length >= 2)).toBe(true)
  })
})
```

`app/test/TrackInfoDialog.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it } from 'vitest'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

describe('track info dialog', () => {
  it('shows the full technical metadata and closes with Escape', async () => {
    const detail = {
      track: track(1, 'Dawn', { album: 'Light', path: '/m/Alpha/Dawn.flac', playCount: 3 }),
      artists: [{ id: 1, name: 'Alpha' }, { id: 2, name: 'Beta' }], genres: [{ id: 3, name: 'Ambient' }], composer: 'C. Writer',
      trackTotal: 9, discTotal: 1, replayGain: { trackGain: -6.5, trackPeak: 0.98, albumGain: null, albumPeak: null },
      musicbrainz: { recording: 'rec-1', release: null, artist: null }, hasLyrics: true, sizeBytes: 31_457_280, mtime: 1_700_000_000_000,
      gapless: { delay: 2112, padding: 576 },
    }
    await renderAt('/', fakeApi({ 'library.track': () => detail }).api)
    act(() => useUi.getState().showTrackInfo(1))
    const dialog = await screen.findByRole('dialog', { name: 'Dawn' })
    for (const text of ['Alpha, Beta', 'Ambient', 'C. Writer', '1 / 9', 'FLAC 16/44.1', '44.1 kHz', '16-bit', '2112 / 576', '-6.50 dB', 'rec-1', '30.0 MB', '/m/Alpha/Dawn.flac'])
      expect(dialog.textContent).toContain(text)
    fireEvent.keyDown(dialog, { key: 'Escape' })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(useUi.getState().trackInfo).toBeNull()
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm --prefix app test -- views-playlists views-queue-search TrackInfoDialog`
Expected: failures. The routes don't exist yet, and the dialog isn't mounted.

- [ ] **Step 3: Strings, navigation and the shared components**

Add to `S`:

```ts
  searchTitle: 'Search',
  searchPlaceholder: 'Artists, albums, tracks',
  searchHint: 'Type at least two characters.',
  noResults: 'No matches.',
  tracksTitle: 'Tracks',
  playlistsTitle: 'Playlists',
  newPlaylistButton: 'New playlist',
  importM3u: 'Import M3U',
  exportM3u: 'Export M3U',
  imported: (name: string, skipped: number) => (skipped ? `Imported ${name} · ${skipped} ${skipped === 1 ? 'entry' : 'entries'} not found in your library` : `Imported ${name}`),
  exported: 'Playlist exported',
  rename: 'Rename',
  renameTitle: 'Rename playlist',
  deletePlaylist: 'Delete playlist',
  deleteConfirm: (name: string) => `Delete “${name}”? The tracks stay in your library.`,
  delete: 'Delete',
  removeFromPlaylist: 'Remove from playlist',
  moveUp: 'Move up',
  moveDown: 'Move down',
  playlist: 'Playlist',
  playPlaylist: 'Play playlist',
  shufflePlaylist: 'Shuffle playlist',
  queueTitle: 'Queue',
  queueEmpty: 'The queue is empty. Play an album or add tracks with “Add to queue”.',
  playingFrom: (album: string) => `Playing from ${album}`,
  removeFromQueue: 'Remove from queue',
  clearQueue: 'Clear queue',
  dragHandle: 'Drag to reorder',
  info: {
    title: 'Title', artists: 'Artists', album: 'Album', albumArtist: 'Album artist', trackDisc: 'Track / disc', year: 'Year',
    genres: 'Genres', composer: 'Composer', duration: 'Duration', format: 'Format', bitrate: 'Bitrate', sampleRate: 'Sample rate',
    bitDepth: 'Bit depth', channels: 'Channels', gapless: 'Gapless (delay / padding)', replayGain: 'ReplayGain (track / album)',
    musicbrainz: 'MusicBrainz recording', lyrics: 'Lyrics', path: 'File', size: 'Size', modified: 'Modified', added: 'Added',
    plays: 'Plays', lastPlayed: 'Last played', status: 'Status', yes: 'Yes', no: 'No', close: 'Close',
  },
```

In `lib/nav.ts`, import `ListMusic, ListOrdered, Search` and set:

```ts
export const NAV: NavItem[] = [
  { to: '/', label: S.nav.home, icon: House },
  { to: '/search', label: S.nav.search, icon: Search },
  { to: '/albums', label: S.nav.albums, icon: Disc3 },
  { to: '/artists', label: S.nav.artists, icon: Users },
  { to: '/genres', label: S.nav.genres, icon: Tags },
  { to: '/playlists', label: S.nav.playlists, icon: ListMusic },
  { to: '/queue', label: S.nav.queue, icon: ListOrdered },
  { to: '/signal', label: S.nav.signal, icon: Activity },
]
```

`app/src/renderer/components/SortableList.module.css`:

```css
.list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
.item { display: grid; grid-template-columns: 24px 1fr; align-items: center; gap: 8px; min-height: 44px; padding: 0 8px; border-radius: var(--radius-sm); background: var(--bg); }
.item:hover { background: var(--raised); }
.item[data-dragging='true'] { position: relative; z-index: 3; background: var(--raised-2); box-shadow: 0 6px 18px rgb(0 0 0 / 0.4); }
.main[data-current='true'] { color: var(--accent); }
.handle { display: grid; place-items: center; height: 28px; color: var(--text-faint); cursor: grab; touch-action: none; border-radius: var(--radius-sm); }
.handle:hover { color: var(--text); }
.main { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1.2fr) 64px 28px; align-items: center; gap: 12px; min-width: 0; }
.main > span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
```

`app/src/renderer/components/SortableList.tsx`:

```tsx
import { closestCenter, DndContext, KeyboardSensor, PointerSensor, useSensor, useSensors, type DragEndEvent } from '@dnd-kit/core'
import { SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { GripVertical } from 'lucide-react'
import { S } from '../lib/strings'
import s from './SortableList.module.css'

function Row({ id, children }: { id: number; children: (handle: React.HTMLAttributes<HTMLElement>) => React.ReactNode }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id })
  return (
    <li ref={setNodeRef} className={s.item} data-dragging={isDragging} style={{ transform: CSS.Transform.toString(transform), transition }}>
      {children({ ...attributes, ...listeners, 'aria-label': S.dragHandle, className: s.handle } as React.HTMLAttributes<HTMLElement>)}
    </li>
  )
}

/** Drag-to-reorder list (pointer, or keyboard: Space, arrows, Space). `onMove` gets array-move indices. */
export function SortableList<T>({ label, items, getId, onMove, renderRow }: {
  label: string
  items: T[]
  getId: (item: T) => number
  onMove: (from: number, to: number) => void
  renderRow: (item: T, index: number, handle: React.HTMLAttributes<HTMLElement>) => React.ReactNode
}) {
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }), useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }))
  const ids = items.map(getId)
  const onDragEnd = (e: DragEndEvent) => {
    if (!e.over || e.active.id === e.over.id) return
    const from = ids.indexOf(Number(e.active.id))
    const to = ids.indexOf(Number(e.over.id))
    if (from >= 0 && to >= 0) onMove(from, to)
  }
  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      <SortableContext items={ids} strategy={verticalListSortingStrategy}>
        <ul className={s.list} aria-label={label}>
          {items.map((item, i) => <Row key={ids[i]} id={ids[i]}>{(handle) => renderRow(item, i, handle)}</Row>)}
        </ul>
      </SortableContext>
    </DndContext>
  )
}

export function DragHandle(props: React.HTMLAttributes<HTMLElement>) {
  return <span role="button" tabIndex={0} {...props}><GripVertical size={14} aria-hidden /></span>
}

export { s as sortableStyles }
```

`app/src/renderer/components/ConfirmDialog.tsx`:

```tsx
import { S } from '../lib/strings'
import { Dialog } from './Dialog'
import s from './ui.module.css'

export function ConfirmDialog({ title, body, confirm, onConfirm, onCancel }: { title: string; body: string; confirm: string; onConfirm: () => void; onCancel: () => void }) {
  return (
    <Dialog title={title} onClose={onCancel}>
      <p>{body}</p>
      <div className={s.actions}>
        <button className={s.secondary} onClick={onCancel}>{S.cancel}</button>
        <button className={s.primary} onClick={onConfirm}>{confirm}</button>
      </div>
    </Dialog>
  )
}
```

`app/src/renderer/components/TrackInfoDialog.tsx`:

```tsx
import { codecLabel, formatTime, khz, trackFormat } from '../lib/format'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useUi } from '../stores/ui'
import { Dialog } from './Dialog'
import s from './ui.module.css'

const db = (v: number | null) => (v == null ? '—' : `${v.toFixed(2)} dB`)
const mb = (bytes: number) => `${(bytes / 1_048_576).toFixed(1)} MB`
const date = (ms: number | null) => (ms == null ? '—' : new Date(ms).toLocaleString())

export function TrackInfoDialog() {
  const id = useUi((u) => u.trackInfo)
  if (id == null) return null
  return <TrackInfoBody key={id} id={id} />
}

function TrackInfoBody({ id }: { id: number }) {
  const close = () => useUi.getState().showTrackInfo(null)
  const q = useEngineQuery(['track-info', id], (a) => a.call('library.track', { id }), [])
  const d = q.data
  if (!d) return q.error ? <Dialog title={S.trackInfo} onClose={close}><p>{q.error}</p></Dialog> : null
  const t = d.track
  const rows: [string, React.ReactNode, boolean?][] = [
    [S.info.artists, d.artists.map((a) => a.name).join(', ') || S.unknownArtist],
    [S.info.album, t.album || S.unknownAlbum],
    [S.info.albumArtist, t.albumArtist ?? '—'],
    [S.info.trackDisc, `${t.trackNo ?? '—'} / ${d.trackTotal ?? '—'} · ${t.discNo ?? 1} / ${d.discTotal ?? 1}`],
    [S.info.year, t.year ?? '—'],
    [S.info.genres, d.genres.map((g) => g.name).join(', ') || '—'],
    [S.info.composer, d.composer ?? '—'],
    [S.info.duration, t.durationMs != null ? formatTime(t.durationMs) : '—', true],
    [S.info.format, t.codec ? `${trackFormat(t)} (${codecLabel({ codec: t.codec, container: t.container } as never)}, ${t.container.toUpperCase()})` : '—', true],
    [S.info.bitrate, t.bitrateKbps ? `${t.bitrateKbps} kbps` : '—', true],
    [S.info.sampleRate, t.sampleRate ? `${khz(t.sampleRate)} kHz` : '—', true],
    [S.info.bitDepth, t.bitDepth ? `${t.bitDepth}-bit` : '—', true],
    [S.info.channels, t.channels ?? '—', true],
    [S.info.gapless, d.gapless ? `${d.gapless.delay} / ${d.gapless.padding}` : '—', true],
    [S.info.replayGain, `${db(d.replayGain.trackGain)} / ${db(d.replayGain.albumGain)}`, true],
    [S.info.musicbrainz, d.musicbrainz.recording ?? '—', true],
    [S.info.lyrics, d.hasLyrics ? S.info.yes : S.info.no],
    [S.info.path, t.path, true],
    [S.info.size, mb(d.sizeBytes), true],
    [S.info.modified, date(d.mtime)],
    [S.info.added, date(t.addedAt * 1000)],
    [S.info.plays, t.playCount, true],
    [S.info.lastPlayed, date(t.lastPlayedAt == null ? null : t.lastPlayedAt * 1000)],
    [S.info.status, t.missing ? S.missingFile : t.unplayable ? S.unplayable(t.unplayable) : '—'],
  ]
  return (
    <Dialog title={t.title} onClose={close}>
      <dl style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: '6px 16px', margin: 0, userSelect: 'text' }}>
        {rows.map(([k, v, mono]) => (
          <div key={k} style={{ display: 'contents' }}>
            <dt className="muted">{k}</dt>
            <dd className={mono ? 'mono' : undefined} style={{ margin: 0, wordBreak: 'break-all' }}>{v}</dd>
          </div>
        ))}
      </dl>
      <div className={s.actions}><button className={s.secondary} onClick={close}>{S.info.close}</button></div>
    </Dialog>
  )
}
```

The `Dialog` focuses its first `button` on open; here that's Close, which is acceptable for a read-only dialog. `Dialog` handles Escape. The test expects `-6.50 dB`, so keep the `toFixed(2)`.

In `Sidebar.tsx`, add and export:

```tsx
import { useLibrary } from '../stores/library'

export function SidebarPlaylists() {
  const playlists = useLibrary((l) => l.playlists)
  const pathname = useRouterState({ select: (r) => r.location.pathname })
  if (!playlists.length) return null
  return (
    <>
      <div className={s.sectionTitle}>{S.playlistsTitle.toUpperCase()}</div>
      <ul className={s.nav}>
        {playlists.map((p) => (
          <li key={p.id}>
            <Link to="/playlists/$playlistId" params={{ playlistId: String(p.id) }} className={s.navLink} aria-current={pathname === `/playlists/${p.id}` ? 'page' : undefined}>
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</span>
            </Link>
          </li>
        ))}
      </ul>
    </>
  )
}
```

In `Layout.tsx`:
- Render `<Sidebar><SidebarPlaylists /></Sidebar>`.
- Add `<TrackInfoDialog />` next to `<PromptDialog />`.

In `PlayerBar.tsx`, the queue button navigates `{ to: '/queue' }`.

- [ ] **Step 4: Implement the views**

`app/src/renderer/views/PlaylistsView.tsx`:

```tsx
import type { Playlist } from '@protocol/Playlist'
import { FileInput, Plus } from 'lucide-react'
import { formatDuration } from '../lib/format'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export async function newPlaylist(go?: (p: string) => void) {
  const name = await useUi.getState().askName({ title: S.newPlaylistTitle, confirm: S.create })
  if (!name) return
  const p = await useLibrary.getState().createPlaylist(name)
  if (p && go) go(`/playlists/${p.id}`)
}

async function importM3u(go: (p: string) => void) {
  const api = usePlayer.getState().api
  const path = await api?.pickM3u()
  if (!api || !path) return
  try {
    const r = await api.call('playlists.importM3u', { path })
    useUi.getState().toast('info', S.imported(r.playlist.name, r.skipped))
    void useLibrary.getState().refreshPlaylists()
    go(`/playlists/${r.playlist.id}`)
  } catch (e) {
    useUi.getState().toast('error', e instanceof Error ? e.message : String(e))
  }
}

export function PlaylistTile({ p }: { p: Playlist }) {
  const go = useGo()
  return (
    <button className={v.tile} onClick={() => go(`/playlists/${p.id}`)}>
      <span className={v.tileName}>{p.name}</span>
      <span className={v.meta}>{S.trackCount(p.trackCount)} · {formatDuration(p.durationMs)}</span>
    </button>
  )
}

export function PlaylistsView() {
  const playlists = useLibrary((l) => l.playlists)
  const go = useGo()
  return (
    <div className={v.view}>
      <div className={v.toolbar}>
        <h1 className={v.title}>{S.playlistsTitle}</h1>
        <div className={v.actions}>
          <button className={v.primary} onClick={() => void newPlaylist(go)}><Plus size={16} /> {S.newPlaylistButton}</button>
          <button className={v.ghost} onClick={() => void importM3u(go)}><FileInput size={16} /> {S.importM3u}</button>
        </div>
      </div>
      {playlists.length === 0 ? <p className="muted">{S.nothingYet}</p> : (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 14 }}>
          {playlists.map((p) => <PlaylistTile key={p.id} p={p} />)}
        </div>
      )}
    </div>
  )
}
```

`app/src/renderer/views/PlaylistView.tsx`:

```tsx
import type { PlaylistItem } from '@protocol/PlaylistItem'
import { useParams } from '@tanstack/react-router'
import { ArrowDown, ArrowUp, FileOutput, Pencil, Play, Shuffle, Trash2, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DragHandle, SortableList, sortableStyles as ss } from '../components/SortableList'
import { trackMenu } from '../components/trackMenu'
import { formatDuration, formatTime } from '../lib/format'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useGo } from '../lib/useGo'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

function arrayMove<T>(a: T[], from: number, to: number): T[] {
  const b = [...a]
  const [x] = b.splice(from, 1)
  b.splice(to, 0, x)
  return b
}

export function PlaylistView() {
  const { playlistId } = useParams({ strict: false }) as { playlistId?: string }
  const id = Number(playlistId)
  const go = useGo()
  const q = useEngineQuery(['playlist', id], (a) => a.call('playlists.get', { id }), ['playlists', 'tracks'])
  const [items, setItems] = useState<PlaylistItem[]>([])
  const [confirmDelete, setConfirmDelete] = useState(false)
  useEffect(() => { if (q.data) setItems(q.data.items) }, [q.data])
  if (q.error) return <p className={v.error}>{q.error}</p>
  if (!q.data) return null
  const pl = q.data.playlist
  const api = usePlayer.getState().api
  const player = usePlayer.getState()
  const ids = items.map((i) => i.track.id)
  const call = async (p: Promise<unknown> | undefined) => { try { await p } catch (e) { useUi.getState().toast('error', e instanceof Error ? e.message : String(e)) } }
  const move = (from: number, to: number) => {
    if (to < 0 || to >= items.length || from === to) return
    setItems((cur) => arrayMove(cur, from, to))
    void call(api?.call('playlists.reorder', { id, from, to }))
  }
  const remove = (itemIds: number[]) => {
    setItems((cur) => cur.filter((i) => !itemIds.includes(i.itemId)))
    void call(api?.call('playlists.removeItems', { id, itemIds }))
  }
  const rename = async () => {
    const name = await useUi.getState().askName({ title: S.renameTitle, initial: pl.name, confirm: S.save })
    if (name) await call(api?.call('playlists.rename', { id, name }))
  }
  const exportM3u = async () => {
    const path = await api?.pickM3uSave(pl.name)
    if (!path) return
    await call(api?.call('playlists.exportM3u', { id, path }).then(() => useUi.getState().toast('info', S.exported)))
  }
  const del = async () => {
    setConfirmDelete(false)
    await call(api?.call('playlists.delete', { id }))
    void useLibrary.getState().refreshPlaylists()
    go('/playlists')
  }
  return (
    <div className={v.view}>
      <header className={v.head} style={{ alignItems: 'center' }}>
        <div>
          <div className={v.kicker}>{S.playlist.toUpperCase()}</div>
          <h1 className={v.title}>{pl.name}</h1>
          <div className={v.meta}>{S.trackCount(pl.trackCount)} · {formatDuration(pl.durationMs)}</div>
          <div className={v.actions} style={{ marginTop: 14 }}>
            <button className={v.primary} aria-label={S.playPlaylist} disabled={!ids.length} onClick={() => void player.playTracks(ids, 0)}><Play size={16} fill="currentColor" /> {S.play}</button>
            <button className={v.ghost} aria-label={S.shufflePlaylist} disabled={!ids.length} onClick={() => void player.playTracks(ids, Math.floor(Math.random() * ids.length), { shuffle: true })}><Shuffle size={16} /> {S.shuffle}</button>
            <button className={v.ghost} onClick={() => void rename()}><Pencil size={16} /> {S.rename}</button>
            <button className={v.ghost} onClick={() => void exportM3u()}><FileOutput size={16} /> {S.exportM3u}</button>
            <button className={v.ghost} aria-label={S.deletePlaylist} onClick={() => setConfirmDelete(true)}><Trash2 size={16} /></button>
          </div>
        </div>
      </header>
      {items.length === 0 ? <p className="muted">{S.nothingYet}</p> : (
        <SortableList label={pl.name} items={items} getId={(i) => i.itemId} onMove={move}
          renderRow={(it, index, handle) => (
            <>
              <DragHandle {...handle} />
              <div className={ss.main} data-dim={it.track.missing || !!it.track.unplayable}
                onDoubleClick={() => void player.playTracks(ids, index)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  useUi.getState().openMenu(e.clientX, e.clientY, trackMenu([it.track], go, [
                    { label: S.moveUp, icon: ArrowUp, disabled: index === 0, onSelect: () => move(index, index - 1) },
                    { label: S.moveDown, icon: ArrowDown, disabled: index === items.length - 1, onSelect: () => move(index, index + 1) },
                    { label: S.removeFromPlaylist, icon: X, onSelect: () => remove([it.itemId]) },
                  ]))
                }}>
                <span>{it.track.missing ? '⚠ ' : ''}{it.track.title}</span>
                <span className="muted">{it.track.artist || S.unknownArtist}</span>
                <span className="mono muted">{it.track.durationMs != null ? formatTime(it.track.durationMs) : ''}</span>
                <span />
              </div>
            </>
          )} />
      )}
      {confirmDelete && <ConfirmDialog title={S.deletePlaylist} body={S.deleteConfirm(pl.name)} confirm={S.delete} onConfirm={() => void del()} onCancel={() => setConfirmDelete(false)} />}
    </div>
  )
}
```

`app/src/renderer/views/QueueView.tsx`:

```tsx
import { ArrowDown, ArrowUp, ListX, X } from 'lucide-react'
import { DragHandle, SortableList, sortableStyles as ss } from '../components/SortableList'
import { formatTime } from '../lib/format'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export function QueueView() {
  const queue = usePlayer((p) => p.queue)
  const album = usePlayer((p) => (p.state.status !== 'stopped' ? p.state.track?.album : null))
  const api = usePlayer((p) => p.api)
  const send = (p: Promise<unknown> | undefined) => void p?.catch((e: unknown) => useUi.getState().toast('error', e instanceof Error ? e.message : String(e)))
  const move = (from: number, to: number) => { if (to >= 0 && to < queue.items.length && from !== to) send(api?.call('queue.move', { from, to })) }
  return (
    <div className={v.view}>
      <div className={v.toolbar}>
        <div>
          <h1 className={v.title}>{S.queueTitle}</h1>
          {album && <div className={v.meta}>{S.playingFrom(album)}</div>}
        </div>
        <button className={v.ghost} disabled={!queue.items.length} onClick={() => send(api?.call('queue.clear', {}))}><ListX size={16} /> {S.clearQueue}</button>
      </div>
      {queue.items.length === 0 ? <p className="muted">{S.queueEmpty}</p> : (
        <SortableList label={S.queueTitle} items={queue.items} getId={(i) => i.id} onMove={move}
          renderRow={(it, index, handle) => (
            <>
              <DragHandle {...handle} />
              <div className={ss.main} data-current={index === queue.current}
                onDoubleClick={() => void usePlayer.getState().playItem(it.id)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  useUi.getState().openMenu(e.clientX, e.clientY, [
                    { label: S.moveUp, icon: ArrowUp, disabled: index === 0, onSelect: () => move(index, index - 1) },
                    { label: S.moveDown, icon: ArrowDown, disabled: index === queue.items.length - 1, onSelect: () => move(index, index + 1) },
                    { label: S.removeFromQueue, icon: X, onSelect: () => send(api?.call('queue.remove', { itemIds: [it.id] })) },
                  ])
                }}>
                <span>{it.title}</span>
                <span className="muted">{it.artist ?? ''}</span>
                <span className="mono muted">{it.durationMs != null ? formatTime(it.durationMs) : ''}</span>
                <button aria-label={S.removeFromQueue} className="muted" onClick={() => send(api?.call('queue.remove', { itemIds: [it.id] }))}><X size={14} /></button>
              </div>
            </>
          )} />
      )}
    </div>
  )
}
```

`app/src/renderer/views/SearchView.tsx`:

```tsx
import type { ArtistRow } from '@protocol/ArtistRow'
import { Search } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { AlbumCard } from '../components/AlbumGrid'
import { TrackTable } from '../components/TrackTable'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useGo } from '../lib/useGo'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

function ArtistChip({ a }: { a: ArtistRow }) {
  const go = useGo()
  return (
    <button className={v.tile} onClick={() => go(`/artists/${a.id}`)}>
      <span className={v.tileName}>{a.name}</span>
      <span className={v.meta}>{S.albumCount(a.albumCount)} · {S.trackCount(a.trackCount)}</span>
    </button>
  )
}

export function SearchView() {
  const text = useUi((u) => u.searchText)
  const [debounced, setDebounced] = useState(text)
  const input = useRef<HTMLInputElement>(null)
  useEffect(() => { input.current?.focus() }, [])
  useEffect(() => { const t = setTimeout(() => setDebounced(text.trim()), 150); return () => clearTimeout(t) }, [text])
  const ok = debounced.length >= 2
  const q = useEngineQuery(['search', ok ? debounced : ''], (a) => (ok ? a.call('library.search', { text: debounced, limit: 20 }) : Promise.resolve(null)))
  const r = ok ? q.data : null
  const empty = r && !r.tracks.length && !r.albums.length && !r.artists.length
  return (
    <div className={v.view}>
      <label style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '0 12px', height: 44, borderRadius: 6, background: 'var(--raised)', border: '1px solid var(--border)' }}>
        <Search size={18} className="muted" aria-hidden />
        <input ref={input} type="search" aria-label={S.searchTitle} placeholder={S.searchPlaceholder} value={text}
          onChange={(e) => useUi.getState().setSearchText(e.target.value)}
          style={{ flex: 1, background: 'transparent', border: 0, color: 'var(--text)', font: 'inherit', fontSize: 15, outline: 'none' }} />
      </label>
      {text.trim().length < 2 && <p className="muted">{S.searchHint}</p>}
      {empty && <p className="muted">{S.noResults}</p>}
      {!!r?.artists.length && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.artistsTitle}</h2>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 10 }}>{r.artists.slice(0, 8).map((a) => <ArtistChip key={a.id} a={a} />)}</div>
        </section>
      )}
      {!!r?.albums.length && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.albumsTitle}</h2>
          <div className={v.row} role="list" aria-label={S.albumsTitle}>{r.albums.map((a) => <AlbumCard key={a.id} a={a} />)}</div>
        </section>
      )}
      {!!r?.tracks.length && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.tracksTitle}</h2>
          <TrackTable label={S.tracksTitle} count={r.tracks.length} row={(i) => r.tracks[i]} columns={['title', 'artist', 'album', 'format', 'duration']}
            onPlay={(i) => void usePlayer.getState().playTracks(r.tracks.map((t) => t.id), i)} />
        </section>
      )}
    </div>
  )
}
```

Add the Playlists section to `HomeView.tsx`, after Most played and inside the non-empty branch:

```tsx
          {playlists.length > 0 && (
            <section className={v.section}>
              <h2 className={v.sectionTitle}>{S.playlistsTitle}</h2>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 14 }}>{playlists.slice(0, 6).map((p) => <PlaylistTile key={p.id} p={p} />)}</div>
            </section>
          )}
```

Read `const playlists = useLibrary((l) => l.playlists)` at the top of `HomeView` and import `PlaylistTile` from `./PlaylistsView`.

- [ ] **Step 5: Register the routes**

In `router.tsx`, add:

```tsx
const searchRoute = createRoute({ getParentRoute: () => rootRoute, path: '/search', component: SearchView })
const playlistsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/playlists', component: PlaylistsView })
const playlistRoute = createRoute({ getParentRoute: () => rootRoute, path: '/playlists/$playlistId', component: PlaylistView })
const queueRoute = createRoute({ getParentRoute: () => rootRoute, path: '/queue', component: QueueView })
```

Include them in `rootRoute.addChildren([...])`.

- [ ] **Step 6: Run the checks**

Run: `npm --prefix app run typecheck && npm --prefix app test`
Expected: all pass, including the playlists (3), queue/search (3) and track-info (1) tests.

In the playlist test, the "New playlist" button and the trackMenu's "New playlist…" item have different names, so they never collide. If the "Delete" button lookup is ambiguous, it's because the dialog renders inside `document.body`: the test scopes it with `within(dialog)`, so keep that.

- [ ] **Step 7: Commit**

```bash
git add app
git commit -m "feat(app): playlists with drag-to-reorder and M3U, instant search, queue view and track info dialog"
```

### Task 22: Complete player bar, Settings view, keyboard shortcuts and accessibility pass

**Files:**
- Modify:
  - `app/src/renderer/components/PlayerBar.tsx` + `PlayerBar.module.css`;
  - `app/src/renderer/components/Transport.tsx` + `Transport.module.css`;
  - `app/src/renderer/components/SeekBar.tsx`;
  - `app/src/renderer/stores/player.ts`: add `setShuffle` and `cycleRepeat`;
  - `app/src/renderer/lib/shortcuts.ts`;
  - `app/src/renderer/Layout.tsx`: shortcut navigation;
  - `app/src/renderer/lib/nav.ts`: Settings;
  - `app/src/renderer/router.tsx`: `/settings`;
  - `app/src/renderer/lib/strings.ts`.
- Create: `app/src/renderer/views/SettingsView.tsx`
- Test: `app/test/PlayerBar.test.tsx` (append), `app/test/shortcuts.test.ts` (append), `app/test/SettingsView.test.tsx`

**Interfaces:**
- Consumes:
  - From Task 19: `playlistSubmenu`.
  - From Task 18: `Artwork`, `useLibrary` (folders, addFolder, removeFolder, rescan), `useUi` and `ConfirmDialog`.
  - The protocol's `PlayerState.shuffle` and `repeat`.
- Produces:
  - **Player bar** (spec §5.4):
    - **Left:** 46 px artwork, title, artist (or album), and a ♥ "Add to playlist" button. The ♥ opens the playlist submenu for the current library track and is disabled for files outside the library.
    - **Centre:** a working shuffle (`aria-pressed`), prev, the square amber play button, next, repeat cycling off → all → one (`aria-label` "Repeat: off/all/one"), and the seek bar.
    - **Right:** format badges, the queue toggle (to `/queue`, or back if already there), the device picker, the BP switch and volume.
  - **`usePlayer`:** `setShuffle(on: boolean)` and `cycleRepeat()`.
  - **`SeekBar`:** `aria-valuetext` reads "1:05 of 4:12". A drag ends cleanly on `pointercancel` and on blur (M1 deferred minor).
  - **Shortcuts** (spec §5.6): `handleShortcut(e, store, now, go?)` adds:
    - Ctrl/Cmd+F goes to `/search`;
    - Ctrl/Cmd+, goes to `/settings`;
    - ↑/↓ change volume by 5 % (ignored in bit-perfect mode).
    - Grids, menus and listboxes keep their own arrow keys.
  - **`SHORTCUTS: { keys: string; action: string }[]`**, which the Settings view lists.
  - **Settings view (`/settings`)** has these sections:
    - **Library folders:** add, rescan all, and per folder: rescan, remove (with confirm), track count, last scan time, an "unavailable" marker.
    - **Audio:** output device, output mode, and a read-only buffer description.
    - **Appearance:** sidebar expanded.
    - **Keyboard shortcuts.**
    - **About:** licences (spec §5.3).
  - DSP settings are M3; the Settings view doesn't show them.

- [ ] **Step 1: Write the failing tests**

Append to `app/test/PlayerBar.test.tsx`:

```tsx
describe('PlayerBar (M2)', () => {
  const playing = { path: '/m/a.flac', title: 'Dawn', durationMs: 60_000, trackId: 7, artist: 'Alpha', album: 'Light', artworkId: 3 }

  it('shows artwork, artist and the library actions for the current track', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, status: 'playing', track: playing } }))
    expect(screen.getByRole('img', { name: 'Dawn' }).getAttribute('src')).toBe('aurora-art://art/3/64')
    expect(screen.getByText('Alpha')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Add to playlist' }))
    expect(screen.getByRole('menuitem', { name: 'New playlist…' })).toBeTruthy()
  })

  it('disables add-to-playlist for files outside the library', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, track: { ...playing, trackId: null, artworkId: null } } }))
    expect((screen.getByRole('button', { name: 'Add to playlist' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('toggles shuffle and cycles repeat', async () => {
    const { api, calls } = fakeApi()
    await renderAt('/', api)
    const shuffle = screen.getByRole('button', { name: 'Shuffle' })
    expect(shuffle.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(shuffle)
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setShuffle', { shuffle: true }]))
    fireEvent.click(screen.getByRole('button', { name: 'Repeat: off' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setRepeat', { repeat: 'all' }]))
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, shuffle: true, repeat: 'all' } }))
    expect(screen.getByRole('button', { name: 'Shuffle' }).getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(screen.getByRole('button', { name: 'Repeat: all' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setRepeat', { repeat: 'one' }]))
  })

  it('describes the seek position for screen readers', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, status: 'paused', positionMs: 65_000, durationMs: 252_000, track: playing } }))
    expect(screen.getByRole('slider', { name: 'Seek' }).getAttribute('aria-valuetext')).toBe('1:05 of 4:12')
  })
})
```

(Add `fakeApi` to the helpers import in that file.)

Append to `app/test/shortcuts.test.ts`:

```ts
describe('M2 shortcuts', () => {
  const setVolume = vi.fn(async () => {})
  beforeEach(() => { vi.clearAllMocks(); usePlayer.setState(usePlayer.getInitialState()); usePlayer.setState({ setVolume, state: { ...usePlayer.getState().state, volume: 0.5 } }) })

  it('Ctrl/Cmd+F goes to search and Ctrl/Cmd+, to settings', () => {
    const go = vi.fn()
    const e = new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, cancelable: true })
    expect(handleShortcut(e, usePlayer.getState(), 0, go)).toBe(true)
    expect(go).toHaveBeenCalledWith('/search')
    expect(e.defaultPrevented).toBe(true)
    handleShortcut(new KeyboardEvent('keydown', { key: ',', metaKey: true }), usePlayer.getState(), 0, go)
    expect(go).toHaveBeenLastCalledWith('/settings')
  })

  it('arrow up/down change volume, but not in bit-perfect mode or inside a grid', () => {
    handleShortcut(new KeyboardEvent('keydown', { key: 'ArrowUp' }), usePlayer.getState(), 0)
    expect(setVolume).toHaveBeenLastCalledWith(0.55)
    handleShortcut(new KeyboardEvent('keydown', { key: 'ArrowDown' }), usePlayer.getState(), 0)
    expect(setVolume).toHaveBeenLastCalledWith(0.45)
    const grid = document.createElement('div'); grid.setAttribute('role', 'grid'); document.body.append(grid)
    press(grid, { key: 'ArrowDown' })
    expect(setVolume).toHaveBeenCalledTimes(2)
    usePlayer.setState({ state: { ...usePlayer.getState().state, mode: 'bitPerfect' } })
    handleShortcut(new KeyboardEvent('keydown', { key: 'ArrowUp' }), usePlayer.getState(), 0)
    expect(setVolume).toHaveBeenCalledTimes(2)
  })
})
```

`app/test/SettingsView.test.tsx`:

```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi, renderAt } from './helpers'

afterEach(cleanup)

describe('settings', () => {
  const folders = [{ id: 1, path: '/music', trackCount: 1200, lastScanAt: 1_700_000_000, available: true }, { id: 2, path: '/mnt/usb', trackCount: 40, lastScanAt: null, available: false }]

  it('manages library folders', async () => {
    const { api, calls } = fakeApi({ 'library.folders': () => folders })
    await renderAt('/settings', api)
    const list = await screen.findByRole('list', { name: 'Library folders' })
    expect(within(list).getByText('/music')).toBeTruthy()
    expect(within(list).getByText('Unavailable')).toBeTruthy()
    fireEvent.click(within(list).getAllByRole('button', { name: 'Rescan' })[0])
    await vi.waitFor(() => expect(calls).toContainEqual(['library.rescan', { folderId: 1 }]))
    fireEvent.click(within(list).getAllByRole('button', { name: 'Remove' })[1])
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Remove' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['library.removeFolder', { id: 2 }]))
    fireEvent.click(screen.getByRole('button', { name: 'Rescan all' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['library.rescan', {}]))
    api.openFolder.mockResolvedValueOnce('/more')
    fireEvent.click(screen.getByRole('button', { name: 'Add a music folder' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['library.addFolder', { path: '/more' }]))
  })

  it('switches output mode and lists shortcuts and licences', async () => {
    const { api, calls } = fakeApi()
    await renderAt('/settings', api)
    fireEvent.click(await screen.findByRole('radio', { name: /Bit-perfect/ }))
    await vi.waitFor(() => expect(calls).toContainEqual(['player.setMode', { mode: 'bitPerfect' }]))
    expect(screen.getByText('Ctrl/Cmd + F')).toBeTruthy()
    expect(screen.getByText(/Lucide/)).toBeTruthy()
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm --prefix app test -- PlayerBar shortcuts SettingsView`
Expected: failures. There's no "Add to playlist" button, shuffle is disabled, `/settings` doesn't exist, and the shortcuts are missing.

- [ ] **Step 3: Strings and the player store**

Add to `S`:

```ts
  settingsTitle: 'Settings',
  libraryFolders: 'Library folders',
  rescan: 'Rescan',
  rescanAll: 'Rescan all',
  remove: 'Remove',
  removeFolderTitle: 'Remove folder',
  removeFolderBody: (path: string) => `Remove ${path} from the library? Files on disk are not touched; its tracks leave your library and playlists.`,
  unavailable: 'Unavailable',
  lastScan: (t: number | null) => (t ? `Scanned ${new Date(t * 1000).toLocaleString()}` : 'Not scanned yet'),
  audio: 'Audio',
  outputDevice: 'Output device',
  outputMode: 'Output mode',
  processed: 'Processed — shared output with software volume',
  bitPerfect: 'Bit-perfect — exclusive, untouched output where the device allows',
  buffer: 'Buffer',
  bufferInfo: '2 s, grows automatically (up to 8 s) after repeated underruns',
  appearance: 'Appearance',
  expandedSidebar: 'Show the expanded sidebar',
  shortcuts: 'Keyboard shortcuts',
  about: 'About',
  licences: 'Aurora is MIT licensed. It includes Inter and JetBrains Mono (SIL OFL 1.1), Lucide icons (ISC), libopus (BSD) and, in builds with HE-AAC, the Fraunhofer FDK AAC library (see THIRD_PARTY_NOTICES.md).',
  repeatLabel: (r: string) => `Repeat: ${r}`,
  addCurrentToPlaylist: 'Add to playlist',
```

In `stores/player.ts`:
- Add `setShuffle(on: boolean): Promise<void>` and `cycleRepeat(): Promise<void>` to the interface.
- Add the implementations:

```ts
    async setShuffle(shuffle) { await safe(call('queue.setShuffle', { shuffle })) },
    async cycleRepeat() {
      const next = { off: 'all', all: 'one', one: 'off' } as const
      await safe(call('queue.setRepeat', { repeat: next[get().state.repeat] }))
    },
```

- [ ] **Step 4: Transport, seek bar and player bar**

`Transport.tsx`:

```tsx
import { Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward } from 'lucide-react'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import s from './Transport.module.css'

// Actions are read from the store at click time so tests can swap them with setState.
const actions = () => usePlayer.getState()

export function Transport() {
  const playing = usePlayer((p) => p.state.status === 'playing')
  const shuffle = usePlayer((p) => p.state.shuffle)
  const repeat = usePlayer((p) => p.state.repeat)
  return (
    <div className={s.row}>
      <button aria-label="Shuffle" aria-pressed={shuffle} className={shuffle ? s.on : undefined} onClick={() => void actions().setShuffle(!shuffle)}><Shuffle size={16} /></button>
      <button aria-label="Previous" onClick={() => void actions().prev()}><SkipBack size={18} /></button>
      <button className={s.play} aria-label={playing ? 'Pause' : 'Play'} onClick={() => void actions().toggle()}>
        {playing ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}
      </button>
      <button aria-label="Next" onClick={() => void actions().next()}><SkipForward size={18} /></button>
      <button aria-label={S.repeatLabel(repeat)} className={repeat !== 'off' ? s.on : undefined} onClick={() => void actions().cycleRepeat()}>
        {repeat === 'one' ? <Repeat1 size={16} /> : <Repeat size={16} />}
      </button>
    </div>
  )
}
```

Append to `Transport.module.css`:

```css
.on { color: var(--accent) !important; position: relative; }
.on::after { content: ''; position: absolute; left: 50%; bottom: -5px; width: 4px; height: 4px; margin-left: -2px; border-radius: 1px; background: var(--accent); }
```

In `SeekBar.tsx`:
- Add `aria-valuetext={`${formatTime(pos)} of ${formatTime(duration)}`}` to the `<input>`.
- End the drag on `onPointerCancel={() => setDrag(null)}` and `onBlur={() => setDrag(null)}`.

Replace `PlayerBar.tsx` with:

```tsx
import { useNavigate, useRouter, useRouterState } from '@tanstack/react-router'
import { Heart, ListMusic, Volume2, VolumeX } from 'lucide-react'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import { Artwork } from './Artwork'
import { DevicePicker } from './DevicePicker'
import { FormatBadges } from './FormatBadges'
import { SeekBar } from './SeekBar'
import { Transport } from './Transport'
import { playlistSubmenu } from './trackMenu'
import s from './PlayerBar.module.css'

export function PlayerBar() {
  const state = usePlayer((p) => p.state)
  const signal = usePlayer((p) => p.signal)
  const navigate = useNavigate()
  const router = useRouter()
  const onQueue = useRouterState({ select: (r) => r.location.pathname === '/queue' })
  const bitPerfect = state.mode === 'bitPerfect'
  const { setVolume, setMode } = usePlayer.getState()
  const t = state.track
  const sub = t ? (t.artist ?? t.album ?? t.path.split(/[\\/]/).slice(-2, -1)[0]) : 'Open files to start'
  const like = (e: React.MouseEvent) => {
    const id = t?.trackId
    if (id == null) return
    const r = e.currentTarget.getBoundingClientRect()
    useUi.getState().openMenu(r.left, r.top - 8, playlistSubmenu(async () => [id]).items ?? [])
  }
  return (
    <footer className={s.bar}>
      <div className={s.now}>
        <Artwork id={t?.artworkId ?? null} size={64} px={46} alt={t?.title ?? ''} />
        <div className={s.meta}>
          <div className={s.title} data-testid="now-title">{t?.title ?? 'Nothing playing'}</div>
          <div className={`${s.sub} muted`}>{sub}</div>
        </div>
        <button className={s.icon} aria-label={S.addCurrentToPlaylist} disabled={t?.trackId == null} onClick={like}><Heart size={16} /></button>
      </div>
      <div className={s.center}>
        <Transport />
        <SeekBar />
      </div>
      <div className={s.right}>
        <FormatBadges signal={signal} />
        <button className={s.icon} aria-label="Queue" aria-pressed={onQueue} onClick={() => (onQueue ? router.history.back() : void navigate({ to: '/queue' }))}><ListMusic size={16} /></button>
        <DevicePicker />
        <button
          role="switch" aria-checked={bitPerfect} aria-label="Bit-perfect" className={s.switch}
          onClick={() => void setMode(bitPerfect ? 'processed' : 'bitPerfect')}
          title={bitPerfect ? 'Bit-perfect: exclusive, untouched output' : 'Processed: shared output with volume'}
        >
          <span className="mono">BP</span>
        </button>
        <span className={s.vol}>
          {state.volume === 0 ? <VolumeX size={16} aria-hidden /> : <Volume2 size={16} aria-hidden />}
          <input
            type="range" min={0} max={1} step={0.01} aria-label="Volume" value={bitPerfect ? 1 : state.volume}
            disabled={bitPerfect} title={bitPerfect ? 'Bit-perfect: use your system or DAC volume' : `${Math.round(state.volume * 100)}%`}
            onChange={(e) => void setVolume(Number(e.target.value))}
          />
        </span>
      </div>
    </footer>
  )
}
```

In `PlayerBar.module.css`:
- Add `.sub { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 12px; }`.
- Add `.icon[aria-pressed='true'] { color: var(--accent); }`.
- Remove the old `.art` rule; `Artwork` brings its own styles.

The ♥ opens the playlist list directly. It uses the items of `playlistSubmenu`, so the user doesn't have to open a one-item "Add to playlist ▸" submenu first.

Update the M1 test `toggles playback through the store`. The track literal now needs `trackId: null, artist: null, album: null, artworkId: null`, which Task 18 already added. Its subtitle now shows the parent folder name, as before.

- [ ] **Step 5: Shortcuts**

Replace `app/src/renderer/lib/shortcuts.ts` with:

```ts
import { livePosition, type PlayerStore } from '../stores/player'

// Controls that own their own keyboard behaviour; global shortcuts must not steal keys from them.
const INTERACTIVE = 'button, [role=switch], [role=slider], [role=grid], [role=listbox], [role=menu], [role=dialog], a, input, select, textarea, [contenteditable="true"]'
const SEEK_STEP_MS = 5000
const VOLUME_STEP = 0.05

/** Listed in Settings (spec §5.6). M3 adds Ctrl/Cmd+L (lyrics) and Ctrl/Cmd+M (mini-player). */
export const SHORTCUTS: { keys: string; action: string }[] = [
  { keys: 'Space', action: 'Play / pause' },
  { keys: '← / →', action: 'Seek back / forward 5 s' },
  { keys: 'Shift + ← / →', action: 'Previous / next track' },
  { keys: '↑ / ↓', action: 'Volume up / down' },
  { keys: 'Ctrl/Cmd + F', action: 'Search' },
  { keys: 'Ctrl/Cmd + ,', action: 'Settings' },
]

/** Applies a global shortcut. Returns true when the event was handled. `go` navigates (Layout passes it). */
export function handleShortcut(e: KeyboardEvent, store: PlayerStore, now: number, go?: (path: string) => void): boolean {
  if (e.defaultPrevented || e.altKey) return false
  if (e.ctrlKey || e.metaKey) {
    const target = e.key === 'f' || e.key === 'F' ? '/search' : e.key === ',' ? '/settings' : null
    if (!target || !go) return false
    e.preventDefault()
    go(target)
    return true
  }
  if ((e.target as Element | null)?.closest?.(INTERACTIVE)) return false
  if (e.code === 'Space') {
    e.preventDefault()
    void store.toggle()
    return true
  }
  if (e.key === 'ArrowRight' && e.shiftKey) { void store.next(); return true }
  if (e.key === 'ArrowLeft' && e.shiftKey) { void store.prev(); return true }
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
    const dir = e.key === 'ArrowRight' ? 1 : -1
    const max = store.state.durationMs ?? Infinity
    void store.seek(Math.min(max, Math.max(0, livePosition(store, now) + dir * SEEK_STEP_MS)))
    return true
  }
  if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
    if (store.state.mode === 'bitPerfect') return false
    e.preventDefault()
    const v = store.state.volume + (e.key === 'ArrowUp' ? VOLUME_STEP : -VOLUME_STEP)
    void store.setVolume(Math.round(Math.min(1, Math.max(0, v)) * 100) / 100)
    return true
  }
  return false
}
```

In `Layout.tsx`, pass navigation into the handler:

```tsx
  const go = useGo()
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { handleShortcut(e, usePlayer.getState(), performance.now(), go) }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [go])
```

Import `useGo` from `./lib/useGo`.

- [ ] **Step 6: Settings view and route**

`app/src/renderer/views/SettingsView.tsx`:

```tsx
import { FolderPlus, RefreshCw, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DevicePicker } from '../components/DevicePicker'
import { SHORTCUTS } from '../lib/shortcuts'
import { S } from '../lib/strings'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export function SettingsView() {
  const folders = useLibrary((l) => l.folders)
  const mode = usePlayer((p) => p.state.mode)
  const expanded = useUi((u) => u.sidebarExpanded)
  const [removing, setRemoving] = useState<{ id: number; path: string } | null>(null)
  const lib = useLibrary.getState()
  return (
    <div className={v.view} style={{ maxWidth: 860 }}>
      <h1 className={v.title}>{S.settingsTitle}</h1>

      <section className={v.section}>
        <div className={v.toolbar}>
          <h2 className={v.sectionTitle}>{S.libraryFolders}</h2>
          <div className={v.actions}>
            <button className={v.ghost} onClick={() => void lib.rescan()}><RefreshCw size={16} /> {S.rescanAll}</button>
            <button className={v.primary} onClick={() => void lib.addFolder()}><FolderPlus size={16} /> {S.addFolder}</button>
          </div>
        </div>
        <ul aria-label={S.libraryFolders} style={{ listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
          {folders.map((f) => (
            <li key={f.id} style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '10px 12px', background: 'var(--raised)', borderRadius: 6 }}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="mono" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{f.path}</div>
                <div className={v.meta}>{S.trackCount(f.trackCount)} · {S.lastScan(f.lastScanAt)}{!f.available && <> · <span style={{ color: 'var(--danger)' }}>{S.unavailable}</span></>}</div>
              </div>
              <button className={v.ghost} aria-label={S.rescan} onClick={() => void lib.rescan(f.id)}><RefreshCw size={14} /></button>
              <button className={v.ghost} aria-label={S.remove} onClick={() => setRemoving({ id: f.id, path: f.path })}><Trash2 size={14} /></button>
            </li>
          ))}
        </ul>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.audio}</h2>
        <div style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: '12px 16px', alignItems: 'center' }}>
          <span className="muted">{S.outputDevice}</span><DevicePicker />
          <span className="muted">{S.outputMode}</span>
          <div role="radiogroup" aria-label={S.outputMode} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {(['processed', 'bitPerfect'] as const).map((m) => (
              <label key={m} style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <input type="radio" name="mode" checked={mode === m} onChange={() => void usePlayer.getState().setMode(m)} />
                {m === 'processed' ? S.processed : S.bitPerfect}
              </label>
            ))}
          </div>
          <span className="muted">{S.buffer}</span><span className="mono">{S.bufferInfo}</span>
        </div>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.appearance}</h2>
        <label style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <input type="checkbox" checked={expanded} onChange={(e) => useUi.getState().setSidebar(e.target.checked)} /> {S.expandedSidebar}
        </label>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.shortcuts}</h2>
        <dl style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: '6px 16px', margin: 0 }}>
          {SHORTCUTS.map((k) => <div key={k.keys} style={{ display: 'contents' }}><dt className="mono">{k.keys}</dt><dd style={{ margin: 0 }}>{k.action}</dd></div>)}
        </dl>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.about}</h2>
        <p className="muted" style={{ margin: 0, userSelect: 'text' }}>{S.licences}</p>
      </section>

      {removing && (
        <ConfirmDialog title={S.removeFolderTitle} body={S.removeFolderBody(removing.path)} confirm={S.remove}
          onCancel={() => setRemoving(null)} onConfirm={() => { const id = removing.id; setRemoving(null); void lib.removeFolder(id) }} />
      )}
    </div>
  )
}
```

The radio's accessible name is its label text, which contains "Bit-perfect". That's what the test's `/Bit-perfect/` matches.

In `lib/nav.ts`, append `{ to: '/settings', label: S.nav.settings, icon: Settings }`, importing `Settings` from lucide.

In `router.tsx`, add `const settingsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/settings', component: SettingsView })` to the tree.

- [ ] **Step 7: Run the checks**

Run: `npm --prefix app run typecheck && npm --prefix app test`
Expected: all pass, including:
- PlayerBar (4 new);
- shortcuts (2 new, plus the M1 ones);
- SettingsView (2).

Then do a manual pass. Run `npm run dev` (it builds the engine first), add `fixtures/out` as a music folder, and click through Home, Albums, an album, Search, Playlists, Queue and Settings. Confirm these:
- each view renders inside the Studio palette;
- focus rings are amber;
- Tab reaches every control in the player bar.

Record any visual defect in the task report.

- [ ] **Step 8: Commit**

```bash
git add app
git commit -m "feat(app): full player bar with shuffle/repeat and add-to-playlist, settings view and M2 shortcuts"
```

### Task 23: M2 Playwright smoke test, test isolation, CI, docs and notices

**Files:**
- Create: `app/e2e/library.spec.ts`, `app/e2e/fixture.ts`
- Modify:
  - `app/e2e/smoke.spec.ts`: an isolated data dir and `try/finally`;
  - `app/src/main/index.ts`: unpackaged runs with `AURORA_DATA_DIR` also redirect `userData`;
  - `.github/workflows/ci.yml`: a reduced benchmark on Linux;
  - `README.md`, `THIRD_PARTY_NOTICES.md`.

**Interfaces:**
- Consumes: the whole app (Tasks 17–22) and engine (Tasks 1–16), through the built Electron app and the debug engine binary.
- Produces:
  - **`app/e2e/fixture.ts`:** `launchAurora({ dataDir, musicDir?, speed? }) -> Promise<{ app, win }>`, which sets the engine path, the null output, `AURORA_DATA_DIR` and `AURORA_TEST_OPEN_FOLDER`; and `musicFolder(files: string[]) -> string`, a temp dir of fixture copies.
  - The M2 "Done when" Playwright smoke test (spec §9), and CI coverage for it on all three OSes.
  - **Isolation:** with `AURORA_DATA_DIR` set (unpackaged only), Electron's `userData` (settings.json, logs) also moves there. Test runs therefore never touch a developer's real library or settings.

- [ ] **Step 1: Isolate test runs from real user data**

In `app/src/main/index.ts`, right after the `protocol.registerSchemesAsPrivileged(...)` call (before `app.whenReady()`):

```ts
// Tests point everything (engine data, settings, logs) at a throwaway folder. Unpackaged builds only.
if (!app.isPackaged && process.env.AURORA_DATA_DIR) app.setPath('userData', process.env.AURORA_DATA_DIR)
```

- [ ] **Step 2: Write the e2e fixture and the library smoke test**

`app/e2e/fixture.ts`:

```ts
import { _electron as electron, type ElectronApplication, type Page } from '@playwright/test'
import { copyFileSync, mkdirSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

export const root = resolve(__dirname, '../..')
const exe = process.platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'

export function tempDir(prefix: string): string { return mkdtempSync(join(tmpdir(), prefix)) }

/** A fresh folder holding copies of generated fixtures (their file stems become the titles). */
export function musicFolder(files: string[]): string {
  const dir = join(tempDir('aurora-music-'), 'Music')
  mkdirSync(dir, { recursive: true })
  for (const f of files) copyFileSync(resolve(root, 'fixtures/out', f), join(dir, f))
  return dir
}

export async function launchAurora(o: { dataDir: string; musicDir?: string; openFiles?: string; speed?: number }): Promise<{ app: ElectronApplication; win: Page }> {
  const app = await electron.launch({
    args: [resolve(__dirname, '../out/main/index.js')],
    env: {
      ...process.env,
      AURORA_ENGINE_PATH: resolve(root, 'engine/target/debug', exe),
      AURORA_OUTPUT: 'null',
      AURORA_NULL_SPEED: String(o.speed ?? 0.5),
      AURORA_OSMEDIA: '0',
      AURORA_DATA_DIR: o.dataDir,
      ...(o.musicDir ? { AURORA_TEST_OPEN_FOLDER: o.musicDir } : {}),
      ...(o.openFiles ? { AURORA_TEST_OPEN_FILES: o.openFiles } : {}),
    },
  })
  return { app, win: await app.firstWindow() }
}
```

`app/e2e/library.spec.ts`:

```ts
import { expect, test } from '@playwright/test'
import { launchAurora, musicFolder, tempDir } from './fixture'

test('add folder → scan → albums → play → search → play next → queue → restart restores', async () => {
  const dataDir = tempDir('aurora-data-')
  const musicDir = musicFolder(['flac16_44k.flac', 'short_a.flac', 'short_b.flac'])
  // Slow null output: a 2 s track lasts 20 s, so the queue is still on track 1 when we inspect it.
  let { app, win } = await launchAurora({ dataDir, musicDir, speed: 0.1 })
  try {
    await win.getByRole('button', { name: 'Add a music folder' }).click()
    await expect(win.getByRole('heading', { name: 'Recently added' })).toBeVisible({ timeout: 30_000 })

    await win.getByRole('link', { name: 'Albums' }).click()
    await win.getByRole('list', { name: 'Albums' }).getByText('Unknown album').click()
    await expect(win.getByRole('heading', { name: 'Unknown album' })).toBeVisible()
    const tracks = win.getByRole('grid', { name: 'Unknown album' })
    await expect(tracks.getByRole('row')).toHaveCount(4) // header + 3 tracks

    await win.getByRole('button', { name: 'Play album' }).click()
    await expect(win.getByTestId('now-title')).toHaveText('flac16_44k')
    await expect(win.getByRole('button', { name: 'Pause' })).toBeVisible()

    await win.keyboard.press(process.platform === 'darwin' ? 'Meta+F' : 'Control+F')
    const search = win.getByRole('searchbox', { name: 'Search' })
    await expect(search).toBeFocused()
    await search.fill('short')
    const results = win.getByRole('grid', { name: 'Tracks' })
    await expect(results.getByText('short_b')).toBeVisible()
    await results.getByText('short_b').click({ button: 'right' })
    await win.getByRole('menuitem', { name: 'Play next' }).click()
    await expect(win.getByRole('status').filter({ hasText: 'Playing next' })).toBeVisible()

    await win.getByRole('button', { name: 'Queue' }).click()
    const queue = win.getByRole('list', { name: 'Queue' })
    await expect(queue.getByRole('listitem')).toHaveCount(4)
    await expect(queue.getByRole('listitem').nth(1)).toContainText('short_b')

    await win.getByRole('button', { name: 'Pause' }).click()
    await expect(win.getByRole('button', { name: 'Play' })).toBeVisible()
  } finally {
    await app.close()
  }

  // Relaunch on the same data: the engine restores the session paused (spec §5.2).
  ;({ app, win } = await launchAurora({ dataDir, speed: 0.1 }))
  try {
    await expect(win.getByTestId('now-title')).toHaveText('flac16_44k', { timeout: 20_000 })
    await expect(win.getByRole('button', { name: 'Play' })).toBeVisible()
    // The app reopens on the last view (/queue); the sidebar link always lands there, unlike the bar's toggle.
    await win.getByRole('link', { name: 'Queue' }).click()
    await expect(win.getByRole('list', { name: 'Queue' }).getByRole('listitem')).toHaveCount(4)
    await win.getByRole('link', { name: 'Home' }).click()
    await expect(win.getByRole('heading', { name: 'Recently added' })).toBeVisible()
  } finally {
    await app.close()
  }
})
```

Update `app/e2e/smoke.spec.ts` to use the fixture, a throwaway data dir and `try/finally`. The assertions are unchanged except that "Signal path" is a link:

```ts
import { expect, test } from '@playwright/test'
import { resolve } from 'node:path'
import { launchAurora, root, tempDir } from './fixture'

test('open → play → signal path → bit-perfect', async () => {
  const { app, win } = await launchAurora({ dataDir: tempDir('aurora-data-'), openFiles: resolve(root, 'fixtures/out/flac24_96k.flac') })
  try {
    await win.getByRole('button', { name: /open files/i }).click()
    await expect(win.getByTestId('now-title')).toHaveText('flac24_96k')
    await expect(win.getByTestId('format-badge')).toHaveText('FLAC 24/96')
    await expect(win.getByRole('button', { name: 'Pause' })).toBeVisible()

    await win.getByRole('link', { name: 'Signal path' }).click()
    await expect(win.getByTestId('signal-output')).toContainText('Null Output')

    await win.getByRole('button', { name: 'Pause' }).click()
    await expect(win.getByRole('button', { name: 'Play' })).toBeVisible()

    await win.getByRole('switch', { name: 'Bit-perfect' }).click()
    await expect(win.getByRole('slider', { name: 'Volume' })).toBeDisabled()
    await win.getByRole('button', { name: 'Play' }).click()
    await expect(win.getByTestId('bitperfect-badge')).toHaveText('Bit-perfect')
  } finally {
    await app.close()
  }
})
```

- [ ] **Step 3: Run the end-to-end tests**

Run: `npm run engine:build && npm --prefix app run e2e`
Expected: both tests pass, `smoke.spec.ts` and `library.spec.ts`.

This test is the integration check of the whole milestone. When it fails, find the failing layer with the app's own tools before changing anything:
- engine stderr goes to `<dataDir>/logs/engine.log`;
- `win.screenshot()` captures the UI at the failure point.

Fix the defect in the owning task's code, with a unit test that reproduces it. Don't add waits to the e2e test.

- [ ] **Step 4: CI**

In `.github/workflows/ci.yml`, after the "Engine tests" step, add a reduced benchmark run on Linux:

```yaml
      - name: Library benchmark (20k tracks, Linux)
        if: runner.os == 'Linux'
        env: { AURORA_BENCH_TRACKS: '20000' }
        run: cargo test --release --manifest-path engine/Cargo.toml -p library --test bench_100k -- --ignored --nocapture
```

The full 100k run is a manual exit-checklist step, because shared CI runners don't have an SSD-class disk you can guarantee. The existing e2e steps already run every spec in `app/e2e`, which now includes `library.spec.ts`.

- [ ] **Step 5: Notices and README**

Append to `THIRD_PARTY_NOTICES.md`, matching its existing format:

```markdown
## SQLite (bundled via rusqlite)

License: Public Domain

## libwebp (via the webp crate)

License: BSD-3-Clause

## lofty, jwalk, notify, image, blake3, souvlaki, rayon

License: MIT and/or Apache-2.0 (blake3: CC0-1.0 or Apache-2.0; notify: CC0-1.0 or MIT/Apache-2.0)

## TanStack Router, TanStack Virtual, dnd-kit

License: MIT
```

In `README.md`, add:
- a "Library" section covering:
  - where data lives (the OS app-data dir, with `library.db`, `artwork/`, `session.json` and `settings.json`);
  - that the engine takes `rpc --data-dir`;
  - the 2 s watcher and 30-day missing-file retention.
- a "Known limitations (M2)" section covering:
  - DSF/DFF files are indexed with file-name titles (tag reading for DSD containers comes later);
  - macOS media keys are best-effort;
  - the Windows media flyout shows the engine executable's name until the M4 installer sets an app identity.
- the benchmark command and numbers (from Task 10), if Task 10 didn't already add them.

- [ ] **Step 6: Run everything**

Run: `npm test && npm run e2e`
Expected: the engine workspace tests (`cargo test --workspace`), the app unit tests, and both e2e specs all pass.

Run: `cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings && npm --prefix app run typecheck`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add app/e2e app/src/main/index.ts .github/workflows/ci.yml README.md THIRD_PARTY_NOTICES.md
git commit -m "test(app): M2 library smoke test with session restore; isolate test data; CI benchmark; docs"
```

### Task 24: RPC framing hardening (M1 deferred findings)

**Files:**
- Modify: `engine/crates/aurora-engine/src/rpc/mod.rs` (stdin loop and `parse_line`, with unit tests)
- Test: `engine/crates/aurora-engine/tests/rpc.rs` (append)

**Interfaces:**
- Consumes: `run()` from Task 13 (with Task 14's saver hooks around the loop).
- Produces:
  - **`rpc::parse_line(line: &str) -> Result<(Option<Value>, Request), Value>`:** the error is a ready-to-send JSON-RPC error response.
    - Malformed JSON gives -32700 with `id: null`.
    - JSON that isn't a valid request object (an array, a missing or non-string `method`, `jsonrpc` ≠ "2.0") gives **-32600**, echoing the `id` when one is present.
  - **Ids:** a present `"id": null` counts as an id and is answered. Only an absent `id` makes a notification (JSON-RPC 2.0 §4).
  - **Bad bytes:** a line that isn't valid UTF-8 gets a -32700 response and the server **keeps running**. Before this task it exited.
  - These close the M1 deferred findings for Task 19 ("non-UTF-8 stdin line ends server", "-32600", "id:null treated as notification").

- [ ] **Step 1: Write the failing tests**

Append to `engine/crates/aurora-engine/tests/rpc.rs`:

```rust
#[test]
fn invalid_requests_get_32600_and_null_ids_are_answered() {
    let mut e = Engine::spawn();
    let send = |e: &mut Engine, raw: &[u8]| { e.stdin.write_all(raw).unwrap(); e.stdin.write_all(b"\n").unwrap(); e.stdin.flush().unwrap(); };
    send(&mut e, b"[]");
    let v = e.wait_for(|v| v["error"]["code"] == -32600);
    assert_eq!(v["id"], Value::Null);
    e.seen.clear();
    send(&mut e, br#"{"jsonrpc":"2.0","id":5}"#);
    let v = e.wait_for(|v| v["error"]["code"] == -32600);
    assert_eq!(v["id"], 5);
    e.seen.clear();
    send(&mut e, br#"{"jsonrpc":"2.0","id":null,"method":"player.state"}"#);
    let v = e.wait_for(|v| v.get("result").is_some_and(|r| r["status"] == "stopped"));
    assert_eq!(v["id"], Value::Null);
}

#[test]
fn non_utf8_lines_are_rejected_without_stopping_the_server() {
    let mut e = Engine::spawn();
    e.stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\xfe\"}\n").unwrap();
    e.stdin.flush().unwrap();
    let v = e.wait_for(|v| v["error"]["code"] == -32700);
    assert!(v["error"]["message"].as_str().unwrap().contains("UTF-8"));
    assert_eq!(e.call("player.state", json!({}))["result"]["status"], "stopped", "still serving");
}
```

Add unit tests at the bottom of `src/rpc/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::parse_line;
    use serde_json::json;

    #[test]
    fn classifies_lines() {
        assert_eq!(parse_line("{nope").unwrap_err()["error"]["code"], -32700);
        assert_eq!(parse_line("42").unwrap_err()["error"]["code"], -32600);
        assert_eq!(parse_line(r#"{"jsonrpc":"1.0","id":3,"method":"x"}"#).unwrap_err()["id"], 3);
        let (id, req) = parse_line(r#"{"jsonrpc":"2.0","id":null,"method":"player.state"}"#).unwrap();
        assert_eq!(id, Some(json!(null)));
        assert_eq!(req.method, "player.state");
        let (id, _) = parse_line(r#"{"jsonrpc":"2.0","method":"player.pause"}"#).unwrap();
        assert_eq!(id, None, "no id = notification");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine`
Expected:
- the unit test fails to compile (`parse_line` doesn't exist);
- `invalid_requests…` sees -32700 instead of -32600;
- `non_utf8…` times out, because the engine exits.

- [ ] **Step 3: Implement**

In `src/rpc/mod.rs`, add:

```rust
fn error_response(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": RpcError { code, message: message.into(), data: None } })
}

/// Parses one request line (see the Task 24 interface): -32700 for bad JSON, -32600 for JSON that
/// isn't a request. A present `"id": null` is an id; only a missing id makes a notification.
pub fn parse_line(line: &str) -> Result<(Option<Value>, Request), Value> {
    let v: Value = serde_json::from_str(line).map_err(|e| error_response(Value::Null, -32700, format!("parse error: {e}")))?;
    let id = v.as_object().and_then(|o| o.get("id").cloned());
    let invalid = |msg: String| error_response(id.clone().unwrap_or(Value::Null), -32600, format!("invalid request: {msg}"));
    let req: Request = serde_json::from_value(v).map_err(|e| invalid(e.to_string()))?;
    if req.jsonrpc != "2.0" { return Err(invalid("jsonrpc must be \"2.0\"".into())); }
    Ok((id, req))
}
```

Replace the stdin loop in `run()` with a byte-level reader. Keep everything before and after it, including Task 14's saver stop and final save:

```rust
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match stdin.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            out.send(&error_response(Value::Null, -32700, "parse error: request is not valid UTF-8"));
            continue;
        };
        let line = line.trim();
        if line.is_empty() { continue; }
        let (id, req) = match parse_line(line) { Ok(p) => p, Err(resp) => { out.send(&resp); continue; } };
        let result = dispatch(&ctx, &req.method, req.params);
        let Some(id) = id else { continue }; // notifications from the client get no response
        match result {
            Ok(Outcome::Reply(v)) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": v })),
            Ok(Outcome::Shutdown) => { out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null })); break; }
            Err(e) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "error": e })),
        }
    }
```

`read_until` comes from `std::io::BufRead`, which is already imported. Remove the now-unused `lines()` usage.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine`
Expected: all pass, including the 2 new rpc tests, the unit test and the existing `rpc_errors` test, which still sees -32700 for `{not json`.

Run: `cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/aurora-engine
git commit -m "fix(engine): -32600 for invalid requests, answer null ids, survive non-UTF-8 input"
```

## Milestone 2 exit checklist (maps to spec §9 M2)

**"Done when" (spec §9):**
- [ ] **100k benchmark meets its targets.** Run `cargo test --release --manifest-path engine/Cargo.toml -p library --test bench_100k -- --ignored --nocapture` on an SSD. The checks: first scan < 120 s, every query median < 50 ms. Numbers are recorded in README (Task 10).
- [ ] **Playwright smoke tests pass.** Run `npm run e2e` (Task 23): the M1 smoke test plus the M2 library test, on Windows locally and on all three OSes in CI once CI can run.

**M2 scope (spec §9), by owning task:**

| Scope item | Tasks |
|---|---|
| Scanner / watcher | 5, 9, 13 |
| DB + FTS | 2, 6, 7 |
| Artwork + palette | 4, 13 (`library.artwork`), 17 (`aurora-art://`), 20 (album header gradient) |
| All §5.3 views except expanded Now Playing (M3) | 18 (layout, signal), 20 (home, albums, artists, genres), 21 (playlists, search, queue, track info), 22 (settings) |
| Queue (set/add/insertNext/move/remove/clear/shuffle/repeat) | 11, 12, 13, 21, 22 |
| Playlists + M3U8 | 8, 13, 21 |
| Persistent player bar | 22 |
| OS media integration | 15 |
| Keyboard shortcuts (M2 subset of §5.6) | 22 |
| Session restore | 14, 16, 23 |

**Quality gates:**
- [ ] `npm test` passes: engine workspace tests and app unit tests.
- [ ] `cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings` is clean.
- [ ] `npm --prefix app run typecheck` is clean.
- [ ] `git status --porcelain shared/protocol` is empty after a fresh `cargo test`. Generated types are committed.
- [ ] The Review Focus tests exist and pass:
  - `insert_next_during_prefeed_replaces_buffered_track`, `removing_prefed_next_resyncs`;
  - `unmounted_root_marks_missing_and_remount_restores`;
  - `corrupt_db_is_moved_aside_and_folders_restored`, `newer_schema_is_refused_and_untouched`;
  - `import_resolves_relative_unicode_and_file_urls`;
  - `truncated_session_is_ignored`, `restored_queue_skips_missing_file`.

**Manual checks:**
- [ ] Media key and flyout check on Windows (Task 15, Step 7).
- [ ] Visual and keyboard pass through every view (Task 22, Step 7). Watch for amber focus rings, no trade-dress violations (§5.5), and the player bar fitting at 940 px window width.
- [ ] Write `docs/superpowers/m2-execution-notes.md` from the execution ledger: every ruling, and every deferred minor. Format it like the M1 notes.
