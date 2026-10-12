//! DSP settings and user EQ presets (spec §3.6, §4.3 `dsp.*`), persisted in `<data-dir>/dsp.json`.
use std::path::{Path, PathBuf};

use engine_core::dsp::eq::{builtin_presets, MAX_DB};
use engine_core::player::Command;
use protocol::{DspState, EqPreset, EqSettings, ReplayGainMode, SetEqParams};
use serde::{Deserialize, Serialize};

const FILE: &str = "dsp.json";
const VERSION: u32 = 1;
pub const MAX_CROSSFADE_SECS: f32 = 12.0;
const MAX_PRESET_NAME: usize = 40;

#[derive(Serialize)]
struct DspFile { version: u32, state: DspState, presets: Vec<EqPreset> }

/// What is read back: parts are parsed one at a time so a bad piece costs only itself.
#[derive(Deserialize)]
struct RawFile {
    version: u32,
    #[serde(default)]
    state: serde_json::Value,
    #[serde(default)]
    presets: Vec<serde_json::Value>,
}

pub struct DspStore { state: DspState, user: Vec<EqPreset>, path: Option<PathBuf> }

fn valid_db(v: f32) -> bool { v.is_finite() && v.abs() <= MAX_DB }

fn valid_gains(bands: &[f32], preamp: f32) -> bool { bands.len() == 10 && bands.iter().all(|b| valid_db(*b)) && valid_db(preamp) }

/// Keeps what is valid in a loaded file and resets only what is not: an invalid EQ becomes the
/// default EQ, an invalid crossfade becomes 0, a dangling preset name is cleared. User presets with
/// an empty, duplicate or built-in name (or bad gains) are dropped.
fn sanitize(mut state: DspState, presets: Vec<EqPreset>) -> (DspState, Vec<EqPreset>) {
    let builtin = builtin_presets();
    let mut user: Vec<EqPreset> = Vec::new();
    for p in presets {
        let name = p.name.trim();
        let ok = !p.builtin && !name.is_empty() && name == p.name && valid_gains(&p.bands, p.preamp)
            && !builtin.iter().any(|b| b.name.eq_ignore_ascii_case(name))
            && !user.iter().any(|u| u.name == p.name);
        if ok { user.push(p); } else { tracing::warn!(name = %p.name, "dropping an invalid user preset from dsp.json"); }
    }
    if !valid_gains(&state.eq.bands, state.eq.preamp) {
        tracing::warn!("resetting out-of-range EQ settings from dsp.json");
        state.eq = EqSettings::default();
    }
    if !state.crossfade_secs.is_finite() || !(0.0..=MAX_CROSSFADE_SECS).contains(&state.crossfade_secs) {
        tracing::warn!("resetting out-of-range crossfade from dsp.json");
        state.crossfade_secs = 0.0;
    }
    if let Some(name) = &state.eq.preset && !builtin.iter().chain(&user).any(|p| p.name == *name) { state.eq.preset = None; }
    (state, user)
}

