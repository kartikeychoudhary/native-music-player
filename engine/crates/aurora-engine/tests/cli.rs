use std::process::Command;

fn fixture(name: &str) -> String {
    format!("{}/../../../fixtures/out/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn probe_prints_source_json() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).args(["probe", &fixture("flac24_96k.flac")]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["source"]["codec"], "flac");
    assert_eq!(v["source"]["sampleRate"], 96000);
    assert_eq!(v["source"]["bitDepth"], 24);
    assert_eq!(v["decoder"], "symphonia");
}

#[test]
fn probe_unsupported_exits_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).args(["probe", &fixture("empty.mp3")]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "errors must never be written to stdout");
}

#[test]
fn devices_prints_a_json_array() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).args(["devices", "--null"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["id"], "null");
}

#[test]
fn play_null_runs_to_end_and_prints_signal_path() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
        .args(["play", "--null", "--speed", "20", "--bit-perfect", &fixture("short_a.flac"), &fixture("short_b.flac")])
        .output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    let paths: Vec<serde_json::Value> = text.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).filter(|v| v["event"] == "signalPath").collect();
    assert!(!paths.is_empty());
    assert_eq!(paths[0]["data"]["bitPerfect"], true);
}
