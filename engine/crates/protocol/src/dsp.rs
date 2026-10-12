//! DSP settings (spec §3.6, §4.3 `dsp.*`). The engine owns and persists them.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReplayGainMode { #[default] Off, Track, Album }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
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
#[serde(rename_all = "camelCase", default)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fill_in_defaults() {
        let s: DspState = serde_json::from_str("{}").unwrap();
        assert_eq!(s, DspState::default());
        let s: DspState = serde_json::from_str(r#"{"crossfadeSecs":4.0,"eq":{"enabled":true,"preamp":-2.0}}"#).unwrap();
        assert_eq!((s.crossfade_secs, s.eq.enabled, s.eq.preamp), (4.0, true, -2.0));
        assert_eq!(s.eq.bands, vec![0.0; 10], "missing bands fall back to flat");
        assert_eq!(s.replay_gain, ReplayGainMode::Off);
        let s: DspState = serde_json::from_str(r#"{"somethingNew":1,"eq":{"enabled":true,"future":[]}}"#).unwrap();
        assert!(s.eq.enabled, "unknown fields are ignored");
    }
}