impl DspStore {
    pub fn load(dir: Option<&Path>) -> Self {
        let path = dir.map(|d| d.join(FILE));
        let raw = path.as_ref().and_then(|p| std::fs::read(p).ok()).and_then(|b| match serde_json::from_slice::<RawFile>(&b) {
            Ok(f) if f.version != VERSION => { tracing::warn!(version = f.version, "ignoring dsp.json from another version"); None }
            Ok(f) => Some(f),
            Err(e) => { tracing::warn!(error = %e, "ignoring unreadable dsp.json"); None }
        });
        let Some(raw) = raw else { return DspStore { state: DspState::default(), user: vec![], path } };
        let state = if raw.state.is_null() { DspState::default() } else {
            serde_json::from_value(raw.state).unwrap_or_else(|e| { tracing::warn!(error = %e, "resetting unreadable DSP state in dsp.json"); DspState::default() })
        };
        let presets = raw.presets.into_iter().filter_map(|v| serde_json::from_value::<EqPreset>(v).map_err(|e| tracing::warn!(error = %e, "dropping an unreadable preset from dsp.json")).ok()).collect();
        let (state, user) = sanitize(state, presets);
        DspStore { state, user, path }
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

    fn write_file(dir: &Path, json: serde_json::Value) { std::fs::write(dir.join(FILE), serde_json::to_vec(&json).unwrap()).unwrap(); }

    fn bands(v: f32) -> Vec<f32> { vec![v; 10] }

    #[test]
    fn a_file_with_missing_fields_loads_with_defaults_and_keeps_presets() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), serde_json::json!({
            "version": 1,
            "state": { "crossfadeSecs": 5.0 },
            "presets": [{ "name": "Mine", "bands": bands(2.0), "preamp": 0.0, "builtin": false }],
        }));
        let s = DspStore::load(Some(dir.path()));
        assert_eq!(s.state().crossfade_secs, 5.0);
        assert_eq!(s.state().eq, protocol::EqSettings::default());
        assert!(s.presets().iter().any(|p| p.name == "Mine"));
        write_file(dir.path(), serde_json::json!({ "version": 1 }));
        assert_eq!(DspStore::load(Some(dir.path())).state(), &DspState::default());
    }

    #[test]
    fn an_invalid_crossfade_resets_only_the_crossfade() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), serde_json::json!({
            "version": 1,
            "state": { "eq": { "enabled": true, "bands": bands(3.0), "preamp": -1.0, "preset": "Mine" }, "crossfadeSecs": 99.0, "replayGain": "album" },
            "presets": [{ "name": "Mine", "bands": bands(3.0), "preamp": -1.0, "builtin": false }],
        }));
        let s = DspStore::load(Some(dir.path()));
        assert_eq!(s.state().crossfade_secs, 0.0);
        assert!(s.state().eq.enabled);
        assert_eq!(s.state().eq.bands, bands(3.0));
        assert_eq!(s.state().eq.preset.as_deref(), Some("Mine"));
        assert_eq!(s.state().replay_gain, ReplayGainMode::Album);
        assert!(s.presets().iter().any(|p| p.name == "Mine"), "the user's presets survive");
    }

    #[test]
    fn an_invalid_eq_resets_only_the_eq() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), serde_json::json!({
            "version": 1,
            "state": { "eq": { "enabled": true, "bands": bands(40.0), "preamp": 0.0 }, "crossfadeSecs": 4.0 },
            "presets": [{ "name": "Mine", "bands": bands(1.0), "preamp": 0.0, "builtin": false }],
        }));
        let s = DspStore::load(Some(dir.path()));
        assert_eq!(s.state().eq, protocol::EqSettings::default());
        assert_eq!(s.state().crossfade_secs, 4.0);
        assert!(s.presets().iter().any(|p| p.name == "Mine"));
    }

    #[test]
    fn a_dangling_preset_name_is_cleared() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), serde_json::json!({
            "version": 1,
            "state": { "eq": { "enabled": false, "bands": bands(1.0), "preamp": 0.0, "preset": "Gone" } },
            "presets": [],
        }));
        let s = DspStore::load(Some(dir.path()));
        assert_eq!(s.state().eq.preset, None);
        assert_eq!(s.state().eq.bands, bands(1.0), "the values stay");
    }

    #[test]
    fn user_presets_with_bad_names_are_dropped_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let p = |name: &str, v: f32| serde_json::json!({ "name": name, "bands": bands(v), "preamp": 0.0, "builtin": false });
        write_file(dir.path(), serde_json::json!({
            "version": 1,
            "state": {},
            "presets": [p("Warm", 1.0), p("flat", 1.0), p("", 1.0), p("   ", 1.0), p("Mine", 1.0), p("Mine", 2.0), p("Other", 3.0)],
        }));
        let s = DspStore::load(Some(dir.path()));
        let user: Vec<_> = s.presets().into_iter().filter(|p| !p.builtin).collect();
        assert_eq!(user.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Mine", "Other"], "built-in names, empty names and duplicates are dropped");
        assert_eq!(user[0].bands, bands(1.0), "the first duplicate wins");
        assert_eq!(s.presets().iter().filter(|p| p.name == "Warm").count(), 1);
    }

    #[test]
    fn an_unparseable_preset_does_not_take_the_others_down() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), serde_json::json!({
            "version": 1,
            "state": {},
            "presets": [{ "name": "Broken", "bands": "nope" }, { "name": "Good", "bands": bands(1.0), "preamp": 0.0, "builtin": false }],
        }));
        let s = DspStore::load(Some(dir.path()));
        assert!(s.presets().iter().any(|p| p.name == "Good"));
        assert!(!s.presets().iter().any(|p| p.name == "Broken"));
    }
}
