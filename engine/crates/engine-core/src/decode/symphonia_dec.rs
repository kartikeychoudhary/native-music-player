//! Symphonia-backed decoding. Gapless: the Ogg/MP3/FLAC demuxers put trim info on each packet,
//! which we apply. ISO-MP4 doesn't, so we apply the iTunSMPB/elst info via `Trimmer`. If an MP4
//! packet ever carries trims (a future Symphonia), we stop using the Trimmer to avoid trimming twice.
use std::{fs::File, path::Path};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::well_known::*;
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{TimeBase, Timestamp};

use super::mp4_gapless::{read_mp4_gapless, Mp4Gapless};
use super::trim::Trimmer;
use super::{DecodeError, GaplessInfo, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};
use protocol::Codec;

/// Raw frames to decode and discard before a seek target on the MP4 path (two AAC frames).
const SEEK_PREROLL: u64 = 2048;

pub struct Probed {
    pub format: Box<dyn FormatReader>,
    pub track_id: u32,
    pub params: AudioCodecParameters,
    pub time_base: Option<TimeBase>,
    pub num_frames: Option<u64>,
    pub container: String,
    pub file_len: u64,
    pub mp4_gapless: Option<Mp4Gapless>,
    /// Encoder padding frames at the end of the stream, as reported by the demuxer (MP3/Ogg).
    pub padding: Option<u32>,
}

pub(crate) fn map_sym_err(e: SymErr) -> DecodeError {
    match e {
        SymErr::IoError(io) => DecodeError::Io(io),
        SymErr::Unsupported(m) => DecodeError::Unsupported(m.to_string()),
        other => DecodeError::Corrupt(other.to_string()),
    }
}

fn container_of(ext: &str) -> String {
    match ext {
        "aif" | "aiff" | "aifc" => "aiff",
        "m4a" | "m4b" | "mp4" | "m4p" | "alac" => "m4a",
        "oga" | "ogg" => "ogg",
        other => other,
    }
    .to_string()
}

pub fn probe_file(path: &Path) -> Result<Probed, DecodeError> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    if file_len == 0 {
        return Err(DecodeError::Unsupported("empty file".into()));
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let container = container_of(&ext);
    let mp4_gapless = if container == "m4a" { read_mp4_gapless(&mut File::open(path)?)? } else { None };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(&ext);
    let format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| match e {
            SymErr::IoError(io) => DecodeError::Io(io),
            other => DecodeError::Unsupported(other.to_string()),
        })?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .cloned()
        .ok_or_else(|| DecodeError::Unsupported("track has no audio codec parameters".into()))?;
    let (track_id, time_base, num_frames, padding) = (track.id, track.time_base, track.num_frames, track.padding);
    Ok(Probed { format, track_id, params, time_base, num_frames, container, file_len, mp4_gapless, padding })
}

pub(crate) fn frames_of(units: u64, tb: Option<TimeBase>, rate: u32) -> u64 {
    match tb {
        Some(tb) => (units as u128 * tb.numer.get() as u128 * rate as u128 / tb.denom.get() as u128) as u64,
        None => units,
    }
}

pub(crate) fn units_of(frames: u64, tb: Option<TimeBase>, rate: u32) -> u64 {
    match tb {
        Some(tb) => (frames as u128 * tb.denom.get() as u128 / (tb.numer.get() as u128 * rate.max(1) as u128)) as u64,
        None => frames,
    }
}

pub fn codec_of(id: AudioCodecId) -> Option<Codec> {
    if id == CODEC_ID_MP3 { Some(Codec::Mp3) }
    else if id == CODEC_ID_AAC { Some(Codec::AacLc) }
    else if id == CODEC_ID_FLAC { Some(Codec::Flac) }
    else if id == CODEC_ID_ALAC { Some(Codec::Alac) }
    else if id == CODEC_ID_VORBIS { Some(Codec::Vorbis) }
    else if id == CODEC_ID_OPUS { Some(Codec::Opus) }
    else if (CODEC_ID_PCM_S32LE..=CODEC_ID_PCM_MULAW).contains(&id) { Some(Codec::Pcm) }
    else { None }
}

