#![allow(dead_code)]
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub fn fixture(name: &str) -> String { format!("{}/../../../fixtures/out/{name}", env!("CARGO_MANIFEST_DIR")) }

/// `seen` is a window that tests may clear; `all` keeps every message for final assertions.
pub struct Engine { pub child: Child, pub stdin: ChildStdin, rx: Receiver<String>, next_id: u64, pub seen: Vec<Value>, pub all: Vec<Value> }

impl Engine {
    pub fn spawn() -> Self { Self::spawn_with(&[], &[]) }

    pub fn spawn_with(args: &[&str], envs: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_aurora-engine"));
        cmd.args(args).env("AURORA_OUTPUT", "null").env("AURORA_NULL_SPEED", "10").env("AURORA_OSMEDIA", "0");
        for (k, v) in envs { cmd.env(k, v); }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || for line in BufReader::new(stdout).lines().map_while(Result::ok) { let _ = tx.send(line); });
        Engine { child, stdin, rx, next_id: 1, seen: vec![], all: vec![] }
    }

    pub fn next(&mut self, timeout: Duration) -> Value {
        let line = self.rx.recv_timeout(timeout).expect("engine went quiet");
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
        self.seen.push(v.clone());
        self.all.push(v.clone());
        v
    }

    pub fn wait_for(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        if let Some(v) = self.seen.iter().find(|v| pred(v)) { return v.clone(); }
        loop {
            assert!(start.elapsed() < Duration::from_secs(30), "timed out; seen: {:#?}", self.seen);
            let v = self.next(Duration::from_secs(30));
            if pred(&v) { return v; }
        }
    }

    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
        self.stdin.flush().unwrap();
        self.wait_for(|v| v["id"] == id)
    }

    /// `call` that asserts success and returns `result`.
    pub fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params);
        assert!(v.get("error").is_none(), "{method} failed: {v}");
        v["result"].clone()
    }

    pub fn notified(&mut self, method: &'static str, pred: impl Fn(&Value) -> bool) -> Value {
        self.wait_for(move |v| v["method"] == method && pred(&v["params"]))["params"].clone()
    }

    /// Asks for a clean shutdown and waits for exit (the engine saves state on the way out).
    pub fn close(mut self) -> std::process::ExitStatus {
        let id = self.next_id;
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": "engine.shutdown" })).unwrap();
        self.stdin.flush().unwrap();
        let start = Instant::now();
        loop {
            if let Some(s) = self.child.try_wait().unwrap() { return s; }
            assert!(start.elapsed() < Duration::from_secs(10), "engine didn't exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Engine { fn drop(&mut self) { let _ = self.child.kill(); } }
