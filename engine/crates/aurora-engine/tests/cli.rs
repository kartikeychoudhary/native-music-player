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