fn to_block(buf: &GenericAudioBufferRef<'_>, bits_hint: Option<u32>) -> PcmBlock {
    let sample_rate = buf.spec().rate();
    let channels = buf.spec().channels().count() as u16;
    match buf {
        GenericAudioBufferRef::F32(_) | GenericAudioBufferRef::F64(_) => {
            let mut v = Vec::new();
            buf.copy_to_vec_interleaved::<f32>(&mut v);
            PcmBlock { spec: PcmSpec { sample_rate, channels, kind: SampleKind::Float }, data: PcmData::Float(v) }
        }
        _ => {
            let intrinsic = match buf {
                GenericAudioBufferRef::U8(_) | GenericAudioBufferRef::S8(_) => 8,
                GenericAudioBufferRef::U16(_) | GenericAudioBufferRef::S16(_) => 16,
                GenericAudioBufferRef::U24(_) | GenericAudioBufferRef::S24(_) => 24,
                _ => 32,
            };
            let bits = bits_hint.map_or(intrinsic, |b| b.min(intrinsic)) as u16;
            let mut v = Vec::new();
            buf.copy_to_vec_interleaved::<i32>(&mut v); // Symphonia left-justifies when widening
            PcmBlock { spec: PcmSpec { sample_rate, channels, kind: SampleKind::Int { bits } }, data: PcmData::Int(v) }
        }
    }
}

pub struct SymphoniaDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    tb: Option<TimeBase>,
    rate: u32,
    bits_hint: Option<u32>,
    info: SourceInfo,
    trimmer: Option<Trimmer>,
    mp4_delay: u64,
    saw_packet_trim: bool,
    skip: u64,
    pending: Option<PcmBlock>,
    decode_errors: u32,
    /// Fallback end trim: the demuxer derives `trim_end` from packet timestamps, which drift when
    /// damaged frames are skipped, so the final packet may miss its trim. If no packet carried a
    /// `trim_end`, drop `eof_padding` frames from the end of the stream instead.
    eof_padding: usize,
    saw_end_trim: bool,
    held: Option<PcmBlock>,
}

