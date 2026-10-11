mod common;
use common::{fixture, Engine};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

#[test]
fn handshake_comes_first() {
    let mut e = Engine::spawn();
    let first = e.next(Duration::from_secs(10));
    assert_eq!(first["method"], "engine.ready");
    assert_eq!(first["params"]["protocolVersion"], 2);
    assert!(first["params"]["features"].as_array().unwrap().iter().any(|f| f == "dsd"));
}

#[test]
fn plays_a_queue_and_reports_signal_path() {
    let mut e = Engine::spawn();
    assert_eq!(e.call("queue.setPaths", json!({ "paths": [fixture("short_a.flac")], "startIndex": 0 }))["result"], Value::Null);
    e.call("player.play", json!({}));
    let sp = e.notified("player.signalPath", |_| true);
    assert_eq!(sp["source"]["codec"], "flac");
    e.notified("player.state", |p| p["status"] == "playing");
    e.seen.clear(); // the "stopped" state from setPaths must not satisfy the next wait
    e.notified("player.state", |p| p["status"] == "stopped");
}

#[test]
fn plays_unicode_path() {
    let dir = std::env::temp_dir().join(format!("aurora-ü-{}", std::process::id())).join("Beyoncé");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("日本語 ♫.flac");
    std::fs::copy(fixture("short_a.flac"), &path).unwrap();
    let mut e = Engine::spawn();
    e.call("queue.setPaths", json!({ "paths": [path.to_string_lossy()], "startIndex": 0 }));
    e.call("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert!(st["track"]["path"].as_str().unwrap().contains("日本語 ♫.flac"));
    e.seen.clear();
    e.notified("player.state", |p| p["status"] == "stopped");
    assert!(!e.all.iter().any(|v| v["method"] == "engine.error"), "{:#?}", e.all);
}

#[test]
fn rpc_errors() {
    let mut e = Engine::spawn();
    assert_eq!(e.call("nope.nothing", json!({}))["error"]["code"], -32601);
    assert_eq!(e.call("player.seek", json!({ "ms": "soon" }))["error"]["code"], -32602);
    assert_eq!(e.call("queue.setPaths", json!({ "paths": [], "startIndex": 3 }))["error"]["code"], -32009);
    writeln!(e.stdin, "{{not json").unwrap();
    e.stdin.flush().unwrap();
    let v = e.wait_for(|v| v["error"]["code"] == -32700);
    assert_eq!(v["id"], Value::Null);
}

#[test]
fn devices_and_state() {
    let mut e = Engine::spawn();
    assert_eq!(e.call("devices.list", json!({}))["result"][0]["id"], "null");
    let st = e.call("player.state", json!({}))["result"].clone();
    assert_eq!(st["status"], "stopped");
    assert_eq!(st["mode"], "processed");
    e.call("player.setMode", json!({ "mode": "bitPerfect" }));
    assert_eq!(e.call("player.state", json!({}))["result"]["mode"], "bitPerfect");
}

#[test]
fn exits_when_stdin_closes() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
        .env("AURORA_OUTPUT", "null").env("AURORA_OSMEDIA", "0")
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().unwrap();
    drop(child.stdin.take());
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() { break; }
        assert!(start.elapsed() < Duration::from_secs(3), "engine didn't exit on stdin EOF");
        std::thread::sleep(Duration::from_millis(20));
    }
}
