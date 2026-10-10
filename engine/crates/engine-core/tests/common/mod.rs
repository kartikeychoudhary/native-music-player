#![allow(dead_code)]
use std::path::PathBuf;

pub fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/out").join(name);
    assert!(p.exists(), "missing fixture {name}: run `bash fixtures/generate.sh` from the repo root");
    p
}

pub fn read_s32le(name: &str) -> Vec<i32> {
    std::fs::read(fixture(name)).unwrap().as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect()
}

pub fn read_f32le(name: &str) -> Vec<f32> {
    std::fs::read(fixture(name)).unwrap().as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

/// Frequency of the strongest bin (Goertzel scan, 10 Hz steps 20 Hz–20 kHz) of one channel.
pub fn dominant_freq(samples: &[f32], channels: usize, ch: usize, rate: u32) -> f64 {
    let x: Vec<f64> = samples.iter().skip(ch).step_by(channels).take(rate as usize).map(|&s| s as f64).collect();
    let mut best = (0.0, 0.0);
    let mut f = 20.0;
    while f < 20_000.0_f64.min(rate as f64 / 2.0) {
        let w = 2.0 * std::f64::consts::PI * f / rate as f64;
        let (mut s1, mut s2) = (0.0, 0.0);
        for &v in &x { let s0 = v + 2.0 * w.cos() * s1 - s2; s2 = s1; s1 = s0; }
        let p = s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2;
        if p > best.1 { best = (f, p); }
        f += 10.0;
    }
    best.0
}

pub fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len().max(1) as f64).sqrt()
}

/// Signal-to-noise ratio of `test` against `reference` over their common length, in dB.
pub fn snr_db(reference: &[f32], test: &[f32]) -> f64 {
    let n = reference.len().min(test.len());
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for i in 0..n {
        sig += (reference[i] as f64).powi(2);
        err += (reference[i] as f64 - test[i] as f64).powi(2);
    }
    10.0 * (sig / err.max(1e-20)).log10()
}

/// Interleaved f32 → one channel.
pub fn channel(samples: &[f32], channels: usize, ch: usize) -> Vec<f32> {
    samples.iter().skip(ch).step_by(channels).copied().collect()
}
