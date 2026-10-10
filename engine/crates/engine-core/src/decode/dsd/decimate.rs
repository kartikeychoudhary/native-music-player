//! DSD → PCM: a ÷8 FIR using per-byte lookup tables, then ÷2 half-band FIR stages, then ×0.5.

const STAGE1_TAPS: usize = 96; // 12 bytes of history
const HB_TAPS: usize = 63;

fn lowpass(taps: usize, cutoff: f64) -> Vec<f64> {
    // Windowed sinc (Blackman), `cutoff` in cycles/sample, unity DC gain.
    let m = (taps - 1) as f64;
    let mut h: Vec<f64> = (0..taps).map(|n| {
        let x = n as f64 - m / 2.0;
        let sinc = if x == 0.0 { 2.0 * cutoff } else { (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x) };
        let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / m).cos() + 0.08 * (4.0 * std::f64::consts::PI * n as f64 / m).cos();
        sinc * w
    }).collect();
    let sum: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= sum);
    h
}

struct Halfband { h: Vec<f64>, hist: Vec<f64>, pos: usize, phase: bool }
impl Halfband {
    fn new() -> Self { Self { h: lowpass(HB_TAPS, 0.25), hist: vec![0.0; HB_TAPS], pos: 0, phase: false } }
    fn push(&mut self, x: f64) -> Option<f64> {
        self.hist[self.pos] = x;
        self.pos = (self.pos + 1) % HB_TAPS;
        self.phase = !self.phase;
        if !self.phase { return None; }
        let mut acc = 0.0;
        for (k, &c) in self.h.iter().enumerate() {
            acc += c * self.hist[(self.pos + HB_TAPS - 1 - k) % HB_TAPS];
        }
        Some(acc)
    }
    fn reset(&mut self) { self.hist.iter_mut().for_each(|v| *v = 0.0); self.pos = 0; self.phase = false; }
}

pub struct Decimator {
    tables: Vec<[f64; 256]>,     // [byte position][byte value]; position 0 = newest byte
    hist: Vec<[u8; STAGE1_TAPS / 8]>,
    stages: Vec<Vec<Halfband>>,  // per channel
    out_rate: u32,
}

impl Decimator {
    pub fn new(channels: usize, dsd_rate: u32) -> Self {
        let stage1_rate = dsd_rate / 8;
        let target = if dsd_rate <= 2_822_400 { 88_200 } else { 176_400 };
        let n_half = (stage1_rate / target).trailing_zeros() as usize;
        // Stage-1 passband edge at half its output Nyquist; later stages finish the job.
        let h = lowpass(STAGE1_TAPS, 0.5 / 16.0);
        let mut tables = vec![[0.0f64; 256]; STAGE1_TAPS / 8];
        for (k, table) in tables.iter_mut().enumerate() {
            for (byte, slot) in table.iter_mut().enumerate() {
                // Bit j (0 = MSB, the oldest sample in the byte) of byte k uses tap 8k + 7 - j.
                *slot = (0..8).map(|j| { let bit = (byte >> (7 - j)) & 1; h[8 * k + 7 - j] * if bit == 1 { 1.0 } else { -1.0 } }).sum();
            }
        }
        Self {
            tables,
            hist: vec![[0x69; STAGE1_TAPS / 8]; channels], // 0x69 = DSD idle pattern (≈ silence)
            stages: (0..channels).map(|_| (0..n_half).map(|_| Halfband::new()).collect()).collect(),
            out_rate: stage1_rate >> n_half,
        }
    }

    pub fn output_rate(&self) -> u32 { self.out_rate }

    /// `input[ch]` holds MSB-first DSD bytes; all channels have the same length. Appends interleaved f32.
    pub fn process(&mut self, input: &[Vec<u8>], out: &mut Vec<f32>) {
        let chs = input.len();
        let len = input.first().map_or(0, Vec::len);
        let mut per_ch: Vec<Vec<f32>> = vec![Vec::with_capacity(len >> self.stages[0].len()); chs];
        for ch in 0..chs {
            for &byte in &input[ch] {
                let h = &mut self.hist[ch];
                h.copy_within(0..STAGE1_TAPS / 8 - 1, 1);
                h[0] = byte;
                let mut y: f64 = h.iter().enumerate().map(|(k, &b)| self.tables[k][b as usize]).sum();
                let mut emitted = true;
                for st in self.stages[ch].iter_mut() {
                    match st.push(y) { Some(v) => y = v, None => { emitted = false; break; } }
                }
                if emitted { per_ch[ch].push((y * 0.5) as f32); }
            }
        }
        let frames = per_ch.iter().map(Vec::len).min().unwrap_or(0);
        out.reserve(frames * chs);
        for i in 0..frames { for ch in &per_ch { out.push(ch[i]); } }
    }

    pub fn reset(&mut self) {
        self.hist.iter_mut().for_each(|h| *h = [0x69; STAGE1_TAPS / 8]);
        self.stages.iter_mut().flatten().for_each(Halfband::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::dsd::modulator::sdm2;

    fn tone(rate: u32, f: f64, secs: f64) -> Vec<u8> {
        let n = (rate as f64 * secs) as usize / 8 * 8;
        sdm2(&(0..n).map(|i| 0.5 * (2.0 * std::f64::consts::PI * f * i as f64 / rate as f64).sin()).collect::<Vec<_>>())
    }

    #[test]
    fn rates() {
        assert_eq!(Decimator::new(2, 2_822_400).output_rate(), 88_200);
        assert_eq!(Decimator::new(2, 5_644_800).output_rate(), 176_400);
        assert_eq!(Decimator::new(2, 11_289_600).output_rate(), 176_400);
    }

    #[test]
    fn dsd64_tone_survives_at_minus_6db() {
        let mut d = Decimator::new(1, 2_822_400);
        let mut out = vec![];
        d.process(&[tone(2_822_400, 1000.0, 0.5)], &mut out);
        assert_eq!(out.len(), 44_100); // 0.5 s of mono at 88.2 kHz
        let tail = &out[4096..];
        let peak = tail.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!((peak - 0.25).abs() < 0.03, "peak {peak}"); // 0.5 modulation × 0.5 gain
        // Silence check on a ~DC-free signal: mean ≈ 0.
        assert!((tail.iter().map(|&s| s as f64).sum::<f64>() / tail.len() as f64).abs() < 0.01);
    }
}