impl SymphoniaDecoder {
    pub fn new(p: Probed) -> Result<Self, DecodeError> {
        // Symphonia 0.6 decoders trim `Packet::trim_*` themselves when `gapless` is on (the default).
        // We apply the trims ourselves (decode_next), so turn it off to avoid trimming twice.
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&p.params, &AudioDecoderOptions::default().gapless(false))
            .map_err(|e| DecodeError::Unsupported(e.to_string()))?;
        let codec = codec_of(p.params.codec).ok_or_else(|| DecodeError::Unsupported(format!("codec {:?}", p.params.codec)))?;
        let rate = p.params.sample_rate.unwrap_or(44_100);
        let (mp4_delay, mp4_valid) = p.mp4_gapless.map(|g| g.to_frames(rate)).unwrap_or((0, None));
        let mp4_gapless_present = p.mp4_gapless.is_some();
        let trimmer = p.mp4_gapless.map(|_| Trimmer::new(mp4_delay, mp4_valid));
        let raw_frames = p.num_frames.map(|n| frames_of(n, p.time_base, rate));
        let total_frames = mp4_valid.or(raw_frames.map(|n| n.saturating_sub(mp4_delay)));
        let gapless = match p.mp4_gapless {
            Some(Mp4Gapless::ITunSmpb { delay, padding, .. }) => Some(GaplessInfo { delay: delay as u32, padding: padding as u32 }),
            Some(Mp4Gapless::EditList { .. }) => Some(GaplessInfo {
                delay: mp4_delay as u32,
                padding: raw_frames.zip(mp4_valid).map_or(0, |(r, v)| r.saturating_sub(v + mp4_delay)) as u32,
            }),
            None => None,
        };
        let bitrate_kbps = total_frames
            .filter(|&f| f > 0)
            .map(|f| (p.file_len as f64 * 8.0 / (f as f64 / rate as f64) / 1000.0).round() as u32);
        let placeholder = PcmSpec { sample_rate: rate, channels: 2, kind: SampleKind::Float };
        let mut me = Self {
            format: p.format,
            decoder,
            track_id: p.track_id,
            tb: p.time_base,
            rate,
            bits_hint: p.params.bits_per_sample,
            info: SourceInfo {
                container: p.container, codec, bitrate_kbps, spec: placeholder, total_frames, gapless,
                decoder_name: "symphonia", dsd_rate: None,
            },
            trimmer,
            mp4_delay,
            saw_packet_trim: false,
            skip: 0,
            pending: None,
            decode_errors: 0,
            eof_padding: if mp4_gapless_present { 0 } else { p.padding.unwrap_or(0) as usize },
            saw_end_trim: false,
            held: None,
        };
        // Decode the first block now so `info().spec` reports the real output format.
        let first = me.decode_next()?.ok_or_else(|| DecodeError::Unsupported("no audio frames".into()))?;
        me.info.spec = first.spec;
        me.pending = Some(first);
        Ok(me)
    }

    /// Next block with the end-padding fallback applied (one block of lookahead).
    fn decode_next(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        if self.eof_padding == 0 { return self.decode_raw(); }
        let cur = match self.held.take() {
            Some(b) => b,
            None => match self.decode_raw()? { Some(b) => b, None => return Ok(None) },
        };
        match self.decode_raw()? {
            Some(next) => { self.held = Some(next); Ok(Some(cur)) }
            None => {
                let mut last = cur;
                if !self.saw_end_trim { last.trim(0, self.eof_padding); }
                Ok(if last.is_empty() { None } else { Some(last) })
            }
        }
    }

    fn decode_raw(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(SymErr::ResetRequired) => { self.decoder.reset(); continue; }
                Err(e) => return Err(map_sym_err(e)),
            };
            if packet.track_id != self.track_id { continue; }
            let trim_start = frames_of(packet.trim_start.get(), self.tb, self.rate) as usize;
            let trim_end = frames_of(packet.trim_end.get(), self.tb, self.rate) as usize;
            if trim_start > 0 || trim_end > 0 { self.saw_packet_trim = true; }
            if trim_end > 0 { self.saw_end_trim = true; }
            let mut block = match self.decoder.decode(&packet) {
                Ok(buf) => to_block(&buf, self.bits_hint),
                Err(SymErr::DecodeError(msg)) => {
                    self.decode_errors += 1;
                    tracing::warn!(error = msg, "skipping undecodable packet");
                    if self.decode_errors > 500 { return Err(DecodeError::Corrupt(msg.to_string())); }
                    continue;
                }
                Err(SymErr::IoError(_)) => continue,
                Err(e) => return Err(map_sym_err(e)),
            };
            block.trim(trim_start, trim_end);
            if !self.saw_packet_trim && let Some(t) = self.trimmer.as_mut() {
                t.apply(&mut block);
            }
            if self.skip > 0 {
                let n = (self.skip as usize).min(block.frames());
                block.trim(n, 0);
                self.skip -= n as u64;
            }
            if block.is_empty() { continue; }
            return Ok(Some(block));
        }
    }
}

impl SymphoniaDecoder {
    fn seek_ts(&mut self, ts: u64) -> Result<symphonia::core::formats::SeekedTo, DecodeError> {
        let seeked = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(ts as i64), track_id: self.track_id })
            .map_err(map_sym_err)?;
        self.decoder.reset();
        Ok(seeked)
    }
}

impl TrackDecoder for SymphoniaDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        if let Some(b) = self.pending.take() { return Ok(Some(b)); }
        self.decode_next()
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        self.pending = None;
        self.held = None;
        let use_trimmer = self.trimmer.is_some() && !self.saw_packet_trim;
        if use_trimmer {
            // MP4: work in raw (untrimmed) coordinates. Land a couple of packets early so the
            // decoder's overlap state is warm (a reset AAC decoder distorts its first frame), and
            // so a landing inside the priming region still skips exactly the right frames.
            let target_raw = frame + self.mp4_delay;
            let ts = units_of(target_raw.saturating_sub(SEEK_PREROLL), self.tb, self.rate);
            let seeked = self.seek_ts(ts)?;
            let landed_raw = frames_of(seeked.actual_ts.get().max(0) as u64, self.tb, self.rate);
            let skip = target_raw - landed_raw.min(target_raw);
            if let Some(t) = self.trimmer.as_mut() { t.seek_raw(skip, frame); }
            self.skip = 0;
        } else {
            let seeked = self.seek_ts(units_of(frame, self.tb, self.rate))?;
            let landed = frames_of(seeked.actual_ts.get().max(0) as u64, self.tb, self.rate).min(frame);
            self.skip = frame - landed;
        }
        Ok(frame)
    }
}
