pub mod chain;
pub mod pipeline;
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
use pipeline::{plan_output, Passthrough, PipelinePlan, PlanInput};
use signal_path::build_signal_path;

const MIN_FREE_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub enum Command {
    SetQueue { paths: Vec<PathBuf>, start: usize },
    Play(Option<u32>), Pause, Toggle, Stop, Next, Prev,
    Seek(u64), SetVolume(f32), SetMode(OutputMode), SetDevice(Option<String>), Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    State(PlayerState), SignalPath(SignalPath), Queue(QueueChanged), Error(EngineErrorEvent), DevicesChanged(Vec<DeviceInfo>), Ended,
}

#[derive(Clone, Debug)]
pub struct PlayerConfig { pub buffer_secs: f64, pub preroll_secs: f64 }
impl Default for PlayerConfig { fn default() -> Self { Self { buffer_secs: 2.0, preroll_secs: 10.0 } } }

pub struct PlayerHandle { tx: Sender<Command>, state: Arc<Mutex<PlayerState>>, join: Option<JoinHandle<()>> }

impl PlayerHandle {
    pub fn send(&self, c: Command) { let _ = self.tx.send(c); }
    pub fn state(&self) -> PlayerState { self.state.lock().unwrap().clone() }
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
    let join = std::thread::Builder::new()
        .name("aurora-player".into())
        .spawn(move || Player::new(backend, cfg, events, st).run(rx))
        .expect("spawn player thread");
    PlayerHandle { tx, state, join: Some(join) }
}

struct Entry { id: u32, path: PathBuf, title: String }

