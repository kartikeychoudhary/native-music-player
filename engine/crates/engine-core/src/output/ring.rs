use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use super::format::{DeviceSample, OutputFormat};
use crate::dsp::eq::{EqDesign, EqShared, EqState};

/// Length of the fade applied when playback is interrupted (skip, seek, pause, stop).
pub const FADE_MS: u64 = 10;

pub struct StreamShared {
    pub consumed_frames: AtomicU64,
    pub discard_to: AtomicU64,
    pub underruns: AtomicU64,
    pub playing: AtomicBool,
    pub draining: AtomicBool,
    pub device_lost: AtomicBool,
    pub apply_gain: AtomicBool,
    /// The player sets this for streams opened in processed mode; without it the EQ never runs.
    pub apply_eq: AtomicBool,
    /// Set by the player before it interrupts audible audio: the callback ramps the output down
    /// over `FADE_MS`, then writes silence without consuming. Clearing it ramps audio back in.
    pub fade_out: AtomicBool,
    /// The fade has finished and enough silence followed it to have reached the device.
    pub faded: AtomicBool,
    /// Callback invocations, so the player can tell a running callback from a stalled one.
    pub fills: AtomicU64,
    gain_bits: AtomicU32,
}

impl StreamShared {
    pub fn set_gain(&self, g: f32) { self.gain_bits.store(g.to_bits(), Relaxed); }
    pub fn gain(&self) -> f32 { f32::from_bits(self.gain_bits.load(Relaxed)) }
}

pub struct RingSource {
    cons: Consumer<u8>, shared: Arc<StreamShared>, bpf: usize, sample: DeviceSample, consumed: u64,
    /// Fade gain (1.0 = untouched, so playback outside a fade stays bit-exact) and its per-frame step.
    level: f32,
    step: f32,
    /// Largest request seen: the silence after a fade must outlast the device's buffer.
    max_request: usize,
    silent_frames: usize,
    was_fading: bool,
    /// EQ settings published by the player, the design built from them for this stream's rate,
    /// and the filter memory. `eq_seq` is the seqlock sequence the design was built from.
    eq: Option<Arc<EqShared>>,
    eq_design: EqDesign,
    eq_state: EqState,
    eq_seq: u64,
    eq_on: bool,
    rate: u32,
}

pub fn ring_pair(capacity_frames: usize, format: OutputFormat) -> (Producer<u8>, RingSource, Arc<StreamShared>) {
    let bpf = format.bytes_per_frame();
    let (prod, cons) = RingBuffer::new(capacity_frames.max(1) * bpf);
    let shared = Arc::new(StreamShared {
        consumed_frames: AtomicU64::new(0), discard_to: AtomicU64::new(0), underruns: AtomicU64::new(0),
        playing: AtomicBool::new(false), draining: AtomicBool::new(false), device_lost: AtomicBool::new(false),
        apply_gain: AtomicBool::new(false), apply_eq: AtomicBool::new(false), fade_out: AtomicBool::new(false), faded: AtomicBool::new(false), fills: AtomicU64::new(0),
        gain_bits: AtomicU32::new(1.0f32.to_bits()),
    });
    let step = 1.0 / (format.sample_rate as u64 * FADE_MS / 1000).max(1) as f32;
    let source = RingSource { cons, shared: shared.clone(), bpf, sample: format.sample, consumed: 0, level: 1.0, step, max_request: 0, silent_frames: 0, was_fading: false,
        eq: None, eq_design: EqDesign::new(format.sample_rate, &[0.0; 10], 0.0), eq_state: EqState::new(format.channels as usize),
        eq_seq: u64::MAX, eq_on: false, rate: format.sample_rate,
    };
    (prod, source, shared)
}

impl RingSource {
    pub fn shared(&self) -> &Arc<StreamShared> { &self.shared }
    pub fn bytes_per_frame(&self) -> usize { self.bpf }

    pub fn attach_eq(&mut self, eq: Arc<EqShared>) { self.eq = Some(eq); self.eq_seq = u64::MAX; }

