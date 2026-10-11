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

fn candidate(path: PathBuf, errored: &mut Vec<PathBuf>) -> Option<Candidate> {
    let Some((mtime, size)) = file_meta(&path) else {
        tracing::warn!(path = %path.display(), "couldn't read file metadata; leaving its index entry untouched");
        errored.push(path);
        return None;
    };
    Some(Candidate { key: path.to_string_lossy().into_owned(), path, mtime, size })
}

/// Collects audio files and folder covers under `root`. Paths that couldn't be read (walk or
/// metadata errors) go to `errored`: what the scan can't see there is unknown, not gone.
fn walk(root: &Path, files: &mut Vec<Candidate>, covers: &mut Covers, errored: &mut Vec<PathBuf>) {
    if root.is_file() {
        if tags::is_audio(root) { files.extend(candidate(root.to_path_buf(), errored)); }
        if let Some(Ok(entries)) = root.parent().map(std::fs::read_dir) {
            for e in entries.flatten() { note_cover(covers, e.path(), &e.file_name().to_string_lossy()); }
        }
        return;
    }
    if !root.is_dir() { return; }
    for entry in jwalk::WalkDir::new(root).skip_hidden(true).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                let at = e.path().unwrap_or(root).to_path_buf();
                tracing::warn!(path = %at.display(), error = %e, "couldn't walk directory; leaving its index entries untouched");
                errored.push(at);
                continue;
            }
        };
        // jwalk doesn't yield a failed `read_dir` as `Err`: it parks the error on the directory entry.
        if let Some(err) = entry.read_children.as_ref().and_then(|c| c.error()) {
            tracing::warn!(path = %entry.path().display(), error = %err, "can't read folder; leaving its index entries untouched");
            errored.push(entry.path());
        }
        if !entry.file_type().is_file() { continue; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if tags::is_audio(&path) { files.extend(candidate(path, errored)); } else { note_cover(covers, path, &name); }
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

/// Per-file artwork hashes plus the newly stored (hash, palette) rows.
type PreparedArt = (Vec<Option<String>>, Vec<(String, Palette)>);

/// Hashes each file's artwork (reading each folder cover once), stores images the DB hasn't seen
/// (in parallel) and returns each file's artwork hash plus the new (hash, palette) rows.
fn prepare_art(conn: &Connection, art_dir: &Path, reads: &mut [ReadFile]) -> Result<PreparedArt> {
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
    // Re-collecting the components normalizes separators (callers may mix slash styles on Windows), so path keys match the DB.
    let roots: Vec<PathBuf> = if roots.is_empty() { vec![PathBuf::from(&folder_root)] } else { roots.iter().map(|r| r.components().collect()).collect() };

    let mut files = vec![];
    let mut covers = Covers::new();
    let mut errored = vec![];
    for root in &roots { walk(root, &mut files, &mut covers, &mut errored); }
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
    for id in newly_missing(&existing, &seen, &errored) {
        tx.execute("UPDATE tracks SET missing_since = ?1 WHERE id = ?2", params![now, id])?;
        summary.missing += 1;
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

/// Ids of present tracks that this scan should mark missing: not seen, and not under a path the
/// scan failed to read (those are unknown, not gone).
fn newly_missing(existing: &HashMap<String, Existing>, seen: &HashSet<String>, errored: &[PathBuf]) -> Vec<i64> {
    existing
        .iter()
        .filter(|(path, e)| !e.missing && !seen.contains(*path) && !errored.iter().any(|p| Path::new(path.as_str()).starts_with(p)))
        .map(|(_, e)| e.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(id: i64, missing: bool) -> Existing { Existing { id, mtime: 0, size: 0, missing } }

    #[test]
    fn unreadable_paths_are_not_marked_missing() {
        let existing: HashMap<String, Existing> = [
            ("/m/ok/a.flac", ex(1, false)),
            ("/m/gone/b.flac", ex(2, false)),
            ("/m/locked/c.flac", ex(3, false)),
            ("/m/locked/deep/d.flac", ex(4, false)),
            ("/m/gone/old.flac", ex(5, true)),
            ("/m/lockedness/e.flac", ex(6, false)),
        ].into_iter().map(|(p, e)| (p.to_string(), e)).collect();
        let seen: HashSet<String> = ["/m/ok/a.flac".to_string()].into();
        let mut ids = newly_missing(&existing, &seen, &[PathBuf::from("/m/locked")]);
        ids.sort();
        assert_eq!(ids, vec![2, 6], "vanished files go missing; the errored dir (and below) and already-missing rows don't");
        let mut all = newly_missing(&existing, &seen, &[]);
        all.sort();
        assert_eq!(all, vec![2, 3, 4, 6]);
    }
}
