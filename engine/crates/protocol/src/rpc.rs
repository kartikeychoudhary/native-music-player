use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::types::OutputMode;

#[derive(Deserialize, Debug)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Serialize, Debug)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Serialize, Debug)]
pub struct Notification<'a> {
    pub jsonrpc: &'static str,
    pub method: &'a str,
    pub params: Value,
}

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[ts(export)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub data: Option<Value>,
}

#[derive(Serialize, Deserialize, TS, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayParams { #[serde(default)] #[ts(optional)] pub queue_item_id: Option<u32> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueSetPathsParams { pub paths: Vec<String>, #[serde(default)] pub start_index: u32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SeekParams { pub ms: u64 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VolumeParams { pub volume: f32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModeParams { pub mode: OutputMode }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceParams { pub id: Option<String> }
