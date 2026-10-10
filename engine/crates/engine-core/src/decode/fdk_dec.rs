//! HE-AAC (SBR/PS) via Fraunhofer FDK AAC. fdk outputs interleaved i16 at the full SBR rate.
use std::path::Path;

use fdk_aac::dec::{Decoder, DecoderError, Transport};
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::{TimeBase, Timestamp};

use super::aac_config::AacConfig;
use super::symphonia_dec::{frames_of, map_sym_err, units_of, Probed};
use super::trim::Trimmer;
use super::{DecodeError, GaplessInfo, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};

const PCM_CAP: usize = 2048 * 8;
/// Output frames to decode and discard before a seek target (four SBR frames), so the MDCT
/// overlap and SBR/PS envelope state are warm. Measured on the HE-AAC fixture: 2048 leaves
/// ~3 dB SNR against a full decode, 4096 ~24 dB, 6144 ~113 dB, 8192 bit-exact.
const SEEK_PREROLL: u64 = 8192;
/// Parametric Stereo's decorrelator is a long all-pass/delay network: on the HE-AACv2 fixture it
/// needs about two seconds (98304 frames) to match a full decode after a seek.
const SEEK_PREROLL_PS: u64 = 98_304;

enum Source {
    Mp4 { format: Box<dyn FormatReader>, track_id: u32, tb: Option<TimeBase>, core_rate: u32 },
    Adts { data: Vec<u8>, frames: Vec<(usize, usize)>, next: usize },
}

pub struct FdkAacDecoder {
    src: Source,
    dec: Decoder,
    asc: Option<Vec<u8>>,
    pcm: Vec<i16>,
    info: SourceInfo,
    trimmer: Option<Trimmer>,
    skip: u64,
    pending: Option<PcmBlock>,
    /// Output frames per input (ADTS) frame: 2048 with SBR, 1024 without.
    per: u64,
    /// Output frames to decode and discard ahead of a seek target.
    preroll: u64,
}

fn preroll_for(codec: protocol::Codec) -> u64 {
    if codec == protocol::Codec::HeAacV2 { SEEK_PREROLL_PS } else { SEEK_PREROLL }
}

fn fdk_err(e: DecoderError) -> DecodeError { DecodeError::Corrupt(e.message().to_string()) }

/// Length of a leading ID3v2 tag (header, body and optional footer), 0 if there is none.
fn id3v2_len(data: &[u8]) -> usize {
    if data.len() >= 10 && &data[..3] == b"ID3" && data[6..10].iter().all(|&b| b & 0x80 == 0) {
        let size = data[6..10].iter().fold(0usize, |n, &b| (n << 7) | b as usize);
        let footer = if data[5] & 0x10 != 0 { 10 } else { 0 };
        (10 + size + footer).min(data.len())
    } else {
        0
    }
}

/// Frame length of a plausible ADTS header at `i`: sync 0xFFF, layer 0, sampling-frequency index
/// below 13, and a 13-bit frame_length of at least the header size (7, or 9 with a CRC).
fn adts_header_len(data: &[u8], i: usize) -> Option<usize> {
    let h = data.get(i..i + 7)?;
    if h[0] != 0xFF || h[1] & 0xF6 != 0xF0 || (h[2] >> 2) & 0x0F >= 13 { return None; }
    let len = (((h[3] & 0x03) as usize) << 11) | ((h[4] as usize) << 3) | ((h[5] as usize) >> 5);
    let header = if h[1] & 1 == 1 { 7 } else { 9 };
    (len >= header).then_some(len)
}

/// Byte ranges of every ADTS frame. Raw `.aac` files may start with an ID3v2 tag, and 0xFFF can
/// occur by chance in tag or payload bytes, so a candidate frame is accepted only when it is
/// followed by another valid header, ends exactly at EOF, or is followed by a trailing tag.
fn scan_adts(data: &[u8]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut i = id3v2_len(data);
    while i + 7 <= data.len() {
        if let Some(len) = adts_header_len(data, i) {
            let end = i + len;
            let rest = &data[end.min(data.len())..];
            let followed = adts_header_len(data, end).is_some()
                || (end == data.len())
                || (end < data.len() && (rest.starts_with(b"TAG") || rest.starts_with(b"ID3") || rest.starts_with(b"APETAGEX")));
            if end <= data.len() && followed { out.push((i, len)); i = end; continue; }
        }
        i += 1;
    }
    out
}

