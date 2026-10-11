use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use engine_core::output::OutputBackend;
use engine_core::player::{Command, PlayerHandle};
use protocol::rpc::*;
use protocol::{ErrorCode, QueueChanged};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

pub struct Ctx {
    pub player: PlayerHandle,
    pub backend: Arc<dyn OutputBackend>,
    pub queue: Arc<Mutex<QueueChanged>>,
}

pub enum Outcome { Reply(Value), Shutdown }

fn err(code: i32, message: impl Into<String>) -> RpcError { RpcError { code, message: message.into(), data: None } }

fn params<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| err(-32602, format!("invalid params: {e}")))
}

pub fn dispatch(ctx: &Ctx, method: &str, p: Value) -> Result<Outcome, RpcError> {
    let ok = |c: Command| -> Result<Outcome, RpcError> { ctx.player.send(c); Ok(Outcome::Reply(Value::Null)) };
    match method {
        "player.play" => { let p: PlayParams = params(p)?; ok(Command::Play(p.queue_item_id)) }
        "player.pause" => ok(Command::Pause),
        "player.toggle" => ok(Command::Toggle),
        "player.stop" => ok(Command::Stop),
        "player.next" => ok(Command::Next),
        "player.prev" => ok(Command::Prev),
        "player.seek" => { let p: SeekParams = params(p)?; ok(Command::Seek(p.ms)) }
        "player.setVolume" => {
            let p: VolumeParams = params(p)?;
            if !(0.0..=1.0).contains(&p.volume) { return Err(err(ErrorCode::InvalidArgument.code(), "volume must be in [0, 1]")); }
            ok(Command::SetVolume(p.volume))
        }
        "player.setMode" => { let p: ModeParams = params(p)?; ok(Command::SetMode(p.mode)) }
        "player.setDevice" => { let p: DeviceParams = params(p)?; ok(Command::SetDevice(p.id)) }
        "player.state" => Ok(Outcome::Reply(serde_json::to_value(ctx.player.state()).unwrap())),
        "queue.setPaths" => {
            let p: QueueSetPathsParams = params(p)?;
            if (p.start_index as usize) >= p.paths.len().max(1) || (p.paths.is_empty() && p.start_index > 0) {
                return Err(err(ErrorCode::InvalidArgument.code(), "startIndex out of range"));
            }
            ok(Command::SetQueue { paths: p.paths.into_iter().map(PathBuf::from).collect(), start: p.start_index as usize })
        }
        "queue.get" => Ok(Outcome::Reply(serde_json::to_value(&*ctx.queue.lock().unwrap_or_else(|p| p.into_inner())).unwrap())),
        "devices.list" => ctx.backend.devices().map(|d| Outcome::Reply(serde_json::to_value(d).unwrap())).map_err(|e| err(e.code().code(), e.to_string())),
        "engine.shutdown" => Ok(Outcome::Shutdown),
        other => Err(err(-32601, format!("method not found: {other}"))),
    }
}
