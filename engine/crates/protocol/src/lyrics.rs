//! Lyrics (spec §3.9): `{ synced, lines: [{ tMs?, text, words? }] }`.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LyricWord { pub t_ms: u32, pub text: String }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LyricLine {
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub t_ms: Option<u32>,
    pub text: String,
    /// Word-level timing from enhanced LRC (`<mm:ss.xx>` tags), when present.
    #[serde(default, skip_serializing_if = "Option::is_none")] #[ts(optional)] pub words: Option<Vec<LyricWord>>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Lyrics { pub synced: bool, pub lines: Vec<LyricLine> }

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LyricsParams { pub track_id: i64 }
