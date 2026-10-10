use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use super::format::{DeviceSample, OutputFormat};

pub struct StreamShared {
    pub consumed_frames: AtomicU64,
    pub discard_to: AtomicU64,
    pub underruns: AtomicU64,
    pub playing: AtomicBool,
    pub draining: AtomicBool,
    pub device_lost: AtomicBool,
    pub apply_gain: AtomicBool,
    gain_bits: AtomicU32,
}

impl StreamShared {
    pub fn set_gain(&self, g: f32) { self.gain_bits.store(g.to_bits(), Relaxed); }
    pub fn gain(&self) -> f32 { f32::from_bits(self.gain_bits.load(Relaxed)) }
}

pub struct RingSource { cons: Consumer<u8>, shared: Arc<StreamShared>, bpf: usize, sample: DeviceSample, consumed: u64 }

pub fn ring_pair(capacity_frames: usize, format: OutputFormat) -> (Producer<u8>, RingSource, Arc<StreamShared>) {
    let bpf = format.bytes_per_frame();
    let (prod, cons) = RingBuffer::new(capacity_frames.max(1) * bpf);
    let shared = Arc::new(StreamShared {
        consumed_frames: AtomicU64::new(0), discard_to: AtomicU64::new(0), underruns: AtomicU64::new(0),
        playing: AtomicBool::new(false), draining: AtomicBool::new(false), device_lost: AtomicBool::new(false),
        apply_gain: AtomicBool::new(false), gain_bits: AtomicU32::new(1.0f32.to_bits()),
    });
    (prod, RingSource { cons, shared: shared.clone(), bpf, sample: format.sample, consumed: 0 }, shared)
}

impl RingSource {
    pub fn shared(&self) -> &Arc<StreamShared> { &self.shared }
    pub fn bytes_per_frame(&self) -> usize { self.bpf }

    /// Realtime-safe (no allocation, no locks). Returns the number of bytes taken from the ring.
    pub fn fill(&mut self, out: &mut [u8]) -> usize {
        let bpf = self.bpf;
        let discard_to = self.shared.discard_to.load(Acquire);
        if self.consumed < discard_to {
            let k = ((self.cons.slots() / bpf) as u64).min(discard_to - self.consumed) as usize;
            if k > 0 {
                if let Ok(c) = self.cons.read_chunk(k * bpf) { c.commit_all(); }
                self.consumed += k as u64;
            }
        }
        let mut n = 0;
        if self.shared.playing.load(Acquire) && self.consumed >= discard_to {
            let want = out.len() / bpf;
            let frames = want.min(self.cons.slots() / bpf);
            n = frames * bpf;
            if n > 0 {
                let chunk = self.cons.read_chunk(n).expect("slots checked above");
                let (a, b) = chunk.as_slices();
                out[..a.len()].copy_from_slice(a);
                out[a.len()..n].copy_from_slice(b);
                chunk.commit_all();
                self.consumed += frames as u64;
                if self.shared.apply_gain.load(Relaxed) { apply_gain(&mut out[..n], self.sample, self.shared.gain()); }
            }
            if frames < want && !self.shared.draining.load(Relaxed) { self.shared.underruns.fetch_add(1, Relaxed); }
        }
        out[n..].fill(0);
        self.shared.consumed_frames.store(self.consumed, Release);
        n
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
}
