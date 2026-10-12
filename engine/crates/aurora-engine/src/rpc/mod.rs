mod dispatch;
mod dsp;
mod library_methods;
mod library_service;
mod session;
mod writer;

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use engine_core::output::null::{NullBackend, NullMode};
use engine_core::output::{system::SystemBackend, OutputBackend};
use engine_core::player::{spawn_player, Command, PlayerConfig, PlayerEvent};
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

fn error_response(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": RpcError { code, message: message.into(), data: None } })
}

/// Parses one request line (see the Task 24 interface): -32700 for bad JSON, -32600 for JSON that
/// isn't a request. A present `"id": null` is an id; only a missing id makes a notification.
pub fn parse_line(line: &str) -> Result<(Option<Value>, Request), Value> {
    let v: Value = serde_json::from_str(line).map_err(|e| error_response(Value::Null, -32700, format!("parse error: {e}")))?;
    let id = v.as_object().and_then(|o| o.get("id").cloned());
    let invalid = |msg: String| error_response(id.clone().unwrap_or(Value::Null), -32600, format!("invalid request: {msg}"));
    let req: Request = serde_json::from_value(v).map_err(|e| invalid(e.to_string()))?;
    if req.jsonrpc != "2.0" { return Err(invalid("jsonrpc must be \"2.0\"".into())); }
    Ok((id, req))
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
    let dsp_store = dsp::DspStore::load(opts.data_dir.as_deref());
    for c in dsp_store.commands() { player.send(c); }
    // The library opens first so a restored queue can pick up album order and ReplayGain from it.
    if let Some(dir) = &opts.data_dir && let Some(r) = session::load(dir).and_then(|s| session::restore(s, |items| if let Some(svc) = &library { dispatch::enrich(svc, items) })) {
        player.send(Command::Restore(Box::new(r)));
    }

    out.notify("engine.ready", EngineReady { version: env!("CARGO_PKG_VERSION").into(), protocol_version: PROTOCOL_VERSION, features: features() });
    if let Some(e) = &library_error { notify_error(&out, e.code(), format!("Library unavailable: {e}")); }
    if let Some(svc) = &library { svc.begin(); }

    let os_tx = crate::osmedia::enabled().then(|| {
        let (tx, rx) = crossbeam_channel::unbounded::<protocol::PlayerState>();
        let lib = library.clone();
        let cover = move |art: i64| lib.as_ref().and_then(|s| s.lib.artwork_info(art, 300).ok()).map(|i| PathBuf::from(i.path));
        crate::osmedia::spawn(rx, player.commander(), player.state_reader(), cover);
        tx
    });

    let (out2, queue2, lib2) = (out.clone(), queue.clone(), library.clone());
    std::thread::Builder::new().name("aurora-events".into()).spawn(move || {
        for ev in rx.iter() {
            match ev {
                PlayerEvent::State(s) => {
                    if let Some(tx) = &os_tx { let _ = tx.send(s.clone()); }
                    out2.notify("player.state", s)
                }
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
                PlayerEvent::Spectrum(bins) => out2.frame(protocol::frames::FRAME_SPECTRUM, &bins),
                PlayerEvent::Ended => {}
            }
        }
    }).expect("spawn event thread");

    let stop_saver = Arc::new(AtomicBool::new(false));
    let saver = opts.data_dir.clone().map(|dir| {
        let (reader, queue, stop) = (player.state_reader(), queue.clone(), stop_saver.clone());
        std::thread::Builder::new().name("aurora-session".into()).spawn(move || {
            let mut last: Option<session::Session> = None;
            while !stop.load(SeqCst) {
                for _ in 0..50 { if stop.load(SeqCst) { return; } std::thread::sleep(Duration::from_millis(100)); }
                let snap = session::snapshot(&queue.lock().unwrap_or_else(|p| p.into_inner()), &reader.get());
                if last.as_ref() != Some(&snap) {
                    match session::save(&dir, &snap) { Ok(()) => last = Some(snap), Err(e) => tracing::warn!(error = %e, "couldn't save session.json") }
                }
            }
        }).expect("spawn session saver")
    });

    let ctx = Ctx { player, backend, queue, library: library.clone(), library_error: library_error.map(|e| e.to_string()), dsp: Mutex::new(dsp_store), out: out.clone() };
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match stdin.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            out.send(&error_response(Value::Null, -32700, "parse error: request is not valid UTF-8"));
            continue;
        };
        let line = line.trim();
        if line.is_empty() { continue; }
        let (id, req) = match parse_line(line) { Ok(p) => p, Err(resp) => { out.send(&resp); continue; } };
        let result = dispatch(&ctx, &req.method, req.params);
        let Some(id) = id else { continue }; // notifications from the client get no response
        match result {
            Ok(Outcome::Reply(v)) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": v })),
            Ok(Outcome::Shutdown) => { out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null })); break; }
            Err(e) => out.send(&json!({ "jsonrpc": "2.0", "id": id, "error": e })),
        }
    }
    stop_saver.store(true, SeqCst);
    if let Some(j) = saver { let _ = j.join(); }
    if let Some(dir) = &opts.data_dir {
        let snap = session::snapshot(&ctx.queue.lock().unwrap_or_else(|p| p.into_inner()), &ctx.player.state());
        if let Err(e) = session::save(dir, &snap) { tracing::warn!(error = %e, "couldn't save session.json"); }
    }
    if let Some(svc) = &library { svc.shutdown(); }
    ctx.player.shutdown();
    0
}

#[cfg(test)]
mod tests {
    use super::parse_line;
    use serde_json::json;

    #[test]
    fn classifies_lines() {
        assert_eq!(parse_line("{nope").unwrap_err()["error"]["code"], -32700);
        assert_eq!(parse_line("42").unwrap_err()["error"]["code"], -32600);
        assert_eq!(parse_line(r#"{"jsonrpc":"1.0","id":3,"method":"x"}"#).unwrap_err()["id"], 3);
        let (id, req) = parse_line(r#"{"jsonrpc":"2.0","id":null,"method":"player.state"}"#).unwrap();
        assert_eq!(id, Some(json!(null)));
        assert_eq!(req.method, "player.state");
        let (id, _) = parse_line(r#"{"jsonrpc":"2.0","method":"player.pause"}"#).unwrap();
        assert_eq!(id, None, "no id = notification");
    }
}
