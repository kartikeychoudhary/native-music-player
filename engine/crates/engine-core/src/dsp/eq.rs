//! 10-band graphic EQ (spec §3.6): RBJ peaking biquads at fixed ISO-ish centres, a preamp and
//! automatic headroom. Runs in the output callback, so processing never allocates.
use std::sync::atomic::{fence, AtomicBool, AtomicU32, AtomicU64, Ordering::*};

use protocol::EqPreset;

use crate::output::format::DeviceSample;

pub const BANDS_HZ: [f64; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const BAND_Q: f64 = 1.41;
pub const MAX_DB: f32 = 12.0;
const MAX_CHANNELS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad { pub b0: f64, pub b1: f64, pub b2: f64, pub a1: f64, pub a2: f64 }

impl Biquad {
    pub const IDENTITY: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };
}

/// RBJ "Audio EQ Cookbook" peaking filter, normalised by a0.
pub fn peaking(fs: f64, f0: f64, q: f64, gain_db: f64) -> Biquad {
    if gain_db == 0.0 || f0 >= 0.45 * fs { return Biquad::IDENTITY; }
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 / fs;
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    Biquad { b0: (1.0 + alpha * a) / a0, b1: -2.0 * cos / a0, b2: (1.0 - alpha * a) / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha / a) / a0 }
}