impl FdkAacDecoder {
    /// `cfg` comes from the container's AudioSpecificConfig. Muxers that remux raw ADTS (ffmpeg)
    /// write a plain AAC-LC config even for HE-AAC streams (implicit signaling), so the first
    /// decoded frame is authoritative for the SBR/PS flags, output rate and channel count.
    pub fn new_mp4(p: Probed, cfg: AacConfig) -> Result<Self, DecodeError> {
        let asc = p.params.extra_data.as_deref().map(<[u8]>::to_vec).ok_or_else(|| DecodeError::Unsupported("MP4 AAC without AudioSpecificConfig".into()))?;
        let mut dec = Decoder::new(Transport::Raw);
        dec.config_raw(&asc).map_err(fdk_err)?;
        let mut me = Self {
            src: Source::Mp4 { format: p.format, track_id: p.track_id, tb: p.time_base, core_rate: cfg.sample_rate },
            dec,
            asc: Some(asc),
            pcm: vec![0; PCM_CAP],
            info: SourceInfo {
                container: p.container, codec: cfg.codec(), bitrate_kbps: None,
                spec: PcmSpec { sample_rate: cfg.output_rate, channels: cfg.channels.max(1) as u16, kind: SampleKind::Int { bits: 16 } },
                total_frames: None, gapless: None, decoder_name: "fdk-aac", dsd_rate: None,
            },
            trimmer: None,
            skip: 0,
            pending: None,
            per: 0,
            preroll: SEEK_PREROLL,
        };
        // First block, still untrimmed: it tells us the real output format.
        let mut first = me.decode_next()?.ok_or_else(|| DecodeError::Unsupported("no AAC frames".into()))?;
        let out_rate = first.spec.sample_rate;
        let aot = me.dec.stream_info().aot as i32;
        let sbr = cfg.sbr || aot == 5 || aot == 29 || out_rate > cfg.sample_rate;
        let ps = cfg.ps || aot == 29 || (sbr && cfg.channels == 1 && first.spec.channels == 2);
        let core_rate = if sbr { (out_rate / 2).max(1) } else { out_rate };
        let ratio = (out_rate / core_rate) as u64;
        if let Source::Mp4 { core_rate: r, .. } = &mut me.src { *r = core_rate; }

        let (delay, valid) = p.mp4_gapless.map(|g| g.to_frames(out_rate)).unwrap_or((0, None));
        let raw_frames = p.num_frames.map(|n| frames_of(n, p.time_base, core_rate) * ratio);
        let total_frames = valid.or(raw_frames.map(|n| n.saturating_sub(delay)));
        me.info.codec = if ps { protocol::Codec::HeAacV2 } else if sbr { protocol::Codec::HeAac } else { protocol::Codec::AacLc };
        me.info.spec = first.spec;
        me.info.total_frames = total_frames;
        me.info.bitrate_kbps = total_frames.filter(|&f| f > 0).map(|f| (p.file_len as f64 * 8.0 / (f as f64 / out_rate as f64) / 1000.0).round() as u32);
        me.info.gapless = (delay > 0).then_some(GaplessInfo { delay: delay as u32, padding: 0 });
        if p.mp4_gapless.is_some() {
            let mut t = Trimmer::new(delay, valid);
            t.apply(&mut first);
            me.trimmer = Some(t);
        }
        me.preroll = preroll_for(me.info.codec);
        me.pending = (!first.is_empty()).then_some(first);
        Ok(me)
    }