    /// Applies the current EQ to `buf`. A changed setting is picked up here: the design is rebuilt
    /// for this stream's rate (no allocation) and the filter memory is kept, so changes are smooth.
    fn eq_step(&mut self, buf: &mut [u8]) {
        let Some(eq) = &self.eq else { return };
        if eq.seq() != self.eq_seq && let Some(s) = eq.read() {
            self.eq_seq = s.seq;
            self.eq_on = s.enabled;
            self.eq_design = EqDesign::new(self.rate, &s.bands, s.preamp);
        }
        if self.eq_on { self.eq_state.process(&self.eq_design, buf, self.sample); }
    }

    /// Realtime-safe (no allocation, no locks). Returns the number of bytes taken from the ring.
    pub fn fill(&mut self, out: &mut [u8]) -> usize {
        let bpf = self.bpf;
        self.shared.fills.fetch_add(1, Relaxed);
        self.max_request = self.max_request.max(out.len() / bpf);
        let fading = self.shared.fade_out.load(Acquire);
        if fading && !self.was_fading { self.silent_frames = 0; }
        self.was_fading = fading;
        let discard_to = self.shared.discard_to.load(Acquire);
        let mut discarded = false;
        if self.consumed < discard_to {
            let k = ((self.cons.slots() / bpf) as u64).min(discard_to - self.consumed) as usize;
            if k > 0 {
                if let Ok(c) = self.cons.read_chunk(k * bpf) { c.commit_all(); }
                self.consumed += k as u64;
                discarded = true;
            }
        }
        let mut n = 0;
        if self.shared.playing.load(Acquire) && self.consumed >= discard_to {
            let mut want = out.len() / bpf;
            // A fade consumes only the frames it ramps over, so the position holds where it went silent.
            if fading { want = want.min((self.level / self.step).ceil() as usize); }
            let frames = want.min(self.cons.slots() / bpf);
            n = frames * bpf;
            if n > 0 {
                // Cannot fail (n <= slots); `if let` keeps panics off the realtime path.
                if let Ok(chunk) = self.cons.read_chunk(n) {
                    let (a, b) = chunk.as_slices();
                    out[..a.len()].copy_from_slice(a);
                    out[a.len()..n].copy_from_slice(b);
                    chunk.commit_all();
                    self.consumed += frames as u64;
                    if self.shared.apply_eq.load(Relaxed) { self.eq_step(&mut out[..n]); }
                    if self.shared.apply_gain.load(Relaxed) { apply_gain(&mut out[..n], self.sample, self.shared.gain()); }
                    if fading || self.level < 1.0 { self.ramp(&mut out[..n], fading); }
                } else {
                    n = 0;
                }
            }
            if fading {
                // Whatever couldn't be ramped (nothing buffered) is silence already.
                if n == 0 { self.level = 0.0; }
                if self.level <= 0.0 {
                    self.silent_frames += out.len() / bpf - n / bpf;
                    if self.silent_frames >= 2 * self.max_request { self.shared.faded.store(true, Release); }
                }
            } else if frames < want && !discarded && !self.shared.draining.load(Relaxed) {
                // A fill that just discarded (seek) is not a dropout.
                self.shared.underruns.fetch_add(1, Relaxed);
            }
        }
        out[n..].fill(0);
        self.shared.consumed_frames.store(self.consumed, Release);
        n
    }

    /// Scales `buf` frame by frame while moving the fade level toward silence (`down`) or unity.
    fn ramp(&mut self, buf: &mut [u8], down: bool) {
        for frame in buf.chunks_exact_mut(self.bpf) {
            self.level = if down { (self.level - self.step).max(0.0) } else { (self.level + self.step).min(1.0) };
            apply_gain(frame, self.sample, self.level);
        }
    }
}

