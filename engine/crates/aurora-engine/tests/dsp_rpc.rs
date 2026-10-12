mod common;
use common::{fixture, Engine};
use serde_json::{json, Value};

fn with_data(dir: &std::path::Path) -> Engine { Engine::spawn_with(&["rpc", "--data-dir", &dir.to_string_lossy()], &[]) }

const BASS: [f32; 10] = [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
const ZERO9: [f32; 9] = [0.0; 9];
const ZERO10: [f32; 10] = [0.0; 10];

#[test]
fn dsp_defaults_and_validation() {
    let mut e = Engine::spawn();
    let s = e.ok("dsp.get", json!({}));
    assert_eq!(s, json!({ "eq": { "enabled": false, "bands": ZERO10, "preamp": 0.0, "preset": "Flat" }, "crossfadeSecs": 0.0, "replayGain": "off" }));
    let names: Vec<String> = e.ok("dsp.presets", json!({})).as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().into()).collect();
    assert_eq!(names, ["Flat", "Warm", "Bass", "Vocal", "Bright", "Loudness"]);
    for (m, p) in [
        ("dsp.setEq", json!({ "bands": ZERO9, "preamp": 0 })),
        ("dsp.setEq", json!({ "bands": [13.0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "preamp": 0 })),
        ("dsp.setEq", json!({ "bands": ZERO10, "preamp": -13 })),
        ("dsp.setEq", json!({ "bands": ZERO10, "preamp": 0, "preset": "Nope" })),
        ("dsp.setCrossfade", json!({ "seconds": 12.5 })),
        ("dsp.savePreset", json!({ "name": "  " })),
        ("dsp.savePreset", json!({ "name": "bass" })),
        ("dsp.deletePreset", json!({ "name": "Flat" })),
        ("dsp.deletePreset", json!({ "name": "Missing" })),
    ] {
        assert_eq!(e.call(m, p.clone())["error"]["code"], -32009, "{m} {p}");
    }
}

#[test]
fn dsp_changes_broadcast_persist_and_restore() {
    let data = tempfile::tempdir().unwrap();
    let mut e = with_data(data.path());
    let s = e.ok("dsp.setEq", json!({ "bands": BASS, "preamp": -1.0, "preset": "Bass" }));
    assert_eq!(s["eq"]["preset"], "Bass");
    assert_eq!(e.notified("dsp.changed", |p| p["eq"]["preset"] == "Bass")["eq"]["preamp"], -1.0);
    e.ok("dsp.setEqEnabled", json!({ "enabled": true }));
    e.ok("dsp.setCrossfade", json!({ "seconds": 4 }));
    e.ok("dsp.setReplayGain", json!({ "mode": "album" }));
    let saved = e.ok("dsp.savePreset", json!({ "name": "My Room" }));
    assert_eq!((saved["builtin"].clone(), saved["preamp"].clone()), (json!(false), json!(-1.0)));
    assert_eq!(e.ok("dsp.get", json!({}))["eq"]["preset"], "My Room");
    let before = e.ok("dsp.get", json!({}));
    e.close();

    let mut e = with_data(data.path());
    assert_eq!(e.ok("dsp.get", json!({})), before, "restored from dsp.json");
    let presets = e.ok("dsp.presets", json!({}));
    assert!(presets.as_array().unwrap().iter().any(|p| p["name"] == "My Room" && p["builtin"] == false));
    let s = e.ok("dsp.deletePreset", json!({ "name": "My Room" }));
    assert_eq!(s["eq"]["preset"], Value::Null, "deleting the current preset leaves the values as Custom");
    let s = e.ok("dsp.setEq", json!({ "bands": ZERO10, "preamp": 0 }));
    assert_eq!(s["eq"]["preset"], Value::Null, "a manual change has no preset");
}

#[test]
fn dsp_settings_reach_the_player() {
    let mut e = Engine::spawn();
    e.ok("dsp.setEq", json!({ "bands": BASS, "preamp": 0, "preset": "Bass" }));
    e.ok("dsp.setEqEnabled", json!({ "enabled": true }));
    e.ok("queue.setPaths", json!({ "paths": [fixture("flac16_44k.flac")] }));
    e.ok("player.play", json!({}));
    let eq = |sp: &Value| sp["stages"].as_array().unwrap().iter().find(|s| s["kind"] == "eq").cloned().unwrap();
    let sp = e.notified("player.signalPath", |p| p["stages"].as_array().is_some_and(|s| s.iter().any(|s| s["kind"] == "eq")));
    assert_eq!((eq(&sp)["active"].clone(), eq(&sp)["detail"].clone()), (json!(true), json!("Bass")));
    e.ok("player.setMode", json!({ "mode": "bitPerfect" }));
    let sp = e.notified("player.signalPath", |p| p["bitPerfect"] == true || p["stages"].as_array().is_some_and(|s| s.iter().any(|s| s["kind"] == "eq" && s["detail"] == "bypassed (bit-perfect)")));
    assert_eq!(eq(&sp)["detail"], "bypassed (bit-perfect)");
}
