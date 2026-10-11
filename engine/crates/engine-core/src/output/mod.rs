pub mod card_match;
pub mod cpal_out;
pub mod format;
pub mod null;
pub mod ring;
pub mod system;

#[cfg(windows)]
pub mod wasapi_excl;
#[cfg(target_os = "macos")]
pub mod coreaudio_hog;
#[cfg(target_os = "linux")]
pub mod alsa_hw;

use format::{DeviceSample, OutputFormat};
use protocol::{BluetoothInfo, DeviceInfo, ErrorCode, ShareMode, Transport};
pub use ring::RingSource;

pub const COMMON_RATES: [u32; 10] = [44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000, 705600, 768000];

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceCaps { pub rates: Vec<u32>, pub max_channels: u16, pub samples: Vec<DeviceSample>, pub exclusive: bool }

impl DeviceCaps {
    pub fn supports_rate(&self, r: u32) -> bool { self.rates.contains(&r) }
    pub fn supports(&self, s: DeviceSample) -> bool { self.samples.contains(&s) }
    /// Narrowest integer format that carries `bits` without loss.
    pub fn best_int_for(&self, bits: u16) -> Option<DeviceSample> {
        [DeviceSample::I16, DeviceSample::I24, DeviceSample::I32].into_iter().find(|&s| s.bits() >= bits && self.supports(s))
    }
    pub fn fallback() -> Self { Self { rates: vec![48_000], max_channels: 2, samples: vec![DeviceSample::F32], exclusive: false } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamInfo { pub device_id: String, pub device_name: String, pub transport: Transport, pub share: ShareMode, pub format: OutputFormat }

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("device unavailable: {0}")] DeviceUnavailable(String),
    #[error("exclusive access denied: {0}")] ExclusiveDenied(String),
    #[error("unsupported output format: {0}")] UnsupportedFormat(String),
    #[error("{0}")] Other(String),
}

impl OutputError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::ExclusiveDenied(_) => ErrorCode::ExclusiveDenied,
            Self::UnsupportedFormat(_) => ErrorCode::UnsupportedFormat,
            _ => ErrorCode::DeviceUnavailable,
        }
    }
}

pub trait OutputBackend: Send + Sync {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError>;
    fn caps(&self, device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError>;
    fn open(&self, device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError>;
    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> {
        (stream.transport == Transport::Bluetooth).then(BluetoothInfo::default)
    }
}

/// Created and dropped on the player thread; dropping it closes the device.
pub trait OutputStream {
    fn info(&self) -> &StreamInfo;
    fn start(&mut self) -> Result<(), OutputError>;
}
