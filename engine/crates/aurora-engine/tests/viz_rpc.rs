use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Kills the engine when the test ends, including when an assertion fails.
struct Engine(std::process::Child);
impl Drop for Engine { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }

fn fixture(name: &str) -> String { format!("{}/../../../fixtures/out/{name}", env!("CARGO_MANIFEST_DIR")) }

#[test]
fn spectrum_frames_are_binary_and_interleave_with_json() {
    let mut child = Engine(Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
        .env("AURORA_OUTPUT", "null").env("AURORA_NULL_SPEED", "1").env("AURORA_OSMEDIA", "0")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
    let mut stdin = child.0.stdin.take().unwrap();
    let mut stdout = child.0.stdout.take().unwrap();
    let path = fixture("flac16_44k.flac");
    for (id, m, p) in [(1, "viz.subscribe", serde_json::json!({})), (2, "queue.setPaths", serde_json::json!({ "paths": [path] })), (3, "player.play", serde_json::json!({}))] {
        writeln!(stdin, "{}", serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": m, "params": p })).unwrap();
    }
    stdin.flush().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || { let mut b = [0u8; 4096]; while let Ok(n) = stdout.read(&mut b) { if n == 0 { break; } let _ = tx.send(b[..n].to_vec()); } });
    let mut buf: Vec<u8> = Vec::new();
    let start = Instant::now();
    let mut json_lines = 0;
    loop {
        assert!(start.elapsed() < Duration::from_secs(10), "no frame; {json_lines} JSON lines");
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) { buf.extend(chunk); }
        // Consume complete JSON lines and frames from the front.
        loop {
            if buf.first() == Some(&0x1E) {
                if buf.len() < 5 { break; }
                let len = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
                if buf.len() < 5 + len { break; }
                assert_eq!(len, 65, "kind + 64 bins");
                assert_eq!(buf[5], 1, "spectrum kind");
                assert!(json_lines >= 1, "engine.ready (JSON) comes first");
                return;
            }
            let Some(nl) = buf.iter().position(|b| *b == b'\n') else { break };
            let line: Vec<u8> = buf.drain(..=nl).collect();
            let v: serde_json::Value = serde_json::from_slice(&line).expect("a JSON line");
            assert!(v.is_object());
            json_lines += 1;
        }
    }
}
