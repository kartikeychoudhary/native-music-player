use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Codec { Mp3, AacLc, HeAac, HeAacV2, Alac, Flac, Pcm, Vorbis, Opus, Dsd }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Transport { Usb, Bluetooth, Builtin, Hdmi, Other }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OutputMode { #[default] Processed, BitPerfect }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ShareMode { Shared, Exclusive }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BitPerfectScope { Device, Os }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum PlaybackStatus { #[default] Stopped, Playing, Paused }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum StageKind { Channels, Dsd, Resampler, Volume, Dither }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Stage { pub kind: StageKind, pub active: bool, pub detail: String }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceSummary {
    pub container: String,
    pub codec: Codec,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: u32,
    pub bit_depth: Option<u16>,
    pub channels: u16,
    pub dsd_rate: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GaplessSummary { pub delay: u32, pub padding: u32 }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecoderSummary { pub name: String, pub gapless: Option<GaplessSummary> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OutputSummary {
    pub device_id: String,
    pub name: String,
    pub transport: Transport,
    pub mode: ShareMode,
    pub sample_rate: u32,
    /// "s16" | "s24" | "s32" | "f32"
    pub format: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BluetoothInfo {
    /// e.g. "ldac", "aac", "sbc"; None = not reported by the OS.
    pub codec: Option<String>,
    pub quality: Option<String>,
    pub bitrate_kbps: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SignalPath {
    pub source: SourceSummary,
    pub decoder: DecoderSummary,
    pub stages: Vec<Stage>,
    pub output: OutputSummary,
    /// Present only when the output transport is Bluetooth.
    pub bluetooth: Option<BluetoothInfo>,
    pub bit_perfect: bool,
    pub bit_perfect_scope: Option<BitPerfectScope>,
    pub underruns: u64,
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub transport: Transport,
    pub is_default: bool,
    pub sample_rates: Vec<u32>,
    pub exclusive_capable: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum RepeatMode { #[default] Off, All, One }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackSummary {
    pub path: String,
    pub title: String,
    pub duration_ms: Option<u64>,
    pub track_id: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_id: Option<i64>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub queue_index: Option<u32>,
    pub queue_item_id: Option<u32>,
    pub track: Option<TrackSummary>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub mode: OutputMode,
    pub device_id: Option<String>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueItem {
    pub id: u32,
    pub path: String,
    pub title: String,
    pub track_id: Option<i64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_id: Option<i64>,
    pub duration_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueChanged { pub items: Vec<QueueItem>, pub current: Option<u32> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineReady { pub version: String, pub protocol_version: u32, pub features: Vec<String> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineErrorEvent { pub code: i32, pub name: String, pub message: String, pub path: Option<String> }