    pub fn open_adts(path: &Path) -> Result<Self, DecodeError> {
        let data = std::fs::read(path)?;
        let frames = scan_adts(&data);
        if frames.is_empty() { return Err(DecodeError::Unsupported("no ADTS frames".into())); }
        let file_len = data.len() as u64;
        let n = frames.len() as u64;
        // Channel configuration the ADTS header declares (the AAC core: mono for HE-AACv2).
        let core_channels = (((data[frames[0].0 + 2] & 1) << 2) | (data[frames[0].0 + 3] >> 6)) as u16;
        let mut me = Self {
            src: Source::Adts { data, frames, next: 0 },
            dec: Decoder::new(Transport::Adts),
            asc: None,
            pcm: vec![0; PCM_CAP],
            info: SourceInfo {
                container: "aac".into(), codec: protocol::Codec::AacLc, bitrate_kbps: None,
                spec: PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Int { bits: 16 } },
                total_frames: None, gapless: None, decoder_name: "fdk-aac", dsd_rate: None,
            },
            trimmer: None,
            skip: 0,
            pending: None,
            per: 1024,
            preroll: SEEK_PREROLL,
        };
        me.prime()?;
        // Each ADTS frame decodes to stream_info.frameSize output frames (1024 LC, 2048 with SBR).
        let si = me.dec.stream_info();
        let per = si.frameSize.max(1) as u64;
        let aot = si.aot as i32;
        let ps = aot == 29 || (per == 2048 && core_channels == 1 && me.info.spec.channels == 2);
        me.info.codec = if ps { protocol::Codec::HeAacV2 } else if aot == 5 || per == 2048 { protocol::Codec::HeAac } else { protocol::Codec::AacLc };
        me.per = per;
        me.preroll = preroll_for(me.info.codec);
        me.info.total_frames = Some(n * per);
        me.info.bitrate_kbps = Some((file_len as f64 * 8.0 / ((n * per) as f64 / me.info.spec.sample_rate as f64) / 1000.0).round() as u32);
        Ok(me)
    }

    /// Decodes the first block so `info().spec` holds fdk's real output rate and channel count.
    fn prime(&mut self) -> Result<(), DecodeError> {
        let first = self.decode_next()?.ok_or_else(|| DecodeError::Unsupported("no AAC frames".into()))?;
        self.info.spec = first.spec;
        self.pending = Some(first);
        Ok(())
    }

    fn decode_frame(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        match self.dec.decode_frame(&mut self.pcm) {
            Ok(()) => {
                let si = self.dec.stream_info();
                let (ch, rate, n) = (si.numChannels.max(1) as u16, si.sampleRate as u32, self.dec.decoded_frame_size());
                let data = self.pcm[..n].iter().map(|&s| (s as i32) << 16).collect();
                Ok(Some(PcmBlock { spec: PcmSpec { sample_rate: rate, channels: ch, kind: SampleKind::Int { bits: 16 } }, data: PcmData::Int(data) }))
            }
            Err(e) if e == DecoderError::NOT_ENOUGH_BITS => Ok(None),
            Err(e) => { tracing::warn!(err = e.message(), "fdk frame error"); Ok(None) }
        }
    }

    fn decode_next(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            // `None` = ADTS input exhausted: fdk's ADTS sync keeps a few frames buffered, so keep
            // decoding (without filling) until it reports it is out of bits.
            let input: Option<Vec<u8>> = match &mut self.src {
                Source::Mp4 { format, track_id, .. } => match format.next_packet() {
                    Ok(Some(p)) if p.track_id == *track_id => Some(p.data.to_vec()),
                    Ok(Some(_)) => continue,
                    Ok(None) => return Ok(None),
                    Err(e) => return Err(map_sym_err(e)),
                },
                Source::Adts { data, frames, next } => {
                    frames.get(*next).map(|&(off, len)| {
                        *next += 1;
                        data[off..off + len].to_vec()
                    })
                }
            };
            if let Some(input) = &input { self.dec.fill(input).map_err(fdk_err)?; }
            let Some(mut block) = self.decode_frame()? else {
                if input.is_none() { return Ok(None); }
                continue;
            };
            if let Some(t) = self.trimmer.as_mut() { t.apply(&mut block); }
            if self.skip > 0 {
                let k = (self.skip as usize).min(block.frames());
                block.trim(k, 0);
                self.skip -= k as u64;
            }
            if !block.is_empty() { return Ok(Some(block)); }
        }
    }

    fn reset_decoder(&mut self) -> Result<(), DecodeError> {
        self.dec = match &self.asc {
            Some(asc) => { let mut d = Decoder::new(Transport::Raw); d.config_raw(asc).map_err(fdk_err)?; d }
            None => Decoder::new(Transport::Adts),
        };
        Ok(())
    }
}

