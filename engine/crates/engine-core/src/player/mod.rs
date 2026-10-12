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
use crate::dsp::eq::EqShared;
use crate::dsp::resample::StreamResampler;
use crate::dsp::volume::volume_gain;
use crate::output::format::OutputFormat;
use crate::output::ring::{ring_pair, StreamShared};
use crate::output::{DeviceCaps, OutputBackend, OutputError, OutputStream, StreamInfo};
use chain::ProcessChain;
use pipeline::{plan_output, Passthrough, PipelinePlan, PlanInput, EXCLUSIVE_UNAVAILABLE};
pub use queue::{Entry, NewEntry, Queue};
use signal_path::{build_signal_path, eq_stage};

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
    SetEq(EqSettings),
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

/// A cloneable view of the latest published player state.
#[derive(Clone)]
pub struct StateReader(Arc<Mutex<PlayerState>>);
impl StateReader { pub fn get(&self) -> PlayerState { self.0.lock().unwrap_or_else(|p| p.into_inner()).clone() } }

impl PlayerHandle {
    pub fn send(&self, c: Command) { let _ = self.tx.send(c); }
    pub fn state(&self) -> PlayerState { self.state.lock().unwrap().clone() }
    pub fn state_reader(&self) -> StateReader { StateReader(self.state.clone()) }
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
    /// Published to every stream's callback; only the player thread writes it (single writer).
    eq: Arc<EqShared>,
    eq_settings: EqSettings,
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
    /// The `current` last sent in `queue.changed`, so a track change re-sends the queue.
    queue_current: Option<usize>,
    shown_idx: Option<usize>,
    resume_ms: Option<u64>,
    last_tick: Instant,
    last_underruns: (u64, Instant),
    last_devices: (Vec<DeviceInfo>, Instant),
    buffer_secs: f64,
    underrun_times: VecDeque<Instant>,
}

/// Ring size after `recent` underruns within the last 10 s (spec §6: +1 s per burst, max 8 s).
/// Longest a skip/seek/pause waits for its fade-out (the fade plus a 100 ms device buffer, with margin).
const FADE_WAIT_MAX: Duration = Duration::from_millis(300);
/// A callback this quiet isn't running (paused device, lost device, manual test backend).
const FADE_STALLED: Duration = Duration::from_millis(50);

pub fn grown_buffer(current: f64, recent: usize) -> f64 {
    if recent >= 3 { (current + 1.0).min(8.0) } else { current }
}

