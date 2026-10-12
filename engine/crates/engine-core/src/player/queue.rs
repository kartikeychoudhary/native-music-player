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
    pub album_id: Option<i64>,
    pub disc_no: Option<u32>,
    pub track_no: Option<u32>,
    pub replay_gain: protocol::ReplayGainInfo,
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
    pub album_id: Option<i64>,
    pub disc_no: Option<u32>,
    pub track_no: Option<u32>,
    pub replay_gain: protocol::ReplayGainInfo,
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
                Entry { id, path: n.path, title, track_id: n.track_id, artist: n.artist, album: n.album, artwork_id: n.artwork_id, duration_ms: n.duration_ms,
                    album_id: n.album_id, disc_no: n.disc_no, track_no: n.track_no, replay_gain: n.replay_gain }
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