impl TrackDecoder for FdkAacDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        if let Some(b) = self.pending.take() { return Ok(Some(b)); }
        self.decode_next()
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        self.pending = None;
        let out_rate = self.info.spec.sample_rate;
        let delay = self.info.gapless.map_or(0, |g| g.delay as u64);
        let preroll = self.preroll;
        self.reset_decoder()?;
        match &mut self.src {
            Source::Mp4 { format, track_id, tb, core_rate } => {
                // Work in raw (untrimmed) output coordinates. Land at least `preroll` frames
                // early so the decoder state is warm; the pre-roll (plus any priming the demuxer
                // landed inside) is decoded and discarded.
                let ratio = (out_rate / (*core_rate).max(1)).max(1) as u64;
                let target_raw = frame + delay;
                let ts = units_of(target_raw.saturating_sub(preroll) / ratio, *tb, *core_rate);
                let s = format.seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(ts as i64), track_id: *track_id }).map_err(map_sym_err)?;
                let landed_raw = frames_of(s.actual_ts.get().max(0) as u64, *tb, *core_rate) * ratio;
                let skip = target_raw - landed_raw.min(target_raw);
                match self.trimmer.as_mut() {
                    Some(t) => { t.seek_raw(skip, frame); self.skip = 0; }
                    None => self.skip = skip,
                }
            }
            Source::Adts { next, frames, .. } => {
                let per = self.per.max(1);
                let idx = ((frame / per).saturating_sub(preroll.div_ceil(per)) as usize).min(frames.len());
                *next = idx;
                self.skip = frame - idx as u64 * per;
            }
        }
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A syntactically valid ADTS frame (MPEG-4, LC, 24 kHz, stereo, no CRC) of `len` bytes.
    fn frame(len: usize) -> Vec<u8> {
        let mut f = vec![0u8; len];
        f[0] = 0xFF;
        f[1] = 0xF1;
        f[2] = 0x58;
        f[3] = 0x80 | ((len >> 11) & 3) as u8;
        f[4] = (len >> 3) as u8;
        f[5] = ((len & 7) << 5) as u8 | 0x1F;
        f[6] = 0xFC;
        f
    }

    #[test]
    fn scans_consecutive_frames() {
        let data: Vec<u8> = [frame(100), frame(200), frame(50)].concat();
        assert_eq!(scan_adts(&data), vec![(0, 100), (100, 200), (300, 50)]);
    }

    #[test]
    fn rejects_false_sync_in_random_data() {
        // A false 0xFF 0xF1 whose 13-bit length points at nothing that looks like a frame.
        let mut junk = vec![0x11u8; 300];
        junk[40..47].copy_from_slice(&[0xFF, 0xF1, 0x58, 0x80, 0x0C, 0x80, 0xFC]); // claims len 100
        let data: Vec<u8> = [junk, frame(120), frame(120)].concat();
        assert_eq!(scan_adts(&data), vec![(300, 120), (420, 120)]);
    }

    #[test]
    fn rejects_bad_layer_and_sample_rate_index() {
        let mut bad_layer = frame(64);
        bad_layer[1] |= 0x02;
        let mut bad_rate = frame(64);
        bad_rate[2] = (bad_rate[2] & !0x3C) | (13 << 2);
        assert!(scan_adts(&[bad_layer.clone(), bad_layer].concat()).is_empty());
        assert!(scan_adts(&[bad_rate.clone(), bad_rate].concat()).is_empty());
    }

    #[test]
    fn skips_leading_id3v2_tag_with_footer() {
        let mut tag = b"ID3\x04\x00\x10\x00\x00\x01\x00".to_vec(); // size 128, footer flag
        tag.extend(std::iter::repeat_n(0xFFu8, 128 + 10)); // payload full of sync-looking bytes
        let data: Vec<u8> = [tag, frame(80), frame(80)].concat();
        let start = 10 + 128 + 10;
        assert_eq!(scan_adts(&data), vec![(start, 80), (start + 80, 80)]);
    }

    #[test]
    fn truncated_trailing_frame_is_dropped() {
        let mut data: Vec<u8> = [frame(100), frame(100)].concat();
        data.truncate(150);
        assert_eq!(scan_adts(&data), vec![(0, 100)]);
    }
}