impl Player {
    fn new(backend: Arc<dyn OutputBackend>, cfg: PlayerConfig, events: Sender<PlayerEvent>, shared_state: Arc<Mutex<PlayerState>>, seed: u64) -> Self {
        let devices = backend.devices().unwrap_or_default();
        Self {
            backend, events, shared_state, queue: Queue::new(seed), cur: 0, status: PlaybackStatus::Stopped,
            volume: 1.0, mode: OutputMode::Processed, eq: Arc::new(EqShared::new()), eq_settings: EqSettings::default(), device: None, active: None, feeding: None, prepared: None,
            prepare_failed: None, markers: VecDeque::new(), reopen: None, pending_start: false, exclusive_denied: None, last_excl_caps: None, end_pending: false, queue_current: None, shown_idx: None, resume_ms: None,
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
                self.fade_out();
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
                self.fade_out();
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
            Command::Stop => { self.fade_out(); self.stop_internal(); self.publish(true); }
            Command::Next => match self.queue.user_next(self.cur) {
                Some(n) => self.start_at(n, 0),
                // Past the last entry: stop back at the top, as when the queue ends by itself.
                None => { self.fade_out(); self.stop_internal(); self.cur = 0; self.publish(true) }
            },
            Command::Prev => {
                if self.queue.is_empty() { return; }
                let target = if self.position_ms() > 3000 { Some(self.cur) } else { self.queue.user_prev(self.cur) };
                if let Some(t) = target { self.start_at(t, 0) }
            }
            Command::Seek(ms) => { self.fade_out(); self.seek(ms) }
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
            Command::SetEq(s) => {
                self.eq.publish(s.enabled, &s.bands, s.preamp);
                self.eq_settings = s;
                self.refresh_dsp_stages();
            }
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
        self.fade_out();
        if let Some(a) = &self.active {
            a.shared.playing.store(false, Release);
            // Resuming ramps back in from the silence the fade left.
            a.shared.fade_out.store(false, Release);
        }
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

    /// Fades the audible output to silence before the caller interrupts it (skip, seek, pause,
    /// stop), so the waveform isn't cut mid-cycle (a click). Waits for the output callback to
    /// finish the fade, giving up when the callback has stalled (a lost device) or after
    /// `FADE_WAIT_MAX`. A no-op when nothing is audible.
    fn fade_out(&mut self) {
        let Some(a) = &self.active else { return };
        let sh = &a.shared;
        if self.status != PlaybackStatus::Playing || self.pending_start || !sh.playing.load(Acquire) || sh.fade_out.load(Acquire) { return; }
        sh.faded.store(false, Release);
        sh.fade_out.store(true, Release);
        let start = Instant::now();
        let (mut fills, mut moved) = (sh.fills.load(Relaxed), Instant::now());
        while !sh.faded.load(Acquire) && start.elapsed() < FADE_WAIT_MAX && moved.elapsed() < FADE_STALLED {
            std::thread::sleep(Duration::from_millis(1));
            let f = sh.fills.load(Relaxed);
            if f != fills { (fills, moved) = (f, Instant::now()); }
        }
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

    /// DSP is allowed only in processed mode (spec §3.5: bit-perfect disables ReplayGain, EQ and crossfade).
    fn dsp_allowed(&self) -> bool { self.mode == OutputMode::Processed }

    /// The DSP stages of the signal path, in chain order.
    fn dsp_stages(&self, dsp_allowed: bool) -> Vec<Stage> {
        vec![eq_stage(&self.eq_settings, dsp_allowed)]
    }

    /// Rebuilds the DSP stages of the audible track's signal path after a setting changed, and sends it.
    fn refresh_dsp_stages(&mut self) {
        let allowed = self.dsp_allowed();
        let fresh = self.dsp_stages(allowed);
        if let Some(m) = self.markers.front_mut() {
            m.signal.stages.retain(|s| !matches!(s.kind, StageKind::ReplayGain | StageKind::Eq | StageKind::Crossfade));
            let at = m.signal.stages.iter().position(|s| s.kind == StageKind::Volume).unwrap_or(m.signal.stages.len());
            for (i, st) in fresh.into_iter().enumerate() { m.signal.stages.insert(at + i, st); }
            let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
        }
    }

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
        let (prod, mut src, shared) = ring_pair((fmt.sample_rate as f64 * self.buffer_secs) as usize, fmt);
        shared.apply_gain.store(self.software_gain() && plan.passthrough == Passthrough::None, Release);
        // The EQ filters up to 8 channels; wider streams bypass it.
        let processed = self.mode == OutputMode::Processed && plan.passthrough == Passthrough::None && fmt.channels <= 8;
        shared.apply_eq.store(processed, Release);
        src.attach_eq(self.eq.clone());
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
        self.fade_out();
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
        let dsp = self.dsp_stages(self.dsp_allowed());
        let a = self.active.as_ref().expect("stream open before feeding");
        let info = dec.info().clone();
        let signal = build_signal_path(&info, &plan, &a.info, self.effective_volume(), a.bluetooth.clone(), a.shared.underruns.load(Relaxed), dsp);
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
            // After a user seek's fade-out, the new position ramps in.
            a.shared.fade_out.store(false, Release);
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
        // A front marker the output already passed (not yet popped) is reported at its end.
        let consumed = a.shared.consumed_frames.load(Acquire).min(self.markers.get(1).map_or(u64::MAX, |n| n.start));
        m.start_ms + consumed.saturating_sub(m.start) * 1000 / m.out_rate.max(1) as u64
    }

    fn update_position(&mut self) {
        let Some(a) = &self.active else { return };
        let consumed = a.shared.consumed_frames.load(Acquire);
        let underruns = a.shared.underruns.load(Relaxed);
        let discard_to = a.shared.discard_to.load(Acquire);
        let mut changed = false;
        // Walks every marker the output has reached, so a track that began and ended between two
        // observations (a short track, a large device period, a long pump pass) is still shown and
        // can still count as played, in order, before the marker after it.
        loop {
            let next_start = self.markers.get(1).map(|n| n.start);
            let passed = next_start.is_some_and(|s| s <= consumed);
            let Some(m) = self.markers.front_mut() else { break };
            if self.shown_idx != Some(m.idx) && m.start <= consumed.max(discard_to) {
                self.shown_idx = Some(m.idx);
                self.cur = m.idx;
                m.signal.underruns = underruns;
                let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
                changed = true;
            } else if !passed && underruns != self.last_underruns.0 && self.last_underruns.1.elapsed() >= Duration::from_secs(1) {
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
                let heard_to = next_start.map_or(consumed, |s| consumed.min(s));
                let listened = m.listened_carry_ms + (heard_to - m.start) * 1000 / m.out_rate.max(1) as u64;
                if listened >= played_threshold_ms(m.duration_ms) {
                    m.reported = true;
                    if let Some(e) = self.queue.get(m.idx) {
                        let _ = self.events.send(PlayerEvent::Played { track_id: e.track_id, path: e.path.clone() });
                    }
                }
            }
            if !passed { break; }
            if changed { self.publish(true); changed = false; }
            self.markers.pop_front();
        }
        if changed || (self.status == PlaybackStatus::Playing && self.last_tick.elapsed() >= Duration::from_millis(250)) {
            self.publish(true);
        }
    }

    fn publish(&mut self, emit: bool) {
        self.last_tick = Instant::now();
        if emit && self.queue_current != (!self.queue.is_empty()).then_some(self.cur) { self.emit_queue(); }
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

    fn emit_queue(&mut self) {
        self.queue_current = (!self.queue.is_empty()).then_some(self.cur);
        let current = self.queue_current.map(|c| c as u32);
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
