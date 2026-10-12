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

/// Rebuilds the saved queue; `enrich` then fills in what the session doesn't store (album order, ReplayGain).
pub fn restore(s: Session, enrich: impl FnOnce(&mut [NewEntry])) -> Option<Restore> {
    if s.items.is_empty() { return None; }
    let mut items: Vec<NewEntry> = s.items.into_iter().map(|i| NewEntry {
        path: PathBuf::from(i.path), title: Some(i.title), track_id: i.track_id, artist: i.artist,
        album: i.album, artwork_id: i.artwork_id, duration_ms: i.duration_ms, ..Default::default()
    }).collect();
    enrich(&mut items);
    Some(Restore {
        items,
        current: s.current,
        position_ms: s.position_ms,
        shuffle: s.shuffle,
        repeat: s.repeat,
        volume: s.volume,
        mode: s.mode,
    })
}

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
        let r = restore(loaded, |_| {}).unwrap();
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
        assert!(restore(s, |_| {}).is_none());
    }
}
