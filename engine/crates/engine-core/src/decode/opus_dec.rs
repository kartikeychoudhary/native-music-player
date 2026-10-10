//! Ogg Opus decoding via libopus. Symphonia demuxes (and puts the OpusHead pre-skip / end trim on
//! packets) but has no Opus decoder, so we decode with libopus and apply the trims ourselves.
//!
//! Symphonia 0.6's Ogg Opus mapper reports the OpusHead pre-skip only as track delay, never as
//! `trim_start` on packets (end padding does arrive as `trim_end`). So we read the pre-skip from
//! the OpusHead ourselves and drop it like a seek skip. Public frame coordinates (`total_frames`,
//! `seek`) exclude the pre-skip; internally the stream is addressed in raw coordinates.
use opus::{Channels, Decoder};
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::{TimeBase, Timestamp};

use super::symphonia_dec::{frames_of, map_sym_err, units_of, Probed};
use super::{DecodeError, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};

const RATE: u32 = 48_000;
const MAX_FRAME: usize = 5_760; // 120 ms at 48 kHz
const PREROLL: u64 = 3_840; // 80 ms decoder convergence after a seek (RFC 7845 §4.6)

pub struct OpusDecoder {
    format: Box<dyn FormatReader>,
    track_id: u32,
    tb: Option<TimeBase>,
    dec: Decoder,
    channels: usize,
    buf: Vec<f32>,
    skip: u64,
    pre_skip: u64,
    info: SourceInfo,
}

/// Pre-skip (samples at 48 kHz) from the OpusHead packet: magic(8) version(1) channels(1) pre_skip(u16 LE).
fn opus_pre_skip(extra: Option<&[u8]>) -> u64 {
    match extra {
        Some(h) if h.len() >= 12 && h.starts_with(b"OpusHead") => u16::from_le_bytes([h[10], h[11]]) as u64,
        _ => 0,
    }
}

impl OpusDecoder {
    pub fn new(p: Probed) -> Result<Self, DecodeError> {
        let channels = p.params.channels.as_ref().map_or(2, |c| c.count());
        let ch = match channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(DecodeError::Unsupported("Opus with more than 2 channels".into())),
        };
        let dec = Decoder::new(RATE, ch).map_err(|e| DecodeError::Corrupt(e.to_string()))?;
        let pre_skip = opus_pre_skip(p.params.extra_data.as_deref());
        // The demuxer's frame count includes the pre-skip.
        let total_frames = p.num_frames.map(|n| frames_of(n, p.time_base, RATE).saturating_sub(pre_skip));
        let bitrate_kbps = total_frames
            .filter(|&f| f > 0)
            .map(|f| (p.file_len as f64 * 8.0 / (f as f64 / RATE as f64) / 1000.0).round() as u32);
        Ok(Self {
            format: p.format,
            track_id: p.track_id,
            tb: p.time_base,
            dec,
            channels,
            buf: vec![0.0; MAX_FRAME * channels],
            skip: pre_skip,
            pre_skip,
            info: SourceInfo {
                container: p.container,
                codec: protocol::Codec::Opus,
                bitrate_kbps,
                spec: PcmSpec { sample_rate: RATE, channels: channels as u16, kind: SampleKind::Float },
                total_frames,
                gapless: None,
                decoder_name: "libopus",
                dsd_rate: None,
            },
        })
    }
}

impl TrackDecoder for OpusDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(e) => return Err(map_sym_err(e)),
            };
            if packet.track_id != self.track_id { continue; }
            let n = match self.dec.decode_float(&packet.data[..], &mut self.buf, false) {
                Ok(n) => n,
                Err(e) => { tracing::warn!(%e, "skipping bad Opus packet"); continue; }
            };
            let mut block = PcmBlock { spec: self.info.spec, data: PcmData::Float(self.buf[..n * self.channels].to_vec()) };
            block.trim(frames_of(packet.trim_start.get(), self.tb, RATE) as usize, frames_of(packet.trim_end.get(), self.tb, RATE) as usize);
            if self.skip > 0 {
                let k = (self.skip as usize).min(block.frames());
                block.trim(k, 0);
                self.skip -= k as u64;
            }
            if !block.is_empty() { return Ok(Some(block)); }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let target_raw = frame + self.pre_skip;
        let target = target_raw.saturating_sub(PREROLL);
        let seeked = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(units_of(target, self.tb, RATE) as i64), track_id: self.track_id })
            .map_err(map_sym_err)?;
        self.dec.reset_state().map_err(|e| DecodeError::Corrupt(e.to_string()))?;
        let landed = frames_of(seeked.actual_ts.get().max(0) as u64, self.tb, RATE).min(target_raw);
        self.skip = target_raw - landed;
        Ok(frame)
    }
}
