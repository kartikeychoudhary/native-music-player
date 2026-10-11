use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::library::RecentKind;
use crate::types::{OutputMode, RepeatMode};

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

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueSetParams {
    pub track_ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub start_index: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackIdsParams { pub track_ids: Vec<i64> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueMoveParams { pub from: u32, pub to: u32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueRemoveParams { pub item_ids: Vec<u32> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShuffleParams { pub shuffle: bool }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepeatParams { pub repeat: RepeatMode }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IdParams { pub id: i64 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PathParams { pub path: String }

#[derive(Serialize, Deserialize, TS, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RescanParams { #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub folder_id: Option<i64> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ArtworkParams { pub id: i64, pub size: u32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchParams { pub text: String, #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub limit: Option<u32> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentParams { pub kind: RecentKind, #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub limit: Option<u32> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NameParams { pub name: String }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RenameParams { pub id: i64, pub name: String }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistAddParams {
    pub id: i64,
    pub track_ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub index: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistRemoveParams { pub id: i64, pub item_ids: Vec<i64> }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistReorderParams { pub id: i64, pub from: u32, pub to: u32 }

#[derive(Serialize, Deserialize, TS, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportParams { pub id: i64, pub path: String }
