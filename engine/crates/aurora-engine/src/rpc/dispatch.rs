use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use engine_core::output::OutputBackend;
use engine_core::player::{Command, NewEntry, PlayerHandle};
use protocol::rpc::*;
use protocol::{ErrorCode, QueueChanged, ReplayGainInfo, TrackRow};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::library_methods::{self, lib_err, unavailable};
use super::library_service::LibraryService;

pub struct Ctx {
    pub player: PlayerHandle,
    pub backend: Arc<dyn OutputBackend>,
    pub queue: Arc<Mutex<QueueChanged>>,
    pub library: Option<Arc<LibraryService>>,
    /// Why the library failed to open when `--data-dir` was given (`library` is then `None`).
    pub library_error: Option<String>,
}

pub enum Outcome { Reply(Value), Shutdown }

fn err(code: i32, message: impl Into<String>) -> RpcError { RpcError { code, message: message.into(), data: None } }

pub(super) fn params<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| err(-32602, format!("invalid params: {e}")))
}

/// A queue entry carrying the library's metadata for `t`.
pub fn entry_from_row(t: TrackRow, replay_gain: ReplayGainInfo) -> NewEntry {
    NewEntry {
        path: PathBuf::from(t.path),
        title: Some(t.title),
        track_id: Some(t.id),
        artist: (!t.artist.is_empty()).then_some(t.artist),
        album: (!t.album.is_empty()).then_some(t.album),
        artwork_id: t.artwork_id,
        duration_ms: t.duration_ms,
        album_id: t.album_id,
        disc_no: t.disc_no,
        track_no: t.track_no,
        replay_gain,
    }
}

fn entries_for(ctx: &Ctx, ids: &[i64]) -> Result<Vec<NewEntry>, RpcError> {
    let svc = ctx.library.as_deref().ok_or_else(|| unavailable(ctx.library_error.as_deref()))?;
    let meta = svc.lib.queue_meta(ids).map_err(lib_err)?;
    // A queue can hold the same track twice, so every copy reads the map (no `remove`).
    Ok(svc.lib.tracks_by_ids(ids).map_err(lib_err)?.into_iter().map(|t| {
        let rg = meta.get(&t.id).map(|m| m.replay_gain.clone()).unwrap_or_default();
        entry_from_row(t, rg)
    }).collect())
}

/// Fills in what a saved session doesn't store (album order, ReplayGain) from the library.
pub fn enrich(svc: &LibraryService, items: &mut [NewEntry]) {
    let ids: Vec<i64> = items.iter().filter_map(|e| e.track_id).collect();
    let Ok(meta) = svc.lib.queue_meta(&ids) else { return };
    for e in items.iter_mut() {
        if let Some(m) = e.track_id.and_then(|id| meta.get(&id)) {
            (e.album_id, e.disc_no, e.track_no, e.replay_gain) = (m.album_id, m.disc_no, m.track_no, m.replay_gain.clone());
        }
    }
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
        other => match library_methods::dispatch(ctx.library.as_deref(), ctx.library_error.as_deref(), other, p) {
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
        let e = entry_from_row(row, ReplayGainInfo::default());
        assert_eq!((e.track_id, e.artist, e.album.as_deref(), e.artwork_id, e.duration_ms), (Some(7), None, Some("Al"), Some(3), Some(5)));
        assert_eq!(e.title.as_deref(), Some("A"));
    }

    #[test]
    fn entries_carry_album_order_and_replay_gain() {
        let row = TrackRow {
            id: 4, path: "/m/a.flac".into(), title: "A".into(), artist: "X".into(), artist_id: None, album: "Al".into(), album_id: Some(9),
            album_artist: None, track_no: Some(3), disc_no: Some(1), year: None, genre: None, duration_ms: Some(1000), codec: None,
            container: "flac".into(), bitrate_kbps: None, sample_rate: None, bit_depth: None, channels: None, artwork_id: None,
            missing: false, unplayable: None, play_count: 0, added_at: 0, last_played_at: None,
        };
        let e = entry_from_row(row, ReplayGainInfo { track_gain: Some(-3.0), ..Default::default() });
        assert_eq!((e.album_id, e.disc_no, e.track_no, e.replay_gain.track_gain), (Some(9), Some(1), Some(3), Some(-3.0)));
    }
}
