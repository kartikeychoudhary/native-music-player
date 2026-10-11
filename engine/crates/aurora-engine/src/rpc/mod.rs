mod dispatch;
mod writer;

use std::io::BufRead;
use std::sync::{Arc, Mutex};

use engine_core::output::null::{NullBackend, NullMode};
use engine_core::output::{system::SystemBackend, OutputBackend};
use engine_core::player::{spawn_player, PlayerConfig, PlayerEvent};
use protocol::rpc::{Request, RpcError};
use protocol::{EngineReady, QueueChanged, PROTOCOL_VERSION};
use serde_json::{json, Value};

use dispatch::{dispatch, Ctx, Outcome};
use writer::Writer;

fn features() -> Vec<String> {
    let mut f = vec!["opus".to_string(), "dsd".into(), "dop".into()];
    if cfg!(feature = "fdk-aac") { f.push("fdk-aac".into()); }
    f
}

fn backend_from_env() -> Arc<dyn OutputBackend> {
    if std::env::var("AURORA_OUTPUT").as_deref() == Ok("null") {
        let speed = std::env::var("AURORA_NULL_SPEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
        Arc::new(NullBackend::new(NullMode::Realtime { speed }).0)
    } else {
        Arc::new(SystemBackend::new())
    }
}

pub fn run() -> i32 {
    let out = Arc::new(Writer::new());
    let backend = backend_from_env();
    let (tx, rx) = crossbeam_channel::unbounded::<PlayerEvent>();
    let player = spawn_player(backend.clone(), PlayerConfig::default(), tx);
    let queue = Arc::new(Mutex::new(QueueChanged { items: vec![], current: None }));

    out.notify("engine.ready", EngineReady { version: env!("CARGO_PKG_VERSION").into(), protocol_version: PROTOCOL_VERSION, features: features() });

    let (out2, queue2) = (out.clone(), queue.clone());
    std::thread::Builder::new().name("aurora-events".into()).spawn(move || {
        for ev in rx.iter() {
            match ev {
                PlayerEvent::State(s) => out2.notify("player.state", s),
                PlayerEvent::SignalPath(s) => out2.notify("player.signalPath", s),
                PlayerEvent::Queue(q) => { *queue2.lock().unwrap_or_else(|p| p.into_inner()) = q.clone(); out2.notify("queue.changed", q) }
                PlayerEvent::Error(e) => out2.notify("engine.error", e),
                PlayerEvent::DevicesChanged(d) => out2.notify("devices.changed", d),
                PlayerEvent::Ended => {}
            }
        }
    }).expect("spawn event thread");

    let ctx = Ctx { player, backend, queue };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => { out.send(&json!({ "jsonrpc": "2.0", "id": Value::Null, "error": RpcError { code: -32700, message: format!("parse error: {e}"), data: None } })); continue; }
        };
        let id = req.id.clone();
        let result = dispatch(&ctx, &req.method, req.params);
        let Some(id) = id else { continue }; // notifications from the client get no response
        match result {
            Ok(Outcome::Reply(v)) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": v })),
            Ok(Outcome::Shutdown) => { out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null })); break; }
            Err(e) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "error": e })),
        }
    }
    ctx.player.shutdown();
    0
}
