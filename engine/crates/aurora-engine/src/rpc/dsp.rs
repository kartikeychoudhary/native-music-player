//! DSP settings and user EQ presets (spec §3.6, §4.3 `dsp.*`), persisted in `<data-dir>/dsp.json`.
use std::path::{Path, PathBuf};

use engine_core::dsp::eq::{builtin_presets, MAX_DB};
use engine_core::player::Command;
use protocol::{DspState, EqPreset, ReplayGainMode, SetEqParams};
use serde::{Deserialize, Serialize};

const FILE: &str = "dsp.json";
const VERSION: u32 = 1;
pub const MAX_CROSSFADE_SECS: f32 = 12.0;
const MAX_PRESET_NAME: usize = 40;

#[derive(Serialize, Deserialize)]
struct DspFile { version: u32, state: DspState, presets: Vec<EqPreset> }

pub struct DspStore { state: DspState, user: Vec<EqPreset>, path: Option<PathBuf> }

fn valid_db(v: f32) -> bool { v.is_finite() && v.abs() <= MAX_DB }

fn valid_gains(bands: &[f32], preamp: f32) -> bool { bands.len() == 10 && bands.iter().all(|b| valid_db(*b)) && valid_db(preamp) }

fn valid_state(s: &DspState) -> bool {
    valid_gains(&s.eq.bands, s.eq.preamp) && s.crossfade_secs.is_finite() && (0.0..=MAX_CROSSFADE_SECS).contains(&s.crossfade_secs)
}

impl DspStore {
    pub fn load(dir: Option<&Path>) -> Self {
        let path = dir.map(|d| d.join(FILE));
        let file = path.as_ref().and_then(|p| std::fs::read(p).ok()).and_then(|b| match serde_json::from_slice::<DspFile>(&b) {
            Ok(f) if f.version != VERSION => { tracing::warn!(version = f.version, "ignoring dsp.json from another version"); None }
            Ok(f) if !valid_state(&f.state) => { tracing::warn!("ignoring dsp.json with out-of-range settings"); None }
            Ok(f) => Some(f),
            Err(e) => { tracing::warn!(error = %e, "ignoring unreadable dsp.json"); None }
        });
        match file {
            Some(f) => DspStore {
                state: f.state,
                user: f.presets.into_iter().filter(|p| !p.builtin && valid_gains(&p.bands, p.preamp)).collect(),
                path,
            },
            None => DspStore { state: DspState::default(), user: vec![], path },
        }
    }

    pub fn state(&self) -> &DspState { &self.state }

    pub fn presets(&self) -> Vec<EqPreset> {
        let mut user = self.user.clone();
        user.sort_by_key(|p| p.name.to_lowercase());
        builtin_presets().into_iter().chain(user).collect()
    }

    pub fn commands(&self) -> Vec<Command> {
        vec![Command::SetEq(self.state.eq.clone()), Command::SetCrossfade(self.state.crossfade_secs), Command::SetReplayGain(self.state.replay_gain)]
    }

    fn find(&self, name: &str) -> Option<EqPreset> { self.presets().into_iter().find(|p| p.name == name) }

    pub fn set_eq(&mut self, p: SetEqParams) -> Result<(), String> {
        if p.bands.len() != 10 { return Err("bands must have exactly 10 values".into()); }
        if !valid_gains(&p.bands, p.preamp) { return Err(format!("gains must be within ±{MAX_DB} dB")); }
        if let Some(name) = &p.preset && self.find(name).is_none() { return Err(format!("no preset named {name:?}")); }
        self.state.eq.bands = p.bands;
        self.state.eq.preamp = p.preamp;
        self.state.eq.preset = p.preset;
        self.save();
        Ok(())
    }

    pub fn set_eq_enabled(&mut self, on: bool) -> Result<(), String> { self.state.eq.enabled = on; self.save(); Ok(()) }

    pub fn save_preset(&mut self, name: &str) -> Result<EqPreset, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_PRESET_NAME { return Err(format!("preset names are 1–{MAX_PRESET_NAME} characters")); }
        if builtin_presets().iter().any(|p| p.name.eq_ignore_ascii_case(name)) { return Err(format!("{name:?} is a built-in preset")); }
        let preset = EqPreset { name: name.into(), bands: self.state.eq.bands.clone(), preamp: self.state.eq.preamp, builtin: false };
        self.user.retain(|p| p.name != name);
        self.user.push(preset.clone());
        self.state.eq.preset = Some(name.into());
        self.save();
        Ok(preset)
    }

    pub fn delete_preset(&mut self, name: &str) -> Result<(), String> {
        if builtin_presets().iter().any(|p| p.name == name) { return Err(format!("{name:?} is a built-in preset")); }
        let before = self.user.len();
        self.user.retain(|p| p.name != name);
        if self.user.len() == before { return Err(format!("no preset named {name:?}")); }
        if self.state.eq.preset.as_deref() == Some(name) { self.state.eq.preset = None; }
        self.save();
        Ok(())
    }

    pub fn set_crossfade(&mut self, secs: f32) -> Result<(), String> {
        if !secs.is_finite() || !(0.0..=MAX_CROSSFADE_SECS).contains(&secs) { return Err(format!("crossfade must be 0–{MAX_CROSSFADE_SECS} s")); }
        self.state.crossfade_secs = secs;
        self.save();
        Ok(())
    }

    pub fn set_replay_gain(&mut self, mode: ReplayGainMode) -> Result<(), String> { self.state.replay_gain = mode; self.save(); Ok(()) }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let file = DspFile { version: VERSION, state: self.state.clone(), presets: self.user.clone() };
        let tmp = path.with_extension("json.tmp");
        let res = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other).and_then(|b| std::fs::write(&tmp, b)).and_then(|_| std::fs::rename(&tmp, path));
        if let Err(e) = res { tracing::warn!(error = %e, "couldn't save dsp.json"); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_or_foreign_files_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), b"{ nope").unwrap();
        assert_eq!(DspStore::load(Some(dir.path())).state(), &DspState::default());
        std::fs::write(dir.path().join(FILE), br#"{"version":99,"state":{},"presets":[]}"#).unwrap();
        assert_eq!(DspStore::load(Some(dir.path())).state(), &DspState::default());
        assert_eq!(DspStore::load(None).presets().len(), 6);
    }

    #[test]
    fn saving_over_a_user_preset_replaces_it() {
        let mut s = DspStore::load(None);
        s.set_eq(SetEqParams { bands: vec![1.0; 10], preamp: 0.0, preset: None }).unwrap();
        s.save_preset("Mine").unwrap();
        s.set_eq(SetEqParams { bands: vec![2.0; 10], preamp: 0.0, preset: None }).unwrap();
        s.save_preset("Mine").unwrap();
        let mine: Vec<_> = s.presets().into_iter().filter(|p| p.name == "Mine").collect();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].bands, vec![2.0; 10]);
    }

    #[test]
    fn out_of_range_saved_settings_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = DspStore::load(Some(dir.path()));
        s.set_crossfade(3.0).unwrap();
        let file = dir.path().join(FILE);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"crossfadeSecs\": 3.0"));
        std::fs::write(&file, text.replace("\"crossfadeSecs\": 3.0", "\"crossfadeSecs\": 99.0")).unwrap();
        assert_eq!(DspStore::load(Some(dir.path())).state(), &DspState::default());
    }
}