fn apply_gain(buf: &mut [u8], s: DeviceSample, g: f32) {
    if g == 1.0 { return; }
    match s {
        DeviceSample::F32 => for c in buf.as_chunks_mut::<4>().0 {
            let v = f32::from_le_bytes(*c) * g;
            *c = v.to_le_bytes();
        },
        DeviceSample::I16 => for c in buf.as_chunks_mut::<2>().0 {
            let v = (i16::from_le_bytes(*c) as f32 * g) as i16;
            *c = v.to_le_bytes();
        },
        DeviceSample::I24 => for c in buf.as_chunks_mut::<3>().0 {
            let v = ((i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 * g) as i32;
            c.copy_from_slice(&v.to_le_bytes()[..3]);
        },
        DeviceSample::I32 => for c in buf.as_chunks_mut::<4>().0 {
            let v = (i32::from_le_bytes(*c) as f64 * g as f64) as i32;
            *c = v.to_le_bytes();
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::format::{DeviceSample, OutputFormat};

    fn fmt() -> OutputFormat { OutputFormat { sample_rate: 48_000, channels: 2, sample: DeviceSample::I16 } }

    fn push(p: &mut rtrb::Producer<u8>, bytes: &[u8]) {
        let mut c = p.write_chunk(bytes.len()).unwrap();
        let (a, b) = c.as_mut_slices();
        a.copy_from_slice(&bytes[..a.len()]);
        b.copy_from_slice(&bytes[a.len()..]);
        c.commit_all();
    }

    #[test]
    fn copies_whole_frames_and_zero_fills_with_underrun() {
        let (mut p, mut s, sh) = ring_pair(16, fmt());
        sh.playing.store(true, Relaxed);
        push(&mut p, &[1, 2, 3, 4, 5, 6, 7, 8, 9]); // 2 frames + 1 stray byte
        let mut out = [0xEEu8; 16];
        assert_eq!(s.fill(&mut out), 8);
        assert_eq!(&out[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(out[8..].iter().all(|&b| b == 0));
        assert_eq!(sh.underruns.load(Relaxed), 1);
        assert_eq!(sh.consumed_frames.load(Relaxed), 2);
    }

    #[test]
    fn paused_outputs_silence_and_keeps_data() {
        let (mut p, mut s, sh) = ring_pair(16, fmt());
        push(&mut p, &[1; 8]);
        let mut out = [9u8; 8];
        assert_eq!(s.fill(&mut out), 0);
        assert_eq!(out, [0; 8]);
        sh.playing.store(true, Relaxed);
        assert_eq!(s.fill(&mut out), 8);
    }

    #[test]
    fn discard_to_drops_queued_frames_only() {
        let (mut p, mut s, sh) = ring_pair(16, fmt());
        sh.playing.store(true, Relaxed);
        push(&mut p, &[1; 12]);      // frames 0..3 (old)
        sh.discard_to.store(3, Release);
        push(&mut p, &[2; 8]);       // frames 3..5 (new)
        let mut out = [0u8; 8];
        assert_eq!(s.fill(&mut out), 8);
        assert_eq!(out, [2; 8]);
        assert_eq!(sh.consumed_frames.load(Relaxed), 5);
    }

    #[test]
    fn draining_does_not_count_underruns() {
        let (_p, mut s, sh) = ring_pair(16, fmt());
        sh.playing.store(true, Relaxed);
        sh.draining.store(true, Relaxed);
        s.fill(&mut [0u8; 8]);
        assert_eq!(sh.underruns.load(Relaxed), 0);
    }

    #[test]
    fn gain_applies_only_when_enabled() {
        let f = OutputFormat { sample_rate: 48_000, channels: 1, sample: DeviceSample::F32 };
        let (mut p, mut s, sh) = ring_pair(16, f);
        sh.playing.store(true, Relaxed);
        sh.set_gain(0.5);
        push(&mut p, &1.0f32.to_le_bytes());
        let mut out = [0u8; 4];
        s.fill(&mut out);
        assert_eq!(f32::from_le_bytes(out), 1.0);
        sh.apply_gain.store(true, Relaxed);
        push(&mut p, &1.0f32.to_le_bytes());
        s.fill(&mut out);
        assert_eq!(f32::from_le_bytes(out), 0.5);
    }

    #[test]
    fn discard_that_empties_ring_does_not_count_underrun() {
        let (mut p, mut s, sh) = ring_pair(16, fmt());
        sh.playing.store(true, Relaxed);
        push(&mut p, &[1; 12]); // frames 0..3
        sh.discard_to.store(3, Release);
        let mut out = [0xEEu8; 8];
        assert_eq!(s.fill(&mut out), 0);
        assert_eq!(out, [0; 8]);
        assert_eq!(sh.underruns.load(Relaxed), 0);
        assert_eq!(sh.consumed_frames.load(Relaxed), 3);
        // a later genuine shortfall still counts
        s.fill(&mut out);
        assert_eq!(sh.underruns.load(Relaxed), 1);
    }

    fn i16s(b: &[u8]) -> Vec<i16> { b.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect() }

    fn loud(p: &mut rtrb::Producer<u8>, frames: usize) { push(p, &10_000i16.to_le_bytes().repeat(frames * 2)); }

    #[test]
    fn fade_out_ramps_to_silence_then_holds_position() {
        let (mut p, mut s, sh) = ring_pair(8192, fmt());
        sh.playing.store(true, Relaxed);
        loud(&mut p, 4000);
        let mut out = vec![0u8; 256 * 4];
        s.fill(&mut out);
        assert!(i16s(&out).iter().all(|&v| v == 10_000), "untouched before a fade");
        sh.fade_out.store(true, Release);
        let mut heard = vec![];
        for _ in 0..8 { s.fill(&mut out); heard.extend(i16s(&out)); }
        let fade = FADE_MS * 48_000 / 1000; // frames at 48 kHz
        let left: Vec<i16> = heard.iter().step_by(2).copied().collect();
        assert!(left.windows(2).all(|w| w[1] <= w[0]), "monotonic ramp down");
        assert!(left[0] > 9_000 && left[fade as usize - 2] < 500, "{} .. {}", left[0], left[fade as usize - 2]);
        assert!(left[fade as usize..].iter().all(|&v| v == 0), "silent after the ramp");
        let held = sh.consumed_frames.load(Relaxed);
        assert_eq!(held, 256 + fade, "the ramp's frames are consumed, nothing after it");
        assert!(sh.faded.load(Acquire), "faded once silence has been written past the device buffer");
        assert_eq!(sh.underruns.load(Relaxed), 0);
    }

    #[test]
    fn audio_ramps_back_in_after_a_fade() {
        let (mut p, mut s, sh) = ring_pair(8192, fmt());
        sh.playing.store(true, Relaxed);
        loud(&mut p, 4000);
        sh.fade_out.store(true, Release);
        let mut out = vec![0u8; 1024 * 4];
        s.fill(&mut out);
        sh.fade_out.store(false, Release);
        s.fill(&mut out);
        let left: Vec<i16> = i16s(&out).into_iter().step_by(2).collect();
        assert!(left[0] < 500 && left.windows(2).all(|w| w[1] >= w[0]) && left[1023] == 10_000, "{} .. {}", left[0], left[1023]);
    }

    #[test]
    fn a_fade_with_nothing_buffered_completes() {
        let (_p, mut s, sh) = ring_pair(64, fmt());
        sh.playing.store(true, Relaxed);
        sh.fade_out.store(true, Release);
        for _ in 0..3 { s.fill(&mut [0u8; 64]); }
        assert!(sh.faded.load(Acquire));
    }

    #[test]
    fn eq_runs_only_when_the_stream_allows_it_and_it_is_enabled() {
        use crate::dsp::eq::EqShared;
        let eq = Arc::new(EqShared::new());
        let (mut p, mut s, sh) = ring_pair(8192, fmt());
        s.attach_eq(eq.clone());
        sh.playing.store(true, Relaxed);
        loud(&mut p, 1000);
        let mut out = vec![0u8; 64 * 4];
        eq.publish(true, &[0.0; 10], -12.0);
        s.fill(&mut out);
        assert!(i16s(&out).iter().all(|&v| v == 10_000), "apply_eq is off: untouched");
        sh.apply_eq.store(true, Relaxed);
        s.fill(&mut out);
        assert!(i16s(&out).iter().all(|&v| v == 2_512), "-12 dB preamp: {:?}", &i16s(&out)[..2]);
        eq.publish(false, &[0.0; 10], -12.0);
        s.fill(&mut out);
        assert!(i16s(&out).iter().all(|&v| v == 10_000), "disabled again");
    }
}
