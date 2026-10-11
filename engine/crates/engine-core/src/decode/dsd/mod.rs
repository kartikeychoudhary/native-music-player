pub mod decimate;
pub mod dff;
pub mod dop;
pub mod dsf;
#[cfg(any(test, feature = "test-support"))]
pub mod modulator;

use std::path::Path;

use self::{decimate::Decimator, dop::DopPacker};
use super::{DecodeError, DsdOutput, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};

pub trait DsdSource: Send {
    fn channels(&self) -> u16;
    fn dsd_rate(&self) -> u32;
    fn total_bytes_per_channel(&self) -> u64;
    /// Fills one Vec per channel with up to `max` bytes, MSB-first. Returns bytes per channel (0 = EOF).
    fn read(&mut self, out: &mut [Vec<u8>], max: usize) -> std::io::Result<usize>;
    fn seek_byte(&mut self, byte_per_channel: u64) -> std::io::Result<()>;
}

const CHUNK: usize = 4096;

/// DSD64 (2.8224 MHz) up to DSD512; anything else is rejected at open time.
const DSD_RATES: [u32; 4] = [2_822_400, 5_644_800, 11_289_600, 22_579_200];
const MAX_CHANNELS: u16 = 8;

/// Shared header validation for the container readers.
fn validate_format(container: &str, channels: u64, rate: u64) -> Result<(u16, u32), DecodeError> {
    let rate = u32::try_from(rate).ok().filter(|r| DSD_RATES.contains(r))
        .ok_or_else(|| DecodeError::Unsupported(format!("{container}: unsupported DSD rate {rate}")))?;
    let channels = u16::try_from(channels).ok().filter(|c| (1..=MAX_CHANNELS).contains(c))
        .ok_or_else(|| DecodeError::Unsupported(format!("{container}: unsupported channel count {channels}")))?;
    Ok((channels, rate))
}

/// PCM frames for `bytes` per channel of 1-bit DSD at `dsd_rate`, decimated to `out_rate`. u128
/// math, saturated, so a crafted header's sample count can't overflow.
fn pcm_frames(bytes: u64, out_rate: u32, dsd_rate: u32) -> u64 {
    let frames = u128::from(bytes) * 8 * u128::from(out_rate) / u128::from(dsd_rate.max(1));
    u64::try_from(frames).unwrap_or(u64::MAX)
}

pub struct DsdDecoder {
    src: Box<dyn DsdSource>,
    mode: DsdOutput,
    dec: Decimator,
    dop: DopPacker,
    bufs: Vec<Vec<u8>>,
    info: SourceInfo,
}

impl DsdDecoder {
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let src = match ext.as_str() {
            "dsf" => dsf::open(path)?,
            "dff" => dff::open(path)?,
            _ => return Err(DecodeError::Unsupported(format!("not a DSD file: .{ext}"))),
        };
        let (ch, rate) = (src.channels(), src.dsd_rate());
        let dec = Decimator::new(ch as usize, rate);
        let mut info = SourceInfo {
            container: ext,
            codec: protocol::Codec::Dsd,
            bitrate_kbps: Some(rate * ch as u32 / 1000),
            spec: PcmSpec { sample_rate: dec.output_rate(), channels: ch, kind: SampleKind::Float },
            total_frames: None,
            gapless: None,
            decoder_name: "dsd",
            dsd_rate: Some(rate),
        };
        info.total_frames = Some(pcm_frames(src.total_bytes_per_channel(), dec.output_rate(), rate));
        Ok(Self { bufs: vec![Vec::new(); ch as usize], src, mode: DsdOutput::Pcm, dec, dop: DopPacker::new(), info })
    }

    fn bytes_to_frames(&self, bytes: u64) -> u64 {
        match self.mode {
            DsdOutput::Pcm => pcm_frames(bytes, self.info.spec.sample_rate, self.src.dsd_rate()),
            DsdOutput::Dop => bytes / 2,
        }
    }
}

impl TrackDecoder for DsdDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            let n = self.src.read(&mut self.bufs, CHUNK)?;
            if n == 0 { return Ok(None); }
            let block = match self.mode {
                DsdOutput::Pcm => { let mut v = Vec::new(); self.dec.process(&self.bufs, &mut v); PcmBlock { spec: self.info.spec, data: PcmData::Float(v) } }
                DsdOutput::Dop => { let mut v = Vec::new(); self.dop.pack(&self.bufs, &mut v); PcmBlock { spec: self.info.spec, data: PcmData::Int(v) } }
            };
            if !block.is_empty() { return Ok(Some(block)); }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let per_byte = self.bytes_to_frames(1_000_000) as f64 / 1_000_000.0;
        let byte = ((frame as f64 / per_byte) as u64 / 2 * 2).min(self.src.total_bytes_per_channel());
        self.src.seek_byte(byte)?;
        self.dec.reset();
        self.dop = DopPacker::new();
        Ok(self.bytes_to_frames(byte))
    }

    fn set_dsd_output(&mut self, out: DsdOutput) -> bool {
        self.mode = out;
        let rate = self.src.dsd_rate();
        self.info.spec = match out {
            DsdOutput::Pcm => PcmSpec { sample_rate: self.dec.output_rate(), channels: self.src.channels(), kind: SampleKind::Float },
            DsdOutput::Dop => PcmSpec { sample_rate: rate / 16, channels: self.src.channels(), kind: SampleKind::Dop },
        };
        self.info.total_frames = Some(self.bytes_to_frames(self.src.total_bytes_per_channel()));
        true
    }
}
