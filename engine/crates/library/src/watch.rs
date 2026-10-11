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

/// Assigns changed paths to library folders for partial rescans: each path goes to the folder that
/// contains it (others are dropped), descendants of another listed path are collapsed into it, and a
/// folder with too many paths (or whose root changed) gets an empty list meaning "scan the whole folder".
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
