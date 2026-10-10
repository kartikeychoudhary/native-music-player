pub mod aac_config;
pub mod dsd;
#[cfg(feature = "fdk-aac")]
pub mod fdk_dec;
pub mod mp4_gapless;
pub mod opus_dec;
pub mod symphonia_dec;
pub mod trim;

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
