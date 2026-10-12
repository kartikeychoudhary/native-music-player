//! Aurora's music library: scanning, tags, artwork, SQLite storage and queries (spec §3.9).
pub mod artwork;
pub mod codec;
pub mod db;
pub mod error;
pub mod history;
pub mod lyrics;
pub mod palette;
pub mod playlists;
pub mod query;
mod rows;
pub mod scan;
pub mod search;
pub mod tags;
pub mod watch;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use protocol::Folder;
use rusqlite::{params, Connection, TransactionBehavior};

pub use error::{LibError, Result};
pub use query::QueueMeta;

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

    /// A scanner with its own connection, for a worker thread (WAL lets it write while queries read).
    pub fn scanner(&self, opts: scan::ScanOptions) -> Result<scan::Scanner> {
        Ok(scan::Scanner::new(db::connect(&self.db_path())?, self.artwork_dir(), opts))
    }

    /// The shared connection. Hidden: for this crate's modules, tests and the scan worker setup.
    #[doc(hidden)]
    pub fn conn(&self) -> MutexGuard<'_, Connection> { self.conn.lock().unwrap_or_else(|p| p.into_inner()) }

    /// Folder rows matching `filter` (SQL after `WHERE`). `available` is filled in by the callers
    /// after they release the connection: a stat of an unplugged network drive can take seconds.
    fn folder_rows(conn: &Connection, filter: &str, args: impl rusqlite::Params) -> Result<Vec<Folder>> {
        let sql = format!(
            "SELECT f.id, f.path, f.last_scan_at,
                    (SELECT COUNT(*) FROM tracks t WHERE t.folder_id = f.id AND t.missing_since IS NULL)
             FROM folders f WHERE {filter} ORDER BY f.path"
        );
        let rows = conn
            .prepare(&sql)?
            .query_map(args, |r| Ok(Folder { id: r.get(0)?, path: r.get(1)?, last_scan_at: r.get(2)?, track_count: r.get::<_, i64>(3)? as u64, available: false }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn with_availability(mut f: Folder) -> Folder {
        f.available = Path::new(&f.path).is_dir();
        f
    }

    pub fn folder(&self, id: i64) -> Result<Folder> {
        let row = Self::folder_rows(&self.conn(), "f.id = ?1", [id])?.pop();
        row.map(Self::with_availability).ok_or_else(|| LibError::NotFound(format!("folder {id}")))
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let rows = Self::folder_rows(&self.conn(), "1", [])?;
        Ok(rows.into_iter().map(Self::with_availability).collect())
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
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
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