/// The open output stream and the producer side of its ring.
struct Active {
    /// Held only to keep the device open; declared first so it closes before the producer drops.
    _stream: Box<dyn OutputStream>,
    prod: rtrb::Producer<u8>,
    shared: Arc<StreamShared>,
    info: StreamInfo,
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
struct Marker { start: u64, idx: usize, start_ms: u64, out_rate: u32, signal: SignalPath, duration_ms: Option<u64> }

/// A track waiting for the ring to drain before the stream is reopened in its format.
/// `prev` is the format of the stream being replaced (for the gap note).
struct Reopen { idx: usize, dec: Box<dyn TrackDecoder>, plan: PipelinePlan, prev: Option<OutputFormat> }

struct Player {
    backend: Arc<dyn OutputBackend>,
    cfg: PlayerConfig,
    events: Sender<PlayerEvent>,
    shared_state: Arc<Mutex<PlayerState>>,
    queue: Vec<Entry>,
    next_id: u32,
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
    /// Why exclusive access was denied on this device. While set, every plan is shared; it is
    /// cleared on SetMode, SetDevice and device loss.
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

fn title_of(p: &std::path::Path) -> String { p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() }

/// Ring size after `recent` underruns within the last 10 s (spec §6: +1 s per burst, max 8 s).
pub fn grown_buffer(current: f64, recent: usize) -> f64 {
    if recent >= 3 { (current + 1.0).min(8.0) } else { current }
}

impl Player {
    fn new(backend: Arc<dyn OutputBackend>, cfg: PlayerConfig, events: Sender<PlayerEvent>, shared_state: Arc<Mutex<PlayerState>>) -> Self {
        let devices = backend.devices().unwrap_or_default();
        Self {
            backend, events, shared_state, queue: vec![], next_id: 1, cur: 0, status: PlaybackStatus::Stopped,
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
            Command::SetQueue { paths, start } => {
                self.stop_internal();
                self.queue = paths.into_iter().map(|path| { let id = self.next_id; self.next_id += 1; Entry { id, title: title_of(&path), path } }).collect();
                self.cur = start.min(self.queue.len().saturating_sub(1));
                self.emit_queue();
                self.publish(true);
            }
            Command::Play(Some(id)) => if let Some(i) = self.queue.iter().position(|e| e.id == id) { self.start_at(i, 0) },
            Command::Play(None) => match self.status {
                PlaybackStatus::Paused => self.resume(),
                PlaybackStatus::Stopped if !self.queue.is_empty() => self.start_at(self.cur, 0),
                _ => {}
            },
            Command::Pause => self.pause(),
            Command::Toggle => if self.status == PlaybackStatus::Playing { self.pause() } else { self.handle(Command::Play(None)) },
            Command::Stop => { self.stop_internal(); self.publish(true); }
            Command::Next => { if self.cur + 1 < self.queue.len() { self.start_at(self.cur + 1, 0) } else { self.stop_internal(); self.publish(true) } }
            Command::Prev => { let target = if self.position_ms() > 3000 { self.cur } else { self.cur.saturating_sub(1) }; if !self.queue.is_empty() { self.start_at(target, 0) } }
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

    fn restart_if_active(&mut self) {
        if self.status == PlaybackStatus::Stopped { return; }
        let (idx, ms, paused) = (self.cur, self.position_ms(), self.status == PlaybackStatus::Paused);
        self.start_at(idx, ms);
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
        decode::open(&self.queue[idx].path)
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
        let device = self.device.clone();
        let dev = device.as_deref();
        let shared = self.backend.caps(dev, false).unwrap_or_else(|_| DeviceCaps::fallback());
        let mut exclusive = None;
        if self.mode == OutputMode::BitPerfect && self.exclusive_denied.is_none() {
            // While our own exclusive stream is open the device is busy to a fresh probe, so reuse its caps.
            let held = self.active.as_ref().filter(|a| a.exclusive()).and_then(|a| a.excl_caps.clone());
            match held.map(Ok).unwrap_or_else(|| self.backend.caps(dev, true)) {
                Ok(c) => exclusive = Some(c),
                Err(OutputError::ExclusiveDenied(reason)) => self.exclusive_denied = Some(reason),
                Err(_) => {}
            }
        }
        self.last_excl_caps = exclusive.clone();
        let force_shared = self.exclusive_denied.is_some();
        let spec = dec.info().spec;
        let mut plan = plan_output(&PlanInput {
            source: &spec, dsd_rate: dec.info().dsd_rate, mode: if force_shared { OutputMode::Processed } else { self.mode },
            transport: self.device_transport(), shared_caps: &shared, exclusive_caps: exclusive.as_ref(), current,
        });
        if let Some(reason) = &self.exclusive_denied {
            plan.notes.push(format!("Exclusive access denied ({reason}) — using shared mode"));
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
        let excl_caps = if plan.exclusive { self.last_excl_caps.clone() } else { None };
        self.active = Some(Active { _stream: stream, prod, shared, info, excl_caps, written_bytes: 0, carry: vec![], carry_pos: 0 });
        // Underrun reporting compares against the new ring's counter, which starts at 0.
        self.last_underruns.0 = 0;
        Ok(())
    }

    /// Opens a stream for `plan`. If exclusive access is denied, remembers the denial (sticky) and
    /// re-plans in shared mode, which adds the denial note. Returns the plan actually used.
    /// The planner's notes for the failed exclusive plan describe that plan, so they aren't carried;
    /// player notes (the reopen gap note) are added by the caller to the returned plan.
    fn open_stream_with_fallback(&mut self, dec: &mut dyn TrackDecoder, plan: PipelinePlan) -> Result<PipelinePlan, OutputError> {
        match self.open_stream(&plan) {
            Ok(()) => Ok(plan),
            Err(OutputError::ExclusiveDenied(reason)) if plan.exclusive => {
                self.exclusive_denied = Some(reason);
                let shared = self.plan_for(dec, None);
                self.open_stream(&shared)?;
                Ok(shared)
            }
            Err(e) => Err(e),
        }
    }

    /// Opens the decoder at `idx` (skipping broken entries), seeks to `ms`, opens a stream and starts feeding.
    fn start_at(&mut self, idx: usize, ms: u64) {
        let keep_prepared = self.prepared.take();
        self.stop_internal();
        self.prepared = keep_prepared;
        let (mut idx, mut ms) = (idx, ms);
        loop {
            if idx >= self.queue.len() { self.finish(); return; }
            let mut dec = match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); idx += 1; ms = 0; continue; } };
            if ms > 0 {
                let frame = ms * dec.info().spec.sample_rate as u64 / 1000;
                if dec.info().total_frames.is_some_and(|t| frame >= t) { idx += 1; ms = 0; continue; }
                if let Err(e) = dec.seek(frame) { self.track_error(idx, &e); idx += 1; ms = 0; continue; }
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
    }

    fn begin_feeding(&mut self, idx: usize, dec: Box<dyn TrackDecoder>, plan: PipelinePlan, start_ms: u64, reuse: Option<StreamResampler>) {
        let a = self.active.as_ref().expect("stream open before feeding");
        let info = dec.info().clone();
        let signal = build_signal_path(&info, &plan, &a.info, self.effective_volume(), self.backend.bluetooth(&a.info), a.shared.underruns.load(Relaxed));
        self.markers.push_back(Marker {
            start: a.written_frames(), idx, start_ms, out_rate: plan.format.sample_rate, signal,
            duration_ms: info.total_frames.map(|f| f * 1000 / info.spec.sample_rate.max(1) as u64),
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
        let next = feed.idx + 1;
        if next >= self.queue.len() || self.prepared.as_ref().is_some_and(|(i, _)| *i == next) || self.prepare_failed == Some(next) { return; }
        let info = feed.dec.info();
        let Some(total) = info.total_frames else { return };
        let remaining_secs = total.saturating_sub(feed.decoded) as f64 / info.spec.sample_rate.max(1) as f64;
        if remaining_secs <= self.cfg.preroll_secs {
            match decode::open(&self.queue[next].path) {
                Ok(dec) => self.prepared = Some((next, dec)),
                Err(_) => self.prepare_failed = Some(next),
            }
        }
    }

    /// The feeding decoder hit EOF: continue gaplessly with the next track, schedule a reopen, or end.
    fn advance(&mut self) {
        let mut feed = self.feeding.take().expect("advance with feeding");
        let mut idx = feed.idx + 1;
        loop {
            if idx >= self.queue.len() {
                let mut tail = vec![];
                feed.chain.flush(&mut tail);
                self.push_bytes(tail);
                self.end_pending = true;
                if let Some(a) = &self.active { a.shared.draining.store(true, Release); }
                return;
            }
            let mut dec = match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); idx += 1; continue; } };
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
        if self.status == PlaybackStatus::Stopped || self.active.is_none() { return; }
        let idx = self.markers.front().map_or(self.cur, |m| m.idx);
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
            _ => match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); return self.start_at(idx + 1, 0); } },
        };
        let frame = ms * dec.info().spec.sample_rate as u64 / 1000;
        if dec.info().total_frames.is_some_and(|t| frame >= t) { return self.start_at(idx + 1, 0); }
        if let Err(e) = dec.seek(frame) { self.track_error(idx, &e); return self.start_at(idx + 1, 0); }
        let current = self.active.as_ref().map(|a| (a.info.format, a.exclusive()));
        let plan = self.plan_for(dec.as_mut(), current);
        if current.is_some_and(|(f, ex)| f != plan.format || ex != plan.exclusive) { return self.start_at(idx, ms); }
        self.begin_feeding(idx, dec, plan, ms, None);
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
        while self.markers.len() > 1 && self.markers[1].start <= consumed { self.markers.pop_front(); }
        let mut changed = false;
        if let Some(m) = self.markers.front_mut() {
            if self.shown_idx != Some(m.idx) && m.start <= consumed.max(a.shared.discard_to.load(Acquire)) {
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
        }
        if changed || (self.status == PlaybackStatus::Playing && self.last_tick.elapsed() >= Duration::from_millis(250)) {
            self.publish(true);
        }
    }

    fn publish(&mut self, emit: bool) {
        self.last_tick = Instant::now();
        let entry = self.queue.get(self.cur);
        let duration_ms = self.markers.front().filter(|m| m.idx == self.cur).and_then(|m| m.duration_ms);
        let state = PlayerState {
            status: self.status,
            queue_index: entry.map(|_| self.cur as u32),
            track: entry.map(|e| TrackSummary { path: e.path.to_string_lossy().into_owned(), title: e.title.clone(), duration_ms }),
            position_ms: self.position_ms(),
            duration_ms,
            volume: self.volume,
            mode: self.mode,
            device_id: self.device.clone(),
        };
        *self.shared_state.lock().unwrap() = state.clone();
        if emit { let _ = self.events.send(PlayerEvent::State(state)); }
    }

    fn emit_queue(&self) {
        let items = self.queue.iter().map(|e| QueueItem { id: e.id, path: e.path.to_string_lossy().into_owned(), title: e.title.clone() }).collect();
        let _ = self.events.send(PlayerEvent::Queue(QueueChanged { items, current: (!self.queue.is_empty()).then_some(self.cur as u32) }));
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
