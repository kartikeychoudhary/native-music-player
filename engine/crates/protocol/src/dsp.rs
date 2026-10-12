//! DSP settings (spec §3.6, §4.3 `dsp.*`). The engine owns and persists them.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReplayGainMode { #[default] Off, Track, Album }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EqSettings {
    pub enabled: bool,
    /// Gain in dB for each of the 10 bands (31 Hz … 16 kHz), each within ±12.
    pub bands: Vec<f32>,
    /// Preamp in dB (±12). Automatic headroom is applied on top of it.
    pub preamp: f32,
    /// The preset these values came from, or `None` after a manual change.
    pub preset: Option<String>,
}

impl Default for EqSettings {
    fn default() -> Self { EqSettings { enabled: false, bands: vec![0.0; 10], preamp: 0.0, preset: Some("Flat".into()) } }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DspState { pub eq: EqSettings, pub crossfade_secs: f32, pub replay_gain: ReplayGainMode }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EqPreset { pub name: String, pub bands: Vec<f32>, pub preamp: f32, pub builtin: bool }

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetEqParams {
    pub bands: Vec<f32>,
    pub preamp: f32,
    #[serde(default)] #[ts(optional)] pub preset: Option<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EnabledParams { pub enabled: bool }

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrossfadeParams { pub seconds: f32 }

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReplayGainParams { pub mode: ReplayGainMode }