/// |H(e^jw)| in dB at `f`.
pub fn magnitude_db(b: &Biquad, f: f64, fs: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / fs;
    let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let (nr, ni) = (b.b0 + b.b1 * c1 + b.b2 * c2, -(b.b1 * s1 + b.b2 * s2));
    let (dr, di) = (1.0 + b.a1 * c1 + b.a2 * c2, -(b.a1 * s1 + b.a2 * s2));
    10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqDesign { pub stages: [Biquad; 10], pub active: [bool; 10], pub gain: f64 }

impl EqDesign {
    pub fn new(fs: u32, bands_db: &[f32; 10], preamp_db: f32) -> Self {
        let mut stages = [Biquad::IDENTITY; 10];
        let mut active = [false; 10];
        for (i, (&f0, &g)) in BANDS_HZ.iter().zip(bands_db).enumerate() {
            stages[i] = peaking(fs as f64, f0, BAND_Q, g.clamp(-MAX_DB, MAX_DB) as f64);
            active[i] = stages[i] != Biquad::IDENTITY;
        }
        // Automatic headroom: a boost can push a full-scale signal past 0 dBFS, so the preamp drops by the largest boost.
        let headroom = bands_db.iter().fold(0f32, |m, b| m.max(b.clamp(-MAX_DB, MAX_DB)));
        let gain_db = preamp_db.clamp(-MAX_DB, MAX_DB) - headroom;
        EqDesign { stages, active, gain: if gain_db == 0.0 { 1.0 } else { 10f64.powf(gain_db as f64 / 20.0) } }
    }

    pub fn is_identity(&self) -> bool { !self.active.iter().any(|a| *a) && self.gain == 1.0 }
}

/// Filter memory (transposed direct form II) for up to 8 interleaved channels.
pub struct EqState { channels: usize, z: [[[f64; 2]; 10]; MAX_CHANNELS] }

impl EqState {
    pub fn new(channels: usize) -> Self { EqState { channels: channels.clamp(1, MAX_CHANNELS), z: [[[0.0; 2]; 10]; MAX_CHANNELS] } }

    /// Zeroes the filter memory (e.g. when the EQ is switched back on, so stale state doesn't click). Realtime-safe.
    pub fn reset(&mut self) { self.z = [[[0.0; 2]; 10]; MAX_CHANNELS]; }

    fn run(&mut self, d: &EqDesign, ch: usize, x: f64) -> f64 {
        let mut y = x * d.gain;
        for (i, b) in d.stages.iter().enumerate() {
            if !d.active[i] { continue; }
            let z = &mut self.z[ch][i];
            let out = b.b0 * y + z[0];
            z[0] = b.b1 * y - b.a1 * out + z[1];
            z[1] = b.b2 * y - b.a2 * out;
            y = out;
        }
        y
    }

    /// Filters interleaved device-format samples in place. Realtime-safe.
    pub fn process(&mut self, d: &EqDesign, buf: &mut [u8], sample: DeviceSample) {
        if d.is_identity() { return; }
        let ch = self.channels;
        match sample {
            DeviceSample::F32 => for (i, c) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                *c = (self.run(d, i % ch, f32::from_le_bytes(*c) as f64) as f32).to_le_bytes();
            },
            DeviceSample::I16 => for (i, c) in buf.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let y = self.run(d, i % ch, i16::from_le_bytes(*c) as f64);
                *c = (y.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16).to_le_bytes();
            },
            DeviceSample::I24 => for (i, c) in buf.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let x = i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8;
                let y = self.run(d, i % ch, x as f64).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                c.copy_from_slice(&y.to_le_bytes()[..3]);
            },
            DeviceSample::I32 => for (i, c) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let y = self.run(d, i % ch, i32::from_le_bytes(*c) as f64);
                *c = (y.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32).to_le_bytes();
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqSnapshot { pub seq: u64, pub enabled: bool, pub bands: [f32; 10], pub preamp: f32 }

/// EQ settings shared from the player thread (the only writer) to output callbacks, as a seqlock:
/// a reader never blocks, and a read that overlaps a write is discarded.
pub struct EqShared { seq: AtomicU64, enabled: AtomicBool, values: [AtomicU32; 11] }

impl Default for EqShared { fn default() -> Self { Self::new() } }

impl EqShared {
    pub fn new() -> Self { EqShared { seq: AtomicU64::new(0), enabled: AtomicBool::new(false), values: std::array::from_fn(|_| AtomicU32::new(0)) } }

    pub fn seq(&self) -> u64 { self.seq.load(Acquire) }

    pub fn publish(&self, enabled: bool, bands: &[f32], preamp: f32) {
        let s = self.seq.load(Relaxed);
        self.seq.store(s + 1, Relaxed);
        fence(Release);
        self.enabled.store(enabled, Relaxed);
        for (i, v) in self.values.iter().enumerate().take(10) { v.store(bands.get(i).copied().unwrap_or(0.0).to_bits(), Relaxed); }
        self.values[10].store(preamp.to_bits(), Relaxed);
        self.seq.store(s + 2, Release);
    }

    pub fn read(&self) -> Option<EqSnapshot> {
        let s1 = self.seq.load(Acquire);
        if s1 & 1 == 1 { return None; }
        let enabled = self.enabled.load(Relaxed);
        let bands = std::array::from_fn(|i| f32::from_bits(self.values[i].load(Relaxed)));
        let preamp = f32::from_bits(self.values[10].load(Relaxed));
        fence(Acquire);
        (self.seq.load(Relaxed) == s1).then_some(EqSnapshot { seq: s1, enabled, bands, preamp })
    }
}

const PRESETS: [(&str, [f32; 10]); 6] = [
    ("Flat", [0.0; 10]),
    ("Warm", [2.0, 3.0, 2.0, 1.0, 0.0, -1.0, -1.0, -2.0, -2.0, -3.0]),
    ("Bass", [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Vocal", [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
    ("Bright", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
    ("Loudness", [5.0, 4.0, 2.0, 0.0, -1.0, -1.0, 0.0, 2.0, 4.0, 5.0]),
];

pub fn builtin_presets() -> Vec<EqPreset> {
    PRESETS.iter().map(|(name, bands)| EqPreset { name: (*name).into(), bands: bands.to_vec(), preamp: 0.0, builtin: true }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::format::DeviceSample;

    const FS: f64 = 48_000.0;

    #[test]
    fn peaking_hits_its_gain_at_the_centre_and_is_flat_far_away() {
        let b = peaking(FS, 1000.0, BAND_Q, 6.0);
        assert!((magnitude_db(&b, 1000.0, FS) - 6.0).abs() < 0.01);
        assert!(magnitude_db(&b, 20.0, FS).abs() < 0.1);
        assert!(magnitude_db(&b, 20_000.0, FS).abs() < 0.2);
        let cut = peaking(FS, 1000.0, BAND_Q, -12.0);
        assert!((magnitude_db(&cut, 1000.0, FS) + 12.0).abs() < 0.01);
    }

    #[test]
    fn zero_gain_and_bands_above_nyquist_are_identity() {
        assert_eq!(peaking(FS, 1000.0, BAND_Q, 0.0), Biquad::IDENTITY);
        assert_eq!(peaking(32_000.0, 16_000.0, BAND_Q, 6.0), Biquad::IDENTITY, "16 kHz at a 32 kHz rate can't be built");
        let mut bands = [0.0f32; 10];
        bands[9] = 6.0;
        let d = EqDesign::new(32_000, &bands, 0.0);
        assert!(!d.active[9]);
    }

    #[test]
    fn automatic_headroom_lowers_the_preamp_by_the_largest_boost() {
        let mut bands = [0.0f32; 10];
        bands[2] = 6.0;
        bands[5] = -9.0;
        let d = EqDesign::new(48_000, &bands, 2.0);
        assert!((20.0 * d.gain.log10() - (2.0 - 6.0)).abs() < 1e-9);
        assert!(EqDesign::new(48_000, &[0.0; 10], 0.0).is_identity());
    }

    fn sine_f32(freq: f64, frames: usize) -> Vec<u8> {
        (0..frames).flat_map(|i| { let s = ((2.0 * std::f64::consts::PI * freq * i as f64 / FS).sin() * 0.25) as f32; [s, s] }).flat_map(f32::to_le_bytes).collect()
    }

    fn rms_tail(bytes: &[u8]) -> f64 {
        let v: Vec<f64> = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c) as f64).collect();
        let tail = &v[v.len() / 2..]; // skip the filter's settling time
        (tail.iter().map(|x| x * x).sum::<f64>() / tail.len() as f64).sqrt()
    }

    #[test]
    fn boosting_a_band_raises_a_tone_in_it_relative_to_one_outside() {
        let tone = sine_f32(1000.0, 48_000);
        let run = |band: usize| {
            let mut bands = [0.0f32; 10];
            bands[band] = 12.0;
            let d = EqDesign::new(48_000, &bands, 0.0);
            let mut buf = tone.clone();
            EqState::new(2).process(&d, &mut buf, DeviceSample::F32);
            rms_tail(&buf)
        };
        let in_band = run(5); // 1 kHz
        let out_of_band = run(8); // 8 kHz
        let diff_db = 20.0 * (in_band / out_of_band).log10();
        assert!((diff_db - 12.0).abs() < 1.0, "{diff_db:.2} dB");
    }

    #[test]
    fn flat_design_leaves_integer_samples_untouched() {
        let d = EqDesign::new(48_000, &[0.0; 10], 0.0);
        let src: Vec<u8> = [-32768i16, -1, 0, 1, 12345, 32767].iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut buf = src.clone();
        EqState::new(2).process(&d, &mut buf, DeviceSample::I16);
        assert_eq!(buf, src);
    }

    #[test]
    fn integer_output_clamps_instead_of_wrapping() {
        let d = EqDesign::new(48_000, &[0.0; 10], 12.0);
        let mut buf: Vec<u8> = [30_000i16, -30_000].iter().flat_map(|s| s.to_le_bytes()).collect();
        EqState::new(2).process(&d, &mut buf, DeviceSample::I16);
        let out: Vec<i16> = buf.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect();
        assert_eq!(out, [32_767, -32_768]);
        let mut b24 = vec![0xFF, 0xFF, 0x3F, 0x01, 0x00, 0xC0]; // +4194303, -4194303 (24-bit, LE)
        EqState::new(2).process(&d, &mut b24, DeviceSample::I24);
        assert_eq!(b24, [0xFF, 0xFF, 0x7F, 0x00, 0x00, 0x80]);
    }

    #[test]
    fn seqlock_publishes_whole_snapshots() {
        let s = EqShared::new();
        let before = s.seq();
        let bands = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        s.publish(true, &bands, -3.0);
        let snap = s.read().unwrap();
        assert!(snap.seq > before && snap.seq.is_multiple_of(2));
        assert_eq!((snap.enabled, snap.bands, snap.preamp), (true, bands, -3.0));
    }

    #[test]
    fn builtin_presets_are_named_and_in_range() {
        let p = builtin_presets();
        assert_eq!(p.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Flat", "Warm", "Bass", "Vocal", "Bright", "Loudness"]);
        assert!(p.iter().all(|p| p.builtin && p.bands.len() == 10 && p.bands.iter().all(|b| b.abs() <= MAX_DB)));
        assert!(p[0].bands.iter().all(|b| *b == 0.0));
    }
}
