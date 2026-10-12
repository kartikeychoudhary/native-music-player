use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

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

/// Runs the engine and fails (killing it) if it hasn't exited within 15 s.
fn run_with_timeout(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() { return child.wait_with_output().unwrap(); }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("aurora-engine {args:?} did not exit within 15 s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn play_with_no_files_exits_2() {
    let out = run_with_timeout(&["play", "--null"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "errors must never be written to stdout");
}

#[test]
fn play_bogus_device_exits_nonzero_without_hanging() {
    let out = run_with_timeout(&["play", "--device", "bogus", &fixture("short_a.flac")]);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"event\":\"error\""));
}

#[test]
fn play_missing_file_exits_1() {
    let out = run_with_timeout(&["play", "--null", "--speed", "20", &fixture("does_not_exist.flac")]);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn panics_exit_cleanly_with_code_70() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).arg("__panic").output().unwrap();
    assert_eq!(out.status.code(), Some(70));
    assert!(String::from_utf8_lossy(&out.stderr).contains("test panic"));
}

#[test]
fn rpc_accepts_an_app_user_model_id() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
        .args(["rpc", "--app-user-model-id", "io.github.kartikeychoudhary.aurora.test"])
        .env("AURORA_OUTPUT", "null").env("AURORA_OSMEDIA", "0")
        .stdin(Stdio::null()).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    assert!(first.contains("\"engine.ready\""), "stdout: {first}; stderr: {}", String::from_utf8_lossy(&out.stderr));
}
