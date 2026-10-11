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
