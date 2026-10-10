pub mod aac_config;
pub mod dsd;
#[cfg(feature = "fdk-aac")]
pub mod fdk_dec;
pub mod mp4_gapless;
pub mod opus_dec;
pub mod symphonia_dec;
pub mod trim;

use std::path::Path;

use symphonia::core::codecs::audio::well_known::{CODEC_ID_AAC, CODEC_ID_OPUS};

use crate::pcm::{PcmBlock, PcmSpec};
use protocol::{Codec, ErrorCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaplessInfo { pub delay: u32, pub padding: u32 }

#[derive(Clone, Debug, PartialEq)]
pub struct SourceInfo {
    pub container: String,
    pub codec: Codec,
    pub bitrate_kbps: Option<u32>,
    pub spec: PcmSpec,
    pub total_frames: Option<u64>,
    pub gapless: Option<GaplessInfo>,
    pub decoder_name: &'static str,
    pub dsd_rate: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsdOutput { Pcm, Dop }

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("unsupported format: {0}")]
    Unsupported(String),
    #[error("{0} decoder not included in this build")]
    CodecNotInBuild(&'static str),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt stream: {0}")]
    Corrupt(String),
}

impl DecodeError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Unsupported(_) => ErrorCode::UnsupportedFormat,
            Self::CodecNotInBuild(_) => ErrorCode::CodecNotInBuild,
            Self::Io(e) if e.kind() == std::io::ErrorKind::NotFound => ErrorCode::FileMissing,
            Self::Io(_) | Self::Corrupt(_) => ErrorCode::DecodeFailed,
        }
    }
}

pub trait TrackDecoder: Send {
    fn info(&self) -> &SourceInfo;
    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError>;
    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError>;
    fn set_dsd_output(&mut self, _out: DsdOutput) -> bool { false }
}

/// True when an MP4 AAC stream is probably HE-AAC. Explicit SBR/PS signalling is conclusive; a
/// plain AAC-LC config with a core rate of 24 kHz or less is implicit SBR (the real output rate is
/// double), which the ASC alone cannot tell apart from low-rate LC, so such streams go to fdk-aac.
fn likely_he_aac(cfg: &aac_config::AacConfig) -> bool {
    cfg.sbr || cfg.ps || (cfg.object_type == 2 && cfg.sample_rate <= 24_000)
}

/// Pick the decoder for `path` by extension and probed codec.
pub fn open(path: &Path) -> Result<Box<dyn TrackDecoder>, DecodeError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "dsf" || ext == "dff" {
        return Ok(Box::new(dsd::DsdDecoder::open(path)?));
    }
    #[cfg(feature = "fdk-aac")]
    if ext == "aac" {
        return Ok(Box::new(fdk_dec::FdkAacDecoder::open_adts(path)?));
    }
    let probed = symphonia_dec::probe_file(path)?;
    let codec = probed.params.codec;
    if codec == CODEC_ID_OPUS {
        return Ok(Box::new(opus_dec::OpusDecoder::new(probed)?));
    }
    if codec == CODEC_ID_AAC {
        let cfg = probed.params.extra_data.as_deref().and_then(aac_config::parse_asc);
        if let Some(cfg) = cfg.filter(likely_he_aac) {
            #[cfg(feature = "fdk-aac")]
            return Ok(Box::new(fdk_dec::FdkAacDecoder::new_mp4(probed, cfg)?));
            #[cfg(not(feature = "fdk-aac"))]
            {
                let _ = cfg;
                return Err(DecodeError::CodecNotInBuild("HE-AAC"));
            }
        }
    }
    Ok(Box::new(symphonia_dec::SymphoniaDecoder::new(probed)?))
}

pub fn source_summary(info: &SourceInfo) -> protocol::SourceSummary {
    protocol::SourceSummary {
        container: info.container.clone(),
        codec: info.codec,
        bitrate_kbps: info.bitrate_kbps,
        sample_rate: info.spec.sample_rate,
        bit_depth: match info.spec.kind {
            crate::pcm::SampleKind::Int { bits } => Some(bits),
            _ => None,
        },
        channels: info.spec.channels,
        dsd_rate: info.dsd_rate,
    }
}

#[cfg(test)]
mod tests {
    use super::{aac_config::AacConfig, likely_he_aac};

    fn cfg(object_type: u8, sample_rate: u32, sbr: bool, ps: bool) -> AacConfig {
        AacConfig { object_type, sample_rate, channels: 2, sbr, ps, output_rate: sample_rate }
    }

    #[test]
    fn likely_he_aac_predicate() {
        assert!(likely_he_aac(&cfg(2, 44_100, true, false)), "explicit SBR");
        assert!(likely_he_aac(&cfg(2, 44_100, true, true)), "explicit PS");
        assert!(likely_he_aac(&cfg(2, 24_000, false, false)), "implicit SBR: LC at 24 kHz core");
        assert!(likely_he_aac(&cfg(2, 16_000, false, false)));
        assert!(!likely_he_aac(&cfg(2, 44_100, false, false)), "plain LC at 44.1 kHz");
        assert!(!likely_he_aac(&cfg(2, 48_000, false, false)));
        assert!(!likely_he_aac(&cfg(1, 22_050, false, false)), "only LC (object type 2) is treated as implicit SBR");
    }
}
