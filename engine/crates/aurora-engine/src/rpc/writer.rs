use std::io::Write;
use std::sync::Mutex;

use serde_json::Value;

/// Serializes whole lines to stdout. The only code allowed to write to stdout in rpc mode.
pub struct Writer { lock: Mutex<()> }

impl Writer {
    pub fn new() -> Self { Self { lock: Mutex::new(()) } }
    pub fn send(&self, v: &Value) {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = std::io::stdout().lock();
        let _ = serde_json::to_writer(&mut out, v);
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
    /// A binary frame (spec §4.1): 0x1E, u32 LE payload length, then the payload (`kind` + bytes).
    pub fn frame(&self, kind: u8, payload: &[u8]) {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = std::io::stdout().lock();
        let len = (1 + payload.len()) as u32;
        let _ = out.write_all(&[protocol::frames::FRAME_MARK]);
        let _ = out.write_all(&len.to_le_bytes());
        let _ = out.write_all(&[kind]);
        let _ = out.write_all(payload);
        let _ = out.flush();
    }
    pub fn notify(&self, method: &str, params: impl serde::Serialize) {
        self.send(&serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }
}
