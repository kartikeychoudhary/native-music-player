//! Decoded audio. Integer samples are LEFT-JUSTIFIED in i32 (16-bit value v is stored as v << 16),
//! which makes the conversion to any wider device format lossless and reversible.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleKind {
    Int { bits: u16 },
    Float,
    /// DSD-over-PCM: 24-bit frames carrying DSD + markers. Must never be processed.
    Dop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmSpec { pub sample_rate: u32, pub channels: u16, pub kind: SampleKind }

#[derive(Clone, Debug, PartialEq)]
pub enum PcmData { Int(Vec<i32>), Float(Vec<f32>) }

#[derive(Clone, Debug, PartialEq)]
pub struct PcmBlock { pub spec: PcmSpec, pub data: PcmData }

impl PcmBlock {
    pub fn frames(&self) -> usize {
        let n = match &self.data { PcmData::Int(v) => v.len(), PcmData::Float(v) => v.len() };
        n / self.spec.channels.max(1) as usize
    }

    pub fn is_empty(&self) -> bool { self.frames() == 0 }

    /// Removes `start` frames from the front and `end` frames from the back (saturating).
    pub fn trim(&mut self, start: usize, end: usize) {
        let ch = self.spec.channels as usize;
        let frames = self.frames();
        let s = start.min(frames);
        let e = frames.saturating_sub(end).max(s);
        match &mut self.data {
            PcmData::Int(v) => { v.truncate(e * ch); v.drain(..s * ch); }
            PcmData::Float(v) => { v.truncate(e * ch); v.drain(..s * ch); }
        }
    }

    /// Appends samples converted to f64 in [-1, 1).
    pub fn to_f64(&self, out: &mut Vec<f64>) {
        match &self.data {
            PcmData::Int(v) => out.extend(v.iter().map(|&s| s as f64 / 2_147_483_648.0)),
            PcmData::Float(v) => out.extend(v.iter().map(|&s| s as f64)),
        }
    }
}
