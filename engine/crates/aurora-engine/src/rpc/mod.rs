mod dispatch;
mod library_methods;
mod library_service;
mod writer;

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use engine_core::output::null::{NullBackend, NullMode};
use engine_core::output::{system::SystemBackend, OutputBackend};
use engine_core::player::{spawn_player, PlayerConfig, PlayerEvent};
use protocol::rpc::{Request, RpcError};
use protocol::{EngineReady, ErrorCode, LibraryScope, QueueChanged, PROTOCOL_VERSION};
use serde_json::{json, Value};

use dispatch::{dispatch, Ctx, Outcome};
use library_service::{changed, notify_error, LibraryService};
use writer::Writer;

#[derive(Default)]
pub struct RunOptions {
    /// Where the library, artwork cache and session live. None = no persistence (M1 behaviour).
    pub data_dir: Option<PathBuf>,
}

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

/// Reads properties through our own decoders for files `lofty` can't parse (DSF/DFF).
fn fallback_probe() -> library::scan::Fallback {
    Arc::new(|p: &Path| {
        let dec = engine_core::decode::open(p).ok()?;
        let i = dec.info();
        let s = engine_core::decode::source_summary(i);
        Some(library::tags::AudioProps {
            duration_ms: i.total_frames.map(|f| f * 1000 / i.spec.sample_rate.max(1) as u64),
            codec: Some(i.codec),
            container: library::tags::container_of(p),
            bitrate_kbps: i.bitrate_kbps,
            sample_rate: Some(i.dsd_rate.unwrap_or(s.sample_rate)),
            bit_depth: s.bit_depth,
            channels: Some(s.channels),
        })
    })
}

/// Player errors that mean the file itself can't be played (recorded on its library track).
const UNPLAYABLE: [ErrorCode; 4] = [ErrorCode::FileMissing, ErrorCode::UnsupportedFormat, ErrorCode::CodecNotInBuild, ErrorCode::DecodeFailed];

fn record_play(svc: &LibraryService, out: &Writer, track_id: Option<i64>, path: &Path) {
    let path = path.to_string_lossy();
    let Some(id) = track_id.or_else(|| svc.lib.track_id_for_path(&path)) else { return };
    let _ = svc.lib.set_unplayable(&path, None);
    match svc.lib.record_play(id, library::unix_now()) {
        Ok(()) => changed(out, &[LibraryScope::History]),
        Err(e) => tracing::warn!(error = %e, "couldn't record a play"),
    }
}

pub fn run(opts: RunOptions) -> i32 {
    let out = Arc::new(Writer::new());
    let backend = backend_from_env();
    let (tx, rx) = crossbeam_channel::unbounded::<PlayerEvent>();
    let player = spawn_player(backend.clone(), PlayerConfig::default(), tx);
    let queue = Arc::new(Mutex::new(QueueChanged { items: vec![], current: None }));
    let (library, library_error) = match &opts.data_dir {
        Some(dir) => match LibraryService::open(dir, out.clone(), fallback_probe()) {
            Ok(s) => (Some(Arc::new(s)), None),
            Err(e) => (None, Some(e)),
        },
        None => (None, None),
    };

    out.notify("engine.ready", EngineReady { version: env!("CARGO_PKG_VERSION").into(), protocol_version: PROTOCOL_VERSION, features: features() });
    if let Some(e) = library_error { notify_error(&out, e.code(), format!("Library unavailable: {e}")); }
    if let Some(svc) = &library { svc.begin(); }

    let (out2, queue2, lib2) = (out.clone(), queue.clone(), library.clone());
    std::thread::Builder::new().name("aurora-events".into()).spawn(move || {
        for ev in rx.iter() {
            match ev {
                PlayerEvent::State(s) => out2.notify("player.state", s),
                PlayerEvent::SignalPath(s) => out2.notify("player.signalPath", s),
                PlayerEvent::Queue(q) => { *queue2.lock().unwrap_or_else(|p| p.into_inner()) = q.clone(); out2.notify("queue.changed", q) }
                PlayerEvent::Error(e) => {
                    if let (Some(svc), Some(path)) = (&lib2, &e.path)
                        && UNPLAYABLE.iter().any(|c| c.code() == e.code)
                        && svc.lib.set_unplayable(path, Some(&e.name)).unwrap_or(false)
                    {
                        changed(&out2, &[LibraryScope::Tracks]);
                    }
                    out2.notify("engine.error", e)
                }
                PlayerEvent::DevicesChanged(d) => out2.notify("devices.changed", d),
                PlayerEvent::Played { track_id, path } => if let Some(svc) = &lib2 { record_play(svc, &out2, track_id, &path) },
                PlayerEvent::Ended => {}
            }
        }
    }).expect("spawn event thread");

    let ctx = Ctx { player, backend, queue, library: library.clone() };
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
    if let Some(svc) = &library { svc.shutdown(); }
    ctx.player.shutdown();
    0
}
