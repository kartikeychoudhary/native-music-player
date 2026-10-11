//! Owns the library on the engine side: a scan worker thread (its own DB connection), the folder
//! watcher, and the notifications they produce.
use std::collections::HashSet;
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
struct Jobs {
    /// The folder being scanned right now.
    scanning: Option<i64>,
    /// Folders being or already removed: their queued jobs are dropped and their scan errors are
    /// expected (rows vanish under the scan), not reported.
    removed: HashSet<i64>,
}

#[derive(Default)]
struct WorkerState { jobs: Mutex<Jobs>, cancel: AtomicBool, stopping: AtomicBool }

/// How long `remove_folder` waits for a cancelled scan to stop. Cancellation lands within a file's
/// read, so this is a safety cap; a scan that outlives it fails quietly once its folder is gone.
const CANCEL_WAIT: Duration = Duration::from_secs(1);

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
            {
                let mut jobs = state.jobs.lock().unwrap();
                if jobs.removed.contains(&job.folder_id) { continue; }
                jobs.scanning = Some(job.folder_id);
                state.cancel.store(false, SeqCst);
            }
            // The scanner ends every scan it started with a `done` progress event, even on error.
            let result = scanner.scan(job.folder_id, &job.roots, job.full, &mut |p| out.notify("library.scanProgress", p), &state.cancel);
            let removed = {
                let mut jobs = state.jobs.lock().unwrap();
                jobs.scanning = None;
                jobs.removed.contains(&job.folder_id)
            };
            match result {
                Ok(s) => {
                    let any = s.added + s.updated + s.missing + s.restored + s.purged > 0;
                    if any { changed(&out, &[LibraryScope::Tracks, LibraryScope::Folders]); }
                }
                Err(LibError::Cancelled) | Err(LibError::NotFound(_)) => {}
                Err(_) if removed => {}
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
        // Folder ids can be reused after a removal.
        self.state.jobs.lock().unwrap().removed.remove(&f.id);
        let root = PathBuf::from(&f.path);
        self.roots.lock().unwrap().push((f.id, root.clone()));
        if let Some(w) = self.watcher.lock().unwrap().as_mut() && let Err(e) = w.watch(&root) {
            tracing::warn!(root = %root.display(), error = %e, "not watching folder");
        }
        self.queue_scan(f.id, vec![], true);
        changed(&self.out, &[LibraryScope::Folders]);
        Ok(f)
    }

    /// Cancels the folder's scan (and drops its queued ones), deletes it from the library, then stops
    /// watching it.
    pub fn remove_folder(&self, id: i64) -> Result<(), LibError> {
        let root = PathBuf::from(self.lib.folder(id)?.path);
        let scanning = {
            let mut jobs = self.state.jobs.lock().unwrap();
            jobs.removed.insert(id);
            let scanning = jobs.scanning == Some(id);
            if scanning { self.state.cancel.store(true, SeqCst); }
            scanning
        };
        if scanning {
            let deadline = Instant::now() + CANCEL_WAIT;
            while self.state.jobs.lock().unwrap().scanning == Some(id) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if let Err(e) = self.lib.remove_folder(id) {
            self.state.jobs.lock().unwrap().removed.remove(&id);
            return Err(e);
        }
        if let Some(w) = self.watcher.lock().unwrap().as_mut() { let _ = w.unwatch(&root); }
        self.roots.lock().unwrap().retain(|(f, _)| *f != id);
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
