//! Spectrum analyzer (spec §3.6): 2048-point FFT, Hann window, 64 log-spaced bands, 30 fps.
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const FFT_SIZE: usize = 2048;
pub const BINS: usize = 64;
pub const FPS: u32 = 30;
const MIN_HZ: f64 = 30.0;
const MAX_HZ: f64 = 16_000.0;
const FLOOR_DB: f32 = -80.0;

pub struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// FFT bin range (inclusive) of each band.
    ranges: [(usize, usize); BINS],
    /// Divides |X[k]| so a full-scale sine reads 1.0 (0 dBFS).
    norm: f32,
    rate: u32,
}

impl Analyzer {
    pub fn new(rate: u32) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE).map(|n| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / (FFT_SIZE - 1) as f64).cos()) as f32).collect();
        let norm = window.iter().sum::<f32>() / 2.0;
        let hi = MAX_HZ.min(rate as f64 / 2.0);
        let half = FFT_SIZE / 2;
        let bin = |hz: f64| ((hz * FFT_SIZE as f64 / rate as f64).round() as usize).clamp(1, half);
        let edge = |i: usize| MIN_HZ * (hi / MIN_HZ).powf(i as f64 / BINS as f64);
        let ranges = std::array::from_fn(|i| { let lo = bin(edge(i)); (lo, bin(edge(i + 1)).max(lo)) });
        Analyzer { input: fft.make_input_vec(), spectrum: fft.make_output_vec(), scratch: fft.make_scratch_vec(), fft, window, ranges, norm, rate }
    }

    /// The band a frequency falls in (clamped to the first/last band).
    pub fn band_of(&self, hz: f64) -> usize {
        let hi = MAX_HZ.min(self.rate as f64 / 2.0);
        let pos = (hz.max(MIN_HZ) / MIN_HZ).ln() / (hi / MIN_HZ).ln() * BINS as f64;
        (pos.floor() as usize).min(BINS - 1)
    }

    pub fn analyze(&mut self, mono: &[f32]) -> [u8; BINS] {
        debug_assert_eq!(mono.len(), FFT_SIZE);
        for ((d, s), w) in self.input.iter_mut().zip(mono).zip(&self.window) { *d = s * w; }
        if self.fft.process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch).is_err() { return [0; BINS]; }
        std::array::from_fn(|i| {
            let (lo, hi) = self.ranges[i];
            let peak = self.spectrum[lo..=hi].iter().map(|c| c.norm()).fold(0f32, f32::max) / self.norm;
            if peak <= 0.0 { return 0; }
            let db = 20.0 * peak.log10();
            ((db - FLOOR_DB) / -FLOOR_DB * 255.0).round().clamp(0.0, 255.0) as u8
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, amp: f32) -> Vec<f32> {
        (0..FFT_SIZE).map(|i| (amp as f64 * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin()) as f32).collect()
    }

    #[test]
    fn a_full_scale_tone_peaks_in_its_band_only() {
        let mut a = Analyzer::new(44_100);
        let bins = a.analyze(&sine(44_100, 1000.0, 1.0));
        let b = a.band_of(1000.0);
        assert!(bins[b] >= 245, "{:?}", bins);
        assert!(bins[a.band_of(8000.0)] < 80, "far bands stay low: {}", bins[a.band_of(8000.0)]);
        assert!(bins.iter().enumerate().max_by_key(|(_, v)| **v).unwrap().0.abs_diff(b) <= 1);
    }

    #[test]
    fn level_maps_linearly_in_db() {
        let mut a = Analyzer::new(48_000);
        let b = a.band_of(1000.0);
        let v = a.analyze(&sine(48_000, 1000.0, 0.1))[b] as i32; // -20 dBFS -> (60/80)*255 = 191
        assert!((v - 191).abs() <= 8, "{v}");
    }

    #[test]
    fn silence_is_all_zero_and_bands_cover_30_hz_to_16_khz() {
        let mut a = Analyzer::new(44_100);
        assert_eq!(a.analyze(&vec![0.0; FFT_SIZE]), [0u8; BINS]);
        assert_eq!(a.band_of(30.0), 0);
        assert_eq!(a.band_of(15_999.0), BINS - 1);
        assert!(a.band_of(440.0) < a.band_of(1000.0));
    }
}
