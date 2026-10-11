use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn fixture(name: &str) -> String { format!("{}/../../../fixtures/out/{name}", env!("CARGO_MANIFEST_DIR")) }

/// `seen` is a window that tests may clear; `all` keeps every message for final assertions.
struct Engine { child: Child, stdin: ChildStdin, rx: Receiver<String>, next_id: u64, seen: Vec<Value>, all: Vec<Value> }

impl Engine {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
            .env("AURORA_OUTPUT", "null").env("AURORA_NULL_SPEED", "10")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || for line in BufReader::new(stdout).lines().map_while(Result::ok) { let _ = tx.send(line); });
        Engine { child, stdin, rx, next_id: 1, seen: vec![], all: vec![] }
    }

    fn next(&mut self, timeout: Duration) -> Value {
        let line = self.rx.recv_timeout(timeout).expect("engine went quiet");
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
        self.seen.push(v.clone());
        self.all.push(v.clone());
        v
    }

    fn wait_for(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        if let Some(v) = self.seen.iter().find(|v| pred(v)) { return v.clone(); }
        loop {
            assert!(start.elapsed() < Duration::from_secs(15), "timed out; seen: {:#?}", self.seen);
            let v = self.next(Duration::from_secs(15));
            if pred(&v) { return v; }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
        self.stdin.flush().unwrap();
        self.wait_for(|v| v["id"] == id)
    }

    fn notified(&mut self, method: &'static str, pred: impl Fn(&Value) -> bool) -> Value {
        self.wait_for(move |v| v["method"] == method && pred(&v["params"]))["params"].clone()
    }
}

impl Drop for Engine { fn drop(&mut self) { let _ = self.child.kill(); } }

#[test]
fn handshake_comes_first() {
    let mut e = Engine::spawn();
    let first = e.next(Duration::from_secs(10));
    assert_eq!(first["method"], "engine.ready");
    assert_eq!(first["params"]["protocolVersion"], 1);
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
        .env("AURORA_OUTPUT", "null")
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
