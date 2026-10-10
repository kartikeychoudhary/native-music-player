# Aurora M1 — Audio Engine + App Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust `aurora-engine` sidecar (all decoders incl. HE-AAC + DSD, gapless, processed + bit-perfect output on Windows/macOS/Linux, device + Bluetooth probe, signal-path reporting, JSON-RPC over stdio) and a minimal Studio-styled Electron shell that opens files, plays them and shows an accurate signal path.

**Architecture:** A Cargo workspace (`protocol`, `engine-core`, `aurora-engine`, `fixtures-gen`) produces one sidecar binary. Electron's main process spawns and supervises the sidecar and bridges JSON-RPC to a sandboxed React renderer through a typed preload API. The decode thread writes device-format bytes into a lock-free ring buffer that a realtime output callback drains.

**Tech Stack:** Rust (edition 2024, stable), symphonia 0.6.1, opus 0.4.0 (libopus), fdk-aac 0.8.0, rubato 5.0.1 + audioadapter-buffers 5.1, rtrb 0.4.0, cpal 0.18.2, wasapi 0.25.0, coreaudio-rs 0.14.2, alsa 0.12.1, serde/serde_json, ts-rs 12.0.1, crossbeam-channel 0.5, clap 4.6, thiserror 2, tracing. Electron 44.7.0, electron-vite 5.0.0, Vite 7.3.7, React 19.3.0, TypeScript 5.9.3, Zustand 5, Vitest 5, Playwright 1.64, lucide-react, @fontsource/inter, @fontsource/jetbrains-mono.

**Spec:** `docs/superpowers/specs/2026-10-10-aurora-music-player-design.md` (this plan implements **Milestone 1** from §9; executors read both).

## Global Constraints

- Sidecar binary name: `aurora-engine`; crates: `protocol`, `engine-core`, `aurora-engine`, `fixtures-gen` (publish = false). The `library` crate belongs to M2; don't create it.
- **No FFmpeg at runtime.** ffmpeg is a test-fixture tool only.
- `fdk-aac` is a cargo feature of `engine-core` (and re-exported by `aurora-engine`), **on by default**. Builds without it must compile and return `CODEC_NOT_IN_BUILD` for HE-AAC.
- **Output thread rule:** no allocation, no locks, no I/O, no logging inside output callbacks.
- JSON-RPC 2.0, newline-delimited, on stdin/stdout. Logs go to **stderr only**; nothing else may write to stdout. `PROTOCOL_VERSION = 1`.
- Protocol types are defined once in `protocol` with `ts-rs` and exported to `shared/protocol/`. The generated files are committed, and CI fails if they're stale.
- **Bluetooth honesty:** for Bluetooth outputs, the bit-perfect badge text is exactly **"Bit-perfect to OS"** (`bitPerfectScope: "os"`). When the codec is unknown, the UI text is exactly **"Codec not reported by the OS"**. Never guess a codec.
- **Bit-perfect mode** disables software volume (and in later milestones ReplayGain/EQ/crossfade). The UI greys out the volume slider.
- **Studio tokens** (spec §5.5): `--bg #17151A`, `--surface #100E12`, `--raised #1B181F`, `--border #2A2530`, `--text #EFE9E1`, `--accent #FFB547`, `--accent-2 #FF7A3D`, `--ok #6FD39A`. Fonts: Inter (UI) and JetBrains Mono (technical readouts). Icons: Lucide.
- **Trade-dress guardrails:** no `#1DB954`-family green, no Circular/Gotham-like fonts, no circular green play button.
- Renderer security: `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`.
- Error codes (spec §4.5): `TRACK_NOT_FOUND -32001`, `FILE_MISSING -32002`, `UNSUPPORTED_FORMAT -32003`, `CODEC_NOT_IN_BUILD -32004`, `DECODE_FAILED -32005`, `DEVICE_UNAVAILABLE -32006`, `EXCLUSIVE_DENIED -32007`, `DB_ERROR -32008`, `INVALID_ARGUMENT -32009`.
- License: MIT. Every third-party native or font dependency gets an entry in `THIRD_PARTY_NOTICES.md`.
- **Explicitly deferred out of M1** (spec places them in later milestones): library/DB/scanner, `session.json`, OS media keys (`souvlaki`), ReplayGain/EQ/crossfade, spectrum analyzer and binary frames, mini-player, tray, installers/auto-update. M1 adds one method that isn't in the spec's method table, `queue.setPaths(paths, startIndex)`, because there's no library to supply track IDs yet. M2 adds `queue.set(trackIds, …)`.

## Review Focus

These are inputs the spec implies but no feature test naturally covers. Each has a pinned test in the task named:
1. **A corrupt frame mid-file** (damaged MP3) keeps playing past the damage rather than aborting the track. Test: Task 5, `corrupt_mp3_decodes_past_damage`.
2. **Mono and 5.1 sources on a stereo device** play: mono is duplicated to both channels, 5.1 is downmixed without clipping, and bit-perfect falls back to processed with a note. Tests: Task 10 `mono_duplicates_to_stereo` / `downmix_51_never_clips`; Task 12 `channel_mismatch_falls_back_to_processed`.
3. **Non-ASCII file paths** (`Beyoncé/日本語.flac`) survive JSON-RPC and play. Test: Task 19, `plays_unicode_path`.
4. **Seek edge cases:** seeking to 0, seeking past the end (moves to the next track), and seeking while the next track is pre-rolled for gapless. Tests: Task 14, `seek_past_end_advances` / `seek_during_preroll_discards_next`.
5. **Very short (0.5 s) and empty (0-byte) files** in the queue: short tracks play back-to-back gaplessly, and empty files are reported as `UNSUPPORTED_FORMAT` and skipped. Tests: Task 14, `short_tracks_play_back_to_back` / `empty_file_is_skipped`.

---

## File Structure

```
Cargo + tooling
  rust-toolchain.toml                      pin stable + rustfmt/clippy
  engine/Cargo.toml                        workspace, shared dependency versions
  engine/.cargo/config.toml                TS_RS_EXPORT_DIR, TS_RS_LARGE_INT
protocol (serde + ts-rs types, zero logic)
  engine/crates/protocol/src/lib.rs        re-exports, PROTOCOL_VERSION
  engine/crates/protocol/src/types.rs      Codec, Transport, SignalPath, DeviceInfo, PlayerState…
  engine/crates/protocol/src/rpc.rs        Request/Response/Notification + method params
  engine/crates/protocol/src/errors.rs     ErrorCode
  engine/crates/protocol/tests/export.rs   writes ProtocolVersion.ts
engine-core
  src/lib.rs
  src/pcm.rs                               PcmSpec, SampleKind, PcmData, PcmBlock
  src/decode/mod.rs                        TrackDecoder trait, SourceInfo, DecodeError, open()
  src/decode/trim.rs                       Trimmer (track-level delay/valid-length trimming)
  src/decode/mp4_gapless.rs                iTunSMPB + elst parser
  src/decode/symphonia_dec.rs              MP3/AAC-LC/ALAC/FLAC/WAV/AIFF/Vorbis
  src/decode/aac_config.rs                 AudioSpecificConfig parser (HE-AAC routing)
  src/decode/fdk_dec.rs                    HE-AAC via fdk-aac (feature "fdk-aac")
  src/decode/opus_dec.rs                   Ogg Opus via libopus
  src/decode/dsd/mod.rs                    DsdDecoder (TrackDecoder), DsdOutput
  src/decode/dsd/dsf.rs  dff.rs            container parsers → DsdStream
  src/decode/dsd/decimate.rs               DSD→PCM multi-stage FIR
  src/decode/dsd/dop.rs                    DoP packer
  src/decode/dsd/modulator.rs              test-support sigma-delta modulator
  src/dsp/mod.rs convert.rs channels.rs volume.rs resample.rs
  src/output/mod.rs                        OutputBackend/OutputStream traits, DeviceCaps
  src/output/format.rs                     DeviceSample, OutputFormat
  src/output/ring.rs                       StreamShared, RingSource, ring_pair
  src/output/null.rs                       NullBackend (recording, manual or realtime)
  src/output/cpal_out.rs                   shared-mode backend
  src/output/wasapi_excl.rs                Windows exclusive (cfg windows)
  src/output/coreaudio_hog.rs              macOS hog mode (cfg macos)
  src/output/alsa_hw.rs                    Linux hw: direct (cfg linux)
  src/output/system.rs                     SystemBackend: picks shared vs exclusive impl
  src/probe/mod.rs  pipewire.rs            Bluetooth codec detection
  src/player/mod.rs                        Player thread, PlayerHandle, Command, PlayerEvent
  src/player/pipeline.rs                   plan_output(): pure output-format decision
  src/player/chain.rs                      ProcessChain: PcmBlock → device bytes
  src/player/signal_path.rs                build_signal_path()
  tests/common/mod.rs                      fixture paths + signal helpers
  tests/decode_lossless.rs decode_lossy.rs gapless.rs he_aac.rs dsd.rs player.rs
aurora-engine (binary)
  src/main.rs                              clap CLI: rpc (default) | probe | play | devices
  src/rpc/mod.rs                           Server: stdin reader, dispatch, event pump
  src/rpc/writer.rs                        serialized stdout line writer
  src/rpc/dispatch.rs                      method → Command mapping
  tests/rpc.rs                             spawns the binary, drives JSON-RPC
fixtures-gen (dev binary)
  src/main.rs                              HE-AAC ADTS, DSF/DFF, corrupt MP3
fixtures/generate.sh                       ffmpeg + fixtures-gen → fixtures/out/
shared/protocol/*.ts                       generated (committed)
app/ (Electron)
  package.json electron.vite.config.ts tsconfig.json tsconfig.node.json tsconfig.web.json vitest.config.ts playwright.config.ts
  src/shared/api.ts                        method map + AuroraApi type
  src/main/index.ts                        windows, IPC handlers, dialog
  src/main/engine/lineCodec.ts             LineDecoder
  src/main/engine/EngineClient.ts          spawn, correlate, events, handshake
  src/main/engine/supervisor.ts            restart policy + EngineSupervisor
  src/main/engine/enginePath.ts            dev/packaged binary location
  src/preload/index.ts                     window.aurora
  src/renderer/index.html main.tsx App.tsx
  src/renderer/styles/tokens.css global.css
  src/renderer/lib/format.ts               badge + time formatting
  src/renderer/stores/player.ts            zustand store wired to events
  src/renderer/components/IconRail.tsx PlayerBar.tsx SeekBar.tsx Transport.tsx
     FormatBadges.tsx SignalPathPanel.tsx DevicePicker.tsx Home.tsx (+ .module.css each)
  test/*.test.ts(x)  e2e/smoke.spec.ts
.github/workflows/ci.yml
THIRD_PARTY_NOTICES.md  LICENSE  package.json (root scripts)
```

---

### Task 1: Toolchain, workspace and protocol crate

**Files:**
- Create: `rust-toolchain.toml`, `engine/Cargo.toml`, `engine/.cargo/config.toml`
- Create: `engine/crates/protocol/Cargo.toml`, `src/lib.rs`, `src/types.rs`, `src/rpc.rs`, `src/errors.rs`, `tests/export.rs`
- Create: `LICENSE`, `THIRD_PARTY_NOTICES.md`
- Generated: `shared/protocol/*.ts`

**Interfaces:**
- Produces: `protocol::{PROTOCOL_VERSION, Codec, Transport, OutputMode, ShareMode, BitPerfectScope, PlaybackStatus, StageKind, Stage, SourceSummary, DecoderSummary, GaplessSummary, OutputSummary, BluetoothInfo, SignalPath, DeviceInfo, TrackSummary, PlayerState, QueueItem, QueueChanged, EngineReady, EngineErrorEvent, ErrorCode, rpc::{Request, Response, Notification, RpcError, PlayParams, QueueSetPathsParams, SeekParams, VolumeParams, ModeParams, DeviceParams}}`. Every struct uses `#[serde(rename_all = "camelCase")]`.

- [ ] **Step 1: Install the toolchain (Windows dev machine)**

Run in PowerShell. Each command is idempotent:
```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Rustlang.Rustup -e
winget install --id Kitware.CMake -e
winget install --id LLVM.LLVM -e
```
Open a new terminal, then:
```bash
rustup default stable
rustc --version
cmake --version
ffmpeg -version
```
Expected: rustc ≥ 1.85 and cmake ≥ 3.20. ffmpeg (8.x) is already installed on this machine.
(macOS: `xcode-select --install && brew install cmake ffmpeg`. Linux: `sudo apt install build-essential cmake clang pkg-config libasound2-dev libpipewire-0.3-dev libspa-0.2-dev ffmpeg`.)

- [ ] **Step 2: Create the workspace files**

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

`engine/Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/protocol", "crates/engine-core", "crates/aurora-engine", "crates/fixtures-gen"]

[workspace.package]
edition = "2024"
license = "MIT"
version = "0.1.0"
rust-version = "1.85"

[workspace.dependencies]
protocol = { path = "crates/protocol" }
engine-core = { path = "crates/engine-core", default-features = false }
serde = { version = "1", features = ["derive"] }
serde_json = "1.0.152"
ts-rs = { version = "12.0.1", features = ["serde-compat"] }
thiserror = "2.0.21"
anyhow = "1.0.104"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["json", "env-filter"] }
crossbeam-channel = "0.5.17"
symphonia = { version = "0.6.1", features = ["all"] }
opus = "0.4.0"
fdk-aac = "0.8.0"
rubato = "5.0.1"
audioadapter-buffers = "5.1"
rtrb = "0.4.0"
cpal = "0.18.2"
clap = { version = "4.6.7", features = ["derive"] }
```
Create `crates/engine-core`, `crates/aurora-engine` and `crates/fixtures-gen` as empty placeholder crates now, so the workspace resolves (each gets `Cargo.toml` with `name`, `version.workspace = true`, `edition.workspace = true`, and a `src/lib.rs` or `src/main.rs` containing `fn main() {}`). Later tasks fill them in.

`engine/.cargo/config.toml`:
```toml
[env]
TS_RS_EXPORT_DIR = { value = "../shared/protocol", relative = true }
TS_RS_LARGE_INT = "number"
```

- [ ] **Step 3: Write the failing export test**

`engine/crates/protocol/tests/export.rs`:
```rust
use protocol::*;
use std::path::PathBuf;

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../shared/protocol")
}

#[test]
fn writes_protocol_version_ts() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("ProtocolVersion.ts"),
        format!("// Generated by protocol/tests/export.rs. Do not edit.\nexport const PROTOCOL_VERSION = {PROTOCOL_VERSION};\n"),
    )
    .unwrap();
}

#[test]
fn signal_path_serializes_camel_case() {
    let sp = SignalPath {
        source: SourceSummary { container: "flac".into(), codec: Codec::Flac, bitrate_kbps: None, sample_rate: 96000, bit_depth: Some(24), channels: 2, dsd_rate: None },
        decoder: DecoderSummary { name: "symphonia".into(), gapless: None },
        stages: vec![Stage { kind: StageKind::Resampler, active: false, detail: "rate matched".into() }],
        output: OutputSummary { device_id: "d".into(), name: "DAC".into(), transport: Transport::Usb, mode: ShareMode::Exclusive, sample_rate: 96000, format: "s24".into() },
        bluetooth: None,
        bit_perfect: true,
        bit_perfect_scope: Some(BitPerfectScope::Device),
        underruns: 0,
        notes: vec![],
    };
    let v = serde_json::to_value(&sp).unwrap();
    assert_eq!(v["bitPerfect"], true);
    assert_eq!(v["bitPerfectScope"], "device");
    assert_eq!(v["source"]["codec"], "flac");
    assert_eq!(v["output"]["mode"], "exclusive");
    assert_eq!(v["stages"][0]["kind"], "resampler");
}

#[test]
fn error_codes_match_spec() {
    assert_eq!(ErrorCode::TrackNotFound.code(), -32001);
    assert_eq!(ErrorCode::InvalidArgument.code(), -32009);
    assert_eq!(ErrorCode::CodecNotInBuild.name(), "CODEC_NOT_IN_BUILD");
}

#[test]
fn output_mode_wire_names() {
    assert_eq!(serde_json::to_string(&OutputMode::BitPerfect).unwrap(), "\"bitPerfect\"");
    assert_eq!(serde_json::to_string(&Codec::HeAacV2).unwrap(), "\"he-aac-v2\"");
}
```

- [ ] **Step 4: Run it to make sure it fails**

Run: `cargo test -p protocol --manifest-path engine/Cargo.toml`
Expected: compile errors (`SignalPath` etc. not found).

- [ ] **Step 5: Implement the protocol crate**

`engine/crates/protocol/Cargo.toml`:
```toml
[package]
name = "protocol"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[dependencies]
serde.workspace = true
serde_json.workspace = true
ts-rs.workspace = true
```

`src/lib.rs`:
```rust
//! Wire types shared by the engine and the Electron app. No logic lives here.
pub mod errors;
pub mod rpc;
pub mod types;

pub use errors::ErrorCode;
pub use types::*;

pub const PROTOCOL_VERSION: u32 = 1;
```

`src/types.rs`:
```rust
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Codec { Mp3, AacLc, HeAac, HeAacV2, Alac, Flac, Pcm, Vorbis, Opus, Dsd }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Transport { Usb, Bluetooth, Builtin, Hdmi, Other }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OutputMode { #[default] Processed, BitPerfect }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ShareMode { Shared, Exclusive }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BitPerfectScope { Device, Os }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum PlaybackStatus { #[default] Stopped, Playing, Paused }

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum StageKind { Channels, Dsd, Resampler, Volume, Dither }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Stage { pub kind: StageKind, pub active: bool, pub detail: String }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceSummary {
    pub container: String,
    pub codec: Codec,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: u32,
    pub bit_depth: Option<u16>,
    pub channels: u16,
    pub dsd_rate: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GaplessSummary { pub delay: u32, pub padding: u32 }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecoderSummary { pub name: String, pub gapless: Option<GaplessSummary> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OutputSummary {
    pub device_id: String,
    pub name: String,
    pub transport: Transport,
    pub mode: ShareMode,
    pub sample_rate: u32,
    /// "s16" | "s24" | "s32" | "f32"
    pub format: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BluetoothInfo {
    /// e.g. "ldac", "aac", "sbc"; None = not reported by the OS.
    pub codec: Option<String>,
    pub quality: Option<String>,
    pub bitrate_kbps: Option<u32>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SignalPath {
    pub source: SourceSummary,
    pub decoder: DecoderSummary,
    pub stages: Vec<Stage>,
    pub output: OutputSummary,
    /// Present only when the output transport is Bluetooth.
    pub bluetooth: Option<BluetoothInfo>,
    pub bit_perfect: bool,
    pub bit_perfect_scope: Option<BitPerfectScope>,
    pub underruns: u64,
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub transport: Transport,
    pub is_default: bool,
    pub sample_rates: Vec<u32>,
    pub exclusive_capable: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackSummary { pub path: String, pub title: String, pub duration_ms: Option<u64> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub queue_index: Option<u32>,
    pub track: Option<TrackSummary>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub mode: OutputMode,
    pub device_id: Option<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueItem { pub id: u32, pub path: String, pub title: String }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueChanged { pub items: Vec<QueueItem>, pub current: Option<u32> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineReady { pub version: String, pub protocol_version: u32, pub features: Vec<String> }

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineErrorEvent { pub code: i32, pub name: String, pub message: String, pub path: Option<String> }
```

`src/errors.rs`:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    TrackNotFound, FileMissing, UnsupportedFormat, CodecNotInBuild, DecodeFailed,
    DeviceUnavailable, ExclusiveDenied, DbError, InvalidArgument,
}

impl ErrorCode {
    pub fn code(self) -> i32 {
        match self {
            Self::TrackNotFound => -32001, Self::FileMissing => -32002, Self::UnsupportedFormat => -32003,
            Self::CodecNotInBuild => -32004, Self::DecodeFailed => -32005, Self::DeviceUnavailable => -32006,
            Self::ExclusiveDenied => -32007, Self::DbError => -32008, Self::InvalidArgument => -32009,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::TrackNotFound => "TRACK_NOT_FOUND", Self::FileMissing => "FILE_MISSING",
            Self::UnsupportedFormat => "UNSUPPORTED_FORMAT", Self::CodecNotInBuild => "CODEC_NOT_IN_BUILD",
            Self::DecodeFailed => "DECODE_FAILED", Self::DeviceUnavailable => "DEVICE_UNAVAILABLE",
            Self::ExclusiveDenied => "EXCLUSIVE_DENIED", Self::DbError => "DB_ERROR",
            Self::InvalidArgument => "INVALID_ARGUMENT",
        }
    }
}
```

`src/rpc.rs`:
```rust
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
    #[ts(optional)]
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
```
`ts(optional)` on `Option` fields with `#[serde(default)]` makes the TS field `field?: T`. Don't add `ts(optional)` to non-default fields.

- [ ] **Step 6: Run the tests (this also generates the TS files)**

Run: `cargo test -p protocol --manifest-path engine/Cargo.toml`
Expected: 4 tests PASS. `shared/protocol/` contains `SignalPath.ts`, `PlayerState.ts`, `ProtocolVersion.ts`, … Open `shared/protocol/PlayerState.ts` and confirm `positionMs: number` (not `bigint`).

- [ ] **Step 7: License files**

`LICENSE`: standard MIT text, `Copyright (c) 2026 Aurora contributors`.
`THIRD_PARTY_NOTICES.md` with headings and license names for: Symphonia (MPL-2.0), libopus (BSD-3-Clause), Fraunhofer FDK AAC (FDK AAC license; state verbatim: "Fraunhofer grants no patent rights; builds without the `fdk-aac` feature exclude this code"), rubato (MIT), cpal (Apache-2.0), wasapi (MIT), coreaudio-rs (MIT/Apache-2.0), alsa-rs (MIT/Apache-2.0), Electron (MIT), React (MIT), Inter (OFL-1.1), JetBrains Mono (OFL-1.1), Lucide (ISC).

- [ ] **Step 8: Commit**

```bash
git add rust-toolchain.toml engine shared LICENSE THIRD_PARTY_NOTICES.md
git commit -m "feat(protocol): workspace scaffold and wire types with ts-rs export"
```

---

### Task 2: Test fixtures (generator script + fixtures-gen)

**Files:**
- Create: `fixtures/generate.sh`, `engine/crates/fixtures-gen/Cargo.toml`, `engine/crates/fixtures-gen/src/main.rs`
- Create: `engine/crates/engine-core/Cargo.toml` (real manifest), `engine/crates/engine-core/src/lib.rs`, `engine/crates/engine-core/src/decode/dsd/modulator.rs`
- Create: `engine/crates/engine-core/tests/common/mod.rs`

**Interfaces:**
- Produces: the fixture files in `fixtures/out/` written by Step 4. Test helpers `common::{fixture(name) -> PathBuf, read_s32le(name) -> Vec<i32>, read_f32le(name) -> Vec<f32>, dominant_freq(samples, channels, ch, rate) -> f64, rms(samples) -> f64, snr_db(reference, test) -> f64, channel(samples, channels, ch) -> Vec<f32>}`.
- Produces: `engine_core::decode::dsd::modulator::{sdm2(input: &[f64]) -> Vec<u8>}` (behind `feature = "test-support"` or `cfg(test)`). Output is 1 bit per sample, **packed MSB-first**, 8 samples per byte.

- [ ] **Step 1: engine-core manifest and lib skeleton**

`engine/crates/engine-core/Cargo.toml`:
```toml
[package]
name = "engine-core"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[features]
default = ["fdk-aac"]
fdk-aac = ["dep:fdk-aac"]
test-support = []

[dependencies]
protocol.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
crossbeam-channel.workspace = true
symphonia.workspace = true
opus.workspace = true
fdk-aac = { workspace = true, optional = true }
rubato.workspace = true
audioadapter-buffers.workspace = true
rtrb.workspace = true
cpal.workspace = true

[target.'cfg(target_os = "linux")'.dependencies]
alsa = "0.12.1"
cpal = { workspace = true, features = ["pipewire"] }

[target.'cfg(target_os = "windows")'.dependencies]
wasapi = "0.25.0"

[target.'cfg(target_os = "macos")'.dependencies]
coreaudio-rs = "0.14.2"

[dev-dependencies]
engine-core = { path = ".", default-features = false, features = ["test-support"] }
```

`src/lib.rs`:
```rust
pub mod decode;
pub mod dsp;
pub mod output;
pub mod pcm;
pub mod player;
pub mod probe;
```
For this task, create empty modules (`decode/mod.rs` containing only `pub mod dsd;`, `decode/dsd/mod.rs` containing only `#[cfg(any(test, feature = "test-support"))] pub mod modulator;`, and empty `dsp/mod.rs`, `output/mod.rs`, `pcm.rs`, `player/mod.rs`, `probe/mod.rs`).

- [ ] **Step 2: Write the modulator with its own unit test**

`src/decode/dsd/modulator.rs`:
```rust
//! Second-order sigma-delta modulator used only to synthesize DSD test signals.

/// Converts samples in [-1, 1] at the DSD rate into a 1-bit stream packed MSB-first.
pub fn sdm2(input: &[f64]) -> Vec<u8> {
    let mut out = vec![0u8; input.len().div_ceil(8)];
    let (mut i1, mut i2, mut y) = (0.0f64, 0.0f64, 0.0f64);
    for (n, &x) in input.iter().enumerate() {
        i1 += x - y;
        i2 += i1 - y;
        y = if i2 >= 0.0 { 1.0 } else { -1.0 };
        if y > 0.0 {
            out[n / 8] |= 0x80 >> (n % 8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_input_sets_proportional_density() {
        // x = 0.5 → density of ones = (1 + 0.5) / 2 = 0.75
        let bits = sdm2(&vec![0.5; 80_000]);
        let ones: u32 = bits.iter().map(|b| b.count_ones()).sum();
        let density = ones as f64 / 80_000.0;
        assert!((density - 0.75).abs() < 0.01, "density {density}");
    }
}
```
Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml modulator`
Expected: PASS. If the workspace fails to build because other crates are empty, make sure `aurora-engine`/`fixtures-gen` still contain `fn main() {}`.

- [ ] **Step 3: fixtures-gen**

`engine/crates/fixtures-gen/Cargo.toml`:
```toml
[package]
name = "fixtures-gen"
version.workspace = true
edition.workspace = true
publish = false

[dependencies]
fdk-aac.workspace = true
engine-core = { path = "../engine-core", features = ["test-support"] }
anyhow.workspace = true
```

`src/main.rs`:
```rust
//! Generates fixtures ffmpeg can't produce: HE-AAC (fdk), DSF/DFF and a corrupt MP3.
use anyhow::{Context, Result};
use engine_core::decode::dsd::modulator::sdm2;
use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder, EncoderParams, Transport};
use std::{f64::consts::PI, fs, path::Path};

fn main() -> Result<()> {
    let out = std::env::args().nth(1).context("usage: fixtures-gen <out-dir>")?;
    let out = Path::new(&out);
    fs::create_dir_all(out)?;
    he_aac(out, "he_aac.aac", AudioObjectType::Mpeg4HeAac, 64_000)?;
    he_aac(out, "he_aac_v2.aac", AudioObjectType::Mpeg4HeAacV2, 32_000)?;
    for (name, mult) in [("dsd64", 64u32), ("dsd128", 128)] {
        let rate = 44_100 * mult;
        let (l, r) = dsd_tones(rate, 1.5);
        fs::write(out.join(format!("{name}.dsf")), dsf(rate, &l, &r))?;
    }
    let (l, r) = dsd_tones(44_100 * 64, 1.5);
    fs::write(out.join("dsd64.dff"), dff(44_100 * 64, &l, &r))?;
    corrupt_mp3(out)?;
    println!("fixtures-gen: done");
    Ok(())
}

/// 2 s stereo: 1 kHz left, 440 Hz right at -6 dBFS, 48 kHz.
fn he_aac(out: &Path, name: &str, aot: AudioObjectType, bitrate: u32) -> Result<()> {
    let enc = Encoder::new(EncoderParams {
        bit_rate: BitRate::Cbr(bitrate),
        sample_rate: 48_000,
        transport: Transport::Adts,
        channels: ChannelMode::Stereo,
        audio_object_type: aot,
    })
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let frames = 96_000usize;
    let mut pcm = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let t = n as f64 / 48_000.0;
        pcm.push((0.5 * (2.0 * PI * 1000.0 * t).sin() * 32767.0) as i16);
        pcm.push((0.5 * (2.0 * PI * 440.0 * t).sin() * 32767.0) as i16);
    }
    // Pad with silence so the encoder flushes its look-ahead.
    pcm.extend(std::iter::repeat_n(0i16, 48_000));
    let mut bytes = Vec::new();
    let mut buf = vec![0u8; 8192];
    let mut pos = 0;
    while pos < pcm.len() {
        let end = (pos + 2048).min(pcm.len());
        let info = enc.encode(&pcm[pos..end], &mut buf).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        bytes.extend_from_slice(&buf[..info.output_size]);
        pos += info.input_consumed.max(1);
    }
    fs::write(out.join(name), bytes)?;
    Ok(())
}

/// Left 1 kHz, right 3 kHz, modulation amplitude 0.5 (−6 dB DSD).
fn dsd_tones(rate: u32, secs: f64) -> (Vec<u8>, Vec<u8>) {
    let n = (rate as f64 * secs) as usize / 8 * 8;
    let tone = |f: f64| (0..n).map(|i| 0.5 * (2.0 * PI * f * i as f64 / rate as f64).sin()).collect::<Vec<_>>();
    (sdm2(&tone(1000.0)), sdm2(&tone(3000.0)))
}

/// DSF: per-channel 4096-byte blocks, LSB-first bit order (bits_per_sample = 1).
fn dsf(rate: u32, l: &[u8], r: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 4096;
    let rev = |v: &[u8]| v.iter().map(|b| b.reverse_bits()).collect::<Vec<u8>>();
    let (l, r) = (rev(l), rev(r));
    let blocks = l.len().div_ceil(BLOCK);
    let mut data = Vec::with_capacity(blocks * BLOCK * 2);
    for b in 0..blocks {
        for ch in [&l, &r] {
            let s = b * BLOCK;
            let e = (s + BLOCK).min(ch.len());
            data.extend_from_slice(&ch[s..e]);
            data.extend(std::iter::repeat_n(0u8, BLOCK - (e - s)));
        }
    }
    let sample_count = (l.len() * 8) as u64;
    let mut f = Vec::new();
    let total = 28 + 52 + 12 + data.len() as u64;
    f.extend_from_slice(b"DSD ");
    f.extend_from_slice(&28u64.to_le_bytes());
    f.extend_from_slice(&total.to_le_bytes());
    f.extend_from_slice(&0u64.to_le_bytes()); // no metadata
    f.extend_from_slice(b"fmt ");
    f.extend_from_slice(&52u64.to_le_bytes());
    for v in [1u32, 0, 2, 2, rate, 1] { f.extend_from_slice(&v.to_le_bytes()); } // version, id, ch type, ch num, fs, bps
    f.extend_from_slice(&sample_count.to_le_bytes());
    f.extend_from_slice(&(BLOCK as u32).to_le_bytes());
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(b"data");
    f.extend_from_slice(&(12 + data.len() as u64).to_le_bytes());
    f.extend_from_slice(&data);
    f
}

/// DSDIFF: big-endian, byte-interleaved channels, MSB-first.
fn dff(rate: u32, l: &[u8], r: &[u8]) -> Vec<u8> {
    let mut snd = Vec::new();
    let chunk = |id: &[u8; 4], body: &[u8]| {
        let mut c = id.to_vec();
        c.extend_from_slice(&(body.len() as u64).to_be_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 { c.push(0); }
        c
    };
    snd.extend_from_slice(b"SND ");
    snd.extend(chunk(b"FS  ", &rate.to_be_bytes()));
    let mut chnl = 2u16.to_be_bytes().to_vec();
    chnl.extend_from_slice(b"SLFTSRGT");
    snd.extend(chunk(b"CHNL", &chnl));
    let mut cmpr = b"DSD ".to_vec();
    cmpr.push(14);
    cmpr.extend_from_slice(b"not compressed");
    snd.extend(chunk(b"CMPR", &cmpr));
    let mut data = Vec::with_capacity(l.len() * 2);
    for i in 0..l.len() { data.push(l[i]); data.push(r[i]); }
    let mut body = b"DSD ".to_vec();
    body.extend(chunk(b"FVER", &0x0105_0000u32.to_be_bytes()));
    body.extend(chunk(b"PROP", &snd));
    body.extend(chunk(b"DSD ", &data));
    chunk(b"FRM8", &body)
}

fn corrupt_mp3(out: &Path) -> Result<()> {
    let src = out.join("mp3_320.mp3");
    let mut bytes = fs::read(&src).context("run the ffmpeg part of generate.sh first")?;
    let mid = bytes.len() / 2;
    for b in &mut bytes[mid..mid + 2048] { *b = 0x55; }
    fs::write(out.join("mp3_corrupt.mp3"), bytes)?;
    Ok(())
}
```

- [ ] **Step 4: generate.sh**

`fixtures/generate.sh` (run from the repo root with `bash fixtures/generate.sh`; works in Git Bash on Windows):
```bash
#!/usr/bin/env bash
# Generates synthetic test audio into fixtures/out (git-ignored). Needs ffmpeg and cargo.
set -euo pipefail
cd "$(dirname "$0")"
OUT=out
mkdir -p "$OUT"
FF="ffmpeg -hide_banner -loglevel error -y"

# Reference signals: L = 440 Hz, R = 1 kHz, sine amplitude 1/8, 2 s.
tone() { # rate dur out
  $FF -f lavfi -i "sine=f=440:d=$2:sample_rate=$1" -f lavfi -i "sine=f=1000:d=$2:sample_rate=$1" \
      -filter_complex "[0][1]amerge=inputs=2" "${@:3}"
}
tone 44100 2 -c:a pcm_s16le "$OUT/pcm16_44k.wav"
tone 96000 2 -c:a pcm_s24le "$OUT/pcm24_96k.wav"

# Lossless encodes + raw left-justified s32 references.
for src in pcm16_44k pcm24_96k; do
  $FF -i "$OUT/$src.wav" -c:a flac "$OUT/${src/pcm/flac}.flac"
  $FF -i "$OUT/$src.wav" -c:a alac "$OUT/${src/pcm/alac}.m4a"
  $FF -i "$OUT/$src.wav" -f s32le "$OUT/$src.s32le"
done
$FF -i "$OUT/pcm16_44k.wav" -c:a pcm_s16be "$OUT/aiff16_44k.aiff"
$FF -i "$OUT/pcm16_44k.wav" -f f32le "$OUT/pcm16_44k.f32le"

# Lossy encodes.
$FF -i "$OUT/pcm16_44k.wav" -c:a libmp3lame -b:a 320k "$OUT/mp3_320.mp3"
$FF -i "$OUT/pcm16_44k.wav" -c:a libmp3lame -q:a 0 "$OUT/mp3_v0.mp3"
$FF -i "$OUT/pcm16_44k.wav" -c:a aac -b:a 256k "$OUT/aac_lc_256.m4a"
$FF -i "$OUT/pcm16_44k.wav" -c:a libvorbis -q:a 6 "$OUT/vorbis_q6.ogg"
$FF -i "$OUT/pcm16_44k.wav" -c:a libopus -b:a 160k "$OUT/opus_160.opus"

# Gapless pairs: 6 s continuous tone split at a non-frame-aligned sample (110583).
tone 44100 6 -c:a pcm_s16le "$OUT/gap_src.wav"
$FF -i "$OUT/gap_src.wav" -f f32le "$OUT/gap_src.f32le"
$FF -i "$OUT/gap_src.wav" -af "atrim=end_sample=110583" "$OUT/gap_a.wav"
$FF -i "$OUT/gap_src.wav" -af "atrim=start_sample=110583,asetpts=PTS-STARTPTS" "$OUT/gap_b.wav"
for p in a b; do
  $FF -i "$OUT/gap_$p.wav" -c:a flac "$OUT/gap_$p.flac"
  $FF -i "$OUT/gap_$p.wav" -c:a libmp3lame -b:a 320k "$OUT/gap_$p.mp3"
  $FF -i "$OUT/gap_$p.wav" -c:a aac -b:a 256k "$OUT/gap_$p.m4a"
  $FF -i "$OUT/gap_$p.wav" -c:a libvorbis -q:a 6 "$OUT/gap_$p.ogg"
  $FF -i "$OUT/gap_$p.wav" -c:a libopus -b:a 160k "$OUT/gap_$p.opus"
done

# Channel layouts, short and empty files.
$FF -f lavfi -i "sine=f=440:d=1:sample_rate=44100" -c:a flac "$OUT/mono16_44k.flac"
$FF -f lavfi -i "sine=f=440:d=1:sample_rate=48000" -af "pan=5.1|FL=c0|FR=c0|FC=c0|LFE=c0|BL=c0|BR=c0" -c:a flac "$OUT/surround51_48k.flac"
$FF -f lavfi -i "sine=f=440:d=0.5:sample_rate=44100" -ac 2 -c:a flac "$OUT/short_a.flac"
$FF -f lavfi -i "sine=f=440:d=0.5:sample_rate=44100" -ac 2 -c:a flac "$OUT/short_b.flac"
: > "$OUT/empty.mp3"

# HE-AAC, DSD and corrupt MP3 from fixtures-gen, then remux HE-AAC into M4A.
cargo run --quiet --manifest-path ../engine/Cargo.toml -p fixtures-gen -- "$PWD/$OUT"
$FF -i "$OUT/he_aac.aac" -c copy "$OUT/he_aac.m4a"
$FF -i "$OUT/he_aac_v2.aac" -c copy "$OUT/he_aac_v2.m4a"
echo "fixtures: $(ls "$OUT" | wc -l) files in fixtures/$OUT"
```
Add `fixtures/out/` to `.gitignore` (already present).

- [ ] **Step 5: Test helpers**

`engine/crates/engine-core/tests/common/mod.rs`:
```rust
#![allow(dead_code)]
use std::path::PathBuf;

pub fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/out").join(name);
    assert!(p.exists(), "missing fixture {name}: run `bash fixtures/generate.sh` from the repo root");
    p
}

pub fn read_s32le(name: &str) -> Vec<i32> {
    std::fs::read(fixture(name)).unwrap().chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
}

pub fn read_f32le(name: &str) -> Vec<f32> {
    std::fs::read(fixture(name)).unwrap().chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

/// Frequency of the strongest bin (Goertzel scan, 10 Hz steps 20 Hz–20 kHz) of one channel.
pub fn dominant_freq(samples: &[f32], channels: usize, ch: usize, rate: u32) -> f64 {
    let x: Vec<f64> = samples.iter().skip(ch).step_by(channels).take(rate as usize).map(|&s| s as f64).collect();
    let mut best = (0.0, 0.0);
    let mut f = 20.0;
    while f < 20_000.0_f64.min(rate as f64 / 2.0) {
        let w = 2.0 * std::f64::consts::PI * f / rate as f64;
        let (mut s1, mut s2) = (0.0, 0.0);
        for &v in &x { let s0 = v + 2.0 * w.cos() * s1 - s2; s2 = s1; s1 = s0; }
        let p = s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2;
        if p > best.1 { best = (f, p); }
        f += 10.0;
    }
    best.0
}

pub fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len().max(1) as f64).sqrt()
}

/// Signal-to-noise ratio of `test` against `reference` over their common length, in dB.
pub fn snr_db(reference: &[f32], test: &[f32]) -> f64 {
    let n = reference.len().min(test.len());
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for i in 0..n {
        sig += (reference[i] as f64).powi(2);
        err += (reference[i] as f64 - test[i] as f64).powi(2);
    }
    10.0 * (sig / err.max(1e-20)).log10()
}

/// Interleaved f32 → one channel.
pub fn channel(samples: &[f32], channels: usize, ch: usize) -> Vec<f32> {
    samples.iter().skip(ch).step_by(channels).copied().collect()
}
```

- [ ] **Step 6: Generate and check**

Run: `bash fixtures/generate.sh`
Expected: ends with `fixtures: 4x files in fixtures/out` and creates `he_aac.m4a`, `dsd64.dsf`, `dsd128.dsf`, `dsd64.dff` and `mp3_corrupt.mp3`. Then run `ffprobe -hide_banner fixtures/out/he_aac.m4a` and confirm the stream is `aac (HE-AAC)`.

- [ ] **Step 7: Commit**

```bash
git add fixtures/generate.sh engine/crates/fixtures-gen engine/crates/engine-core
git commit -m "test: synthetic fixture generator (ffmpeg + fdk/DSD fixtures-gen)"
```

---

### Task 3: PCM types, decoder trait and the Trimmer

**Files:**
- Modify: `engine/crates/engine-core/src/pcm.rs`, `src/decode/mod.rs`
- Create: `engine/crates/engine-core/src/decode/trim.rs`

**Interfaces:**
- Produces (`engine_core::pcm`):
```rust
pub enum SampleKind { Int { bits: u16 }, Float, Dop }
pub struct PcmSpec { pub sample_rate: u32, pub channels: u16, pub kind: SampleKind }
pub enum PcmData { Int(Vec<i32>), Float(Vec<f32>) }       // interleaved; Int is LEFT-JUSTIFIED i32
pub struct PcmBlock { pub spec: PcmSpec, pub data: PcmData }
impl PcmBlock { pub fn frames(&self) -> usize; pub fn is_empty(&self) -> bool; pub fn trim(&mut self, start: usize, end: usize); pub fn to_f64(&self, out: &mut Vec<f64>); }
```
- Produces (`engine_core::decode`):
```rust
pub trait TrackDecoder: Send {
    fn info(&self) -> &SourceInfo;
    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError>;
    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError>;      // sample-accurate; returns frame now at
    fn set_dsd_output(&mut self, _out: DsdOutput) -> bool { false }  // true if switched
}
pub struct SourceInfo { pub container: String, pub codec: protocol::Codec, pub bitrate_kbps: Option<u32>, pub spec: PcmSpec, pub total_frames: Option<u64>, pub gapless: Option<GaplessInfo>, pub decoder_name: &'static str, pub dsd_rate: Option<u32> }
pub struct GaplessInfo { pub delay: u32, pub padding: u32 }
pub enum DsdOutput { Pcm, Dop }
pub enum DecodeError { Unsupported(String), CodecNotInBuild(&'static str), Io(std::io::Error), Corrupt(String) }
impl DecodeError { pub fn code(&self) -> protocol::ErrorCode }
```
- Produces (`engine_core::decode::trim`): `Trimmer::new(delay: u64, valid: Option<u64>)`, `Trimmer::apply(&mut self, block: &mut PcmBlock)` (empties the block when everything is trimmed), `Trimmer::seek_to(&mut self, target: u64, landed: u64)`.

- [ ] **Step 1: Write the failing tests**

Add at the bottom of `src/decode/trim.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcm::*;

    fn block(frames: usize) -> PcmBlock {
        PcmBlock {
            spec: PcmSpec { sample_rate: 44100, channels: 2, kind: SampleKind::Int { bits: 16 } },
            data: PcmData::Int((0..frames as i32 * 2).collect()),
        }
    }

    #[test]
    fn drops_delay_across_blocks_and_stops_at_valid_length() {
        let mut t = Trimmer::new(1500, Some(2000));
        let mut a = block(1024);
        t.apply(&mut a);
        assert_eq!(a.frames(), 0);
        let mut b = block(1024);
        t.apply(&mut b);
        assert_eq!(b.frames(), 548); // 2048 - 1500
        let PcmData::Int(v) = &b.data else { panic!() };
        assert_eq!(v[0], 476 * 2); // first kept frame is frame 476 of block b
        let mut c = block(4096);
        t.apply(&mut c);
        assert_eq!(c.frames(), 1452); // 2000 - 548
        let mut d = block(10);
        t.apply(&mut d);
        assert!(d.is_empty());
    }

    #[test]
    fn seek_resets_skip_and_remaining() {
        let mut t = Trimmer::new(100, Some(1000));
        t.seek_to(500, 480); // demuxer landed 20 frames early (in trimmed coordinates)
        let mut a = block(64);
        t.apply(&mut a);
        assert_eq!(a.frames(), 44);
        let mut b = block(1000);
        t.apply(&mut b);
        assert_eq!(b.frames(), 1000 - 480 - 44); // remaining valid frames after landing
    }

    #[test]
    fn pcm_trim_and_to_f64() {
        let mut b = block(4);
        b.trim(1, 1);
        assert_eq!(b.frames(), 2);
        let mut f = vec![];
        let fb = PcmBlock { spec: PcmSpec { sample_rate: 1, channels: 1, kind: SampleKind::Int { bits: 16 } }, data: PcmData::Int(vec![i32::MIN, 1 << 30]) };
        fb.to_f64(&mut f);
        assert_eq!(f, vec![-1.0, 0.5]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml trim`
Expected: compile error (`Trimmer` not defined).

- [ ] **Step 3: Implement**

`src/pcm.rs`:
```rust
//! Decoded audio. Integer samples are LEFT-JUSTIFIED in i32 (16-bit value v is stored as v << 16),
//! which makes the conversion to any wider device format lossless and reversible.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleKind {
    Int { bits: u16 },
    Float,
    /// DSD-over-PCM: 24-bit frames carrying DSD + markers. Must never be processed.
    Dop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmSpec { pub sample_rate: u32, pub channels: u16, pub kind: SampleKind }

#[derive(Clone, Debug, PartialEq)]
pub enum PcmData { Int(Vec<i32>), Float(Vec<f32>) }

#[derive(Clone, Debug, PartialEq)]
pub struct PcmBlock { pub spec: PcmSpec, pub data: PcmData }

impl PcmBlock {
    pub fn frames(&self) -> usize {
        let n = match &self.data { PcmData::Int(v) => v.len(), PcmData::Float(v) => v.len() };
        n / self.spec.channels.max(1) as usize
    }

    pub fn is_empty(&self) -> bool { self.frames() == 0 }

    /// Removes `start` frames from the front and `end` frames from the back (saturating).
    pub fn trim(&mut self, start: usize, end: usize) {
        let ch = self.spec.channels as usize;
        let frames = self.frames();
        let s = start.min(frames);
        let e = frames.saturating_sub(end).max(s);
        match &mut self.data {
            PcmData::Int(v) => { v.truncate(e * ch); v.drain(..s * ch); }
            PcmData::Float(v) => { v.truncate(e * ch); v.drain(..s * ch); }
        }
    }

    /// Appends samples converted to f64 in [-1, 1).
    pub fn to_f64(&self, out: &mut Vec<f64>) {
        match &self.data {
            PcmData::Int(v) => out.extend(v.iter().map(|&s| s as f64 / 2_147_483_648.0)),
            PcmData::Float(v) => out.extend(v.iter().map(|&s| s as f64)),
        }
    }
}
```

`src/decode/trim.rs` (above the tests):
```rust
use crate::pcm::PcmBlock;

/// Track-level gapless trimming for containers whose demuxer doesn't trim (MP4).
/// Coordinates are output frames from the start of the decoded stream.
pub struct Trimmer {
    skip: u64,
    remaining: Option<u64>,
    delay: u64,
    valid: Option<u64>,
}

impl Trimmer {
    pub fn new(delay: u64, valid: Option<u64>) -> Self {
        Self { skip: delay, remaining: valid, delay, valid }
    }

    pub fn apply(&mut self, block: &mut PcmBlock) {
        let frames = block.frames() as u64;
        let front = self.skip.min(frames);
        self.skip -= front;
        let mut keep = frames - front;
        if let Some(rem) = self.remaining.as_mut() {
            keep = keep.min(*rem);
            *rem -= keep;
        }
        let back = frames - front - keep;
        block.trim(front as usize, back as usize);
    }

    /// `target` and `landed` are positions in trimmed (user-visible) frames. The decoder has been
    /// repositioned so the next decoded frame is `landed + delay` in raw coordinates.
    pub fn seek_to(&mut self, target: u64, landed: u64) {
        self.skip = target.saturating_sub(landed);
        self.remaining = self.valid.map(|v| v.saturating_sub(landed));
        let _ = self.delay;
    }
}
```

`src/decode/mod.rs`:
```rust
pub mod dsd;
pub mod trim;

use crate::pcm::{PcmBlock, PcmSpec};
use protocol::{Codec, ErrorCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaplessInfo { pub delay: u32, pub padding: u32 }

#[derive(Clone, Debug, PartialEq)]
pub struct SourceInfo {
    pub container: String,
    pub codec: Codec,
    pub bitrate_kbps: Option<u32>,
    pub spec: PcmSpec,
    pub total_frames: Option<u64>,
    pub gapless: Option<GaplessInfo>,
    pub decoder_name: &'static str,
    pub dsd_rate: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsdOutput { Pcm, Dop }

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("unsupported format: {0}")]
    Unsupported(String),
    #[error("{0} decoder not included in this build")]
    CodecNotInBuild(&'static str),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt stream: {0}")]
    Corrupt(String),
}

impl DecodeError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Unsupported(_) => ErrorCode::UnsupportedFormat,
            Self::CodecNotInBuild(_) => ErrorCode::CodecNotInBuild,
            Self::Io(e) if e.kind() == std::io::ErrorKind::NotFound => ErrorCode::FileMissing,
            Self::Io(_) | Self::Corrupt(_) => ErrorCode::DecodeFailed,
        }
    }
}

pub trait TrackDecoder: Send {
    fn info(&self) -> &SourceInfo;
    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError>;
    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError>;
    fn set_dsd_output(&mut self, _out: DsdOutput) -> bool { false }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml trim`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core/src
git commit -m "feat(core): PCM block types, TrackDecoder trait and gapless Trimmer"
```

---

### Task 4: MP4 gapless metadata (iTunSMPB + elst)

**Files:**
- Create: `engine/crates/engine-core/src/decode/mp4_gapless.rs`
- Modify: `src/decode/mod.rs` (add `pub mod mp4_gapless;`)

**Interfaces:**
- Produces:
```rust
pub enum Mp4Gapless {
    ITunSmpb { delay: u64, padding: u64, length: u64 },            // output sample frames
    EditList { media_time: u64, duration: u64, timescale: u32 },   // both in the media (mdhd) timescale
}
impl Mp4Gapless { pub fn to_frames(&self, out_rate: u32) -> (u64 /*delay*/, Option<u64> /*valid*/) }
pub fn parse_itunsmpb(s: &str) -> Option<Mp4Gapless>;
pub fn read_mp4_gapless<R: Read + Seek>(r: &mut R) -> std::io::Result<Option<Mp4Gapless>>;  // iTunSMPB wins over elst
```

- [ ] **Step 1: Write the failing tests**

At the bottom of `mp4_gapless.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn bx(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(typ);
        v.extend_from_slice(payload);
        v
    }
    fn full(version: u8, rest: &[u8]) -> Vec<u8> { let mut v = vec![version, 0, 0, 0]; v.extend_from_slice(rest); v }

    fn mvhd(ts: u32) -> Vec<u8> { let mut p = vec![0u8; 8]; p.extend(ts.to_be_bytes()); p.extend([0u8; 88]); bx(b"mvhd", &full(0, &p)) }
    fn mdhd(ts: u32) -> Vec<u8> { let mut p = vec![0u8; 8]; p.extend(ts.to_be_bytes()); p.extend([0u8; 8]); bx(b"mdhd", &full(0, &p)) }
    fn hdlr(t: &[u8; 4]) -> Vec<u8> { let mut p = vec![0u8; 4]; p.extend(t); p.extend([0u8; 13]); bx(b"hdlr", &full(0, &p)) }
    fn elst(dur: u32, media_time: i32) -> Vec<u8> {
        let mut p = 1u32.to_be_bytes().to_vec();
        p.extend(dur.to_be_bytes()); p.extend(media_time.to_be_bytes()); p.extend(0x0001_0000u32.to_be_bytes());
        bx(b"elst", &full(0, &p))
    }

    fn movie(udta: Option<Vec<u8>>) -> Vec<u8> {
        let mdia = bx(b"mdia", &[mdhd(44100), hdlr(b"soun")].concat());
        let edts = bx(b"edts", &elst(1000 * 2, 1024)); // movie ts 1000 → 2 s
        let trak = bx(b"trak", &[edts, mdia].concat());
        let mut moov = [mvhd(1000), trak].concat();
        if let Some(u) = udta { moov.extend(u); }
        [bx(b"ftyp", b"M4A \0\0\0\0"), bx(b"moov", &moov)].concat()
    }

    #[test]
    fn parses_itunsmpb_string() {
        let g = parse_itunsmpb(" 00000000 00000840 000001CA 00000000000D2C36 00000000").unwrap();
        assert_eq!(g, Mp4Gapless::ITunSmpb { delay: 0x840, padding: 0x1CA, length: 0xD2C36 });
        assert_eq!(g.to_frames(44100), (2112, Some(0xD2C36)));
    }

    #[test]
    fn reads_edit_list_and_converts_movie_to_media_timescale() {
        let g = read_mp4_gapless(&mut Cursor::new(movie(None))).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::EditList { media_time: 1024, duration: 88200, timescale: 44100 });
        assert_eq!(g.to_frames(44100), (1024, Some(88200)));
        assert_eq!(g.to_frames(88200), (2048, Some(176400))); // HE-AAC: SBR doubles the output rate
    }

    #[test]
    fn itunsmpb_takes_precedence() {
        let data = bx(b"data", &[0u8, 0, 0, 1, 0, 0, 0, 0].iter().copied().chain(b" 00000000 00000840 00000100 0000000000015888".iter().copied()).collect::<Vec<u8>>());
        let mean = bx(b"mean", &full(0, b"com.apple.iTunes"));
        let name = bx(b"name", &full(0, b"iTunSMPB"));
        let ilst = bx(b"ilst", &bx(b"----", &[mean, name, data].concat()));
        let meta = bx(b"meta", &full(0, &[hdlr(b"mdir"), ilst].concat()));
        let udta = bx(b"udta", &meta);
        let g = read_mp4_gapless(&mut Cursor::new(movie(Some(udta)))).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::ITunSmpb { delay: 0x840, padding: 0x100, length: 0x15888 });
    }

    #[test]
    fn no_moov_means_none() {
        assert_eq!(read_mp4_gapless(&mut Cursor::new(bx(b"ftyp", b"isom"))).unwrap(), None);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml mp4_gapless`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Gapless info for MP4/M4A. Symphonia 0.6's isomp4 demuxer doesn't apply edit lists or iTunSMPB.
use std::io::{self, Read, Seek, SeekFrom};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mp4Gapless {
    ITunSmpb { delay: u64, padding: u64, length: u64 },
    EditList { media_time: u64, duration: u64, timescale: u32 },
}

impl Mp4Gapless {
    pub fn to_frames(&self, out_rate: u32) -> (u64, Option<u64>) {
        match *self {
            Self::ITunSmpb { delay, length, .. } => (delay, (length > 0).then_some(length)),
            Self::EditList { media_time, duration, timescale } => {
                let conv = |v: u64| (v as u128 * out_rate as u128 / timescale.max(1) as u128) as u64;
                (conv(media_time), (duration > 0).then(|| conv(duration)))
            }
        }
    }
}

pub fn parse_itunsmpb(s: &str) -> Option<Mp4Gapless> {
    let f: Vec<&str> = s.split_whitespace().collect();
    if f.len() < 4 { return None; }
    Some(Mp4Gapless::ITunSmpb {
        delay: u64::from_str_radix(f[1], 16).ok()?,
        padding: u64::from_str_radix(f[2], 16).ok()?,
        length: u64::from_str_radix(f[3], 16).ok()?,
    })
}

struct BoxHdr { typ: [u8; 4], body_start: u64, end: u64 }

fn read_hdr<R: Read + Seek>(r: &mut R, limit: u64) -> io::Result<Option<BoxHdr>> {
    let start = r.stream_position()?;
    if start + 8 > limit { return Ok(None); }
    let mut h = [0u8; 8];
    if r.read_exact(&mut h).is_err() { return Ok(None); }
    let mut size = u32::from_be_bytes(h[0..4].try_into().unwrap()) as u64;
    let typ: [u8; 4] = h[4..8].try_into().unwrap();
    let mut body_start = start + 8;
    if size == 1 {
        let mut big = [0u8; 8];
        r.read_exact(&mut big)?;
        size = u64::from_be_bytes(big);
        body_start += 8;
    } else if size == 0 {
        size = limit - start;
    }
    if size < body_start - start { return Ok(None); }
    Ok(Some(BoxHdr { typ, body_start, end: (start + size).min(limit) }))
}

/// Calls `f` for every child box of the range [from, to).
fn children<R: Read + Seek>(r: &mut R, from: u64, to: u64, mut f: impl FnMut(&mut R, &BoxHdr) -> io::Result<()>) -> io::Result<()> {
    let mut pos = from;
    while pos + 8 <= to {
        r.seek(SeekFrom::Start(pos))?;
        let Some(h) = read_hdr(r, to)? else { break };
        f(r, &h)?;
        if h.end <= pos { break; }
        pos = h.end;
    }
    Ok(())
}

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> { let mut b = [0u8; 4]; r.read_exact(&mut b)?; Ok(u32::from_be_bytes(b)) }
fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> { let mut b = [0u8; 8]; r.read_exact(&mut b)?; Ok(u64::from_be_bytes(b)) }

/// Reads the timescale of mvhd/mdhd (both FullBoxes: v0 has 32-bit times, v1 64-bit).
fn read_timescale<R: Read + Seek>(r: &mut R, h: &BoxHdr) -> io::Result<u32> {
    r.seek(SeekFrom::Start(h.body_start))?;
    let version = read_u32(r)? >> 24;
    r.seek(SeekFrom::Current(if version == 1 { 16 } else { 8 }))?;
    read_u32(r)
}

pub fn read_mp4_gapless<R: Read + Seek>(r: &mut R) -> io::Result<Option<Mp4Gapless>> {
    let len = r.seek(SeekFrom::End(0))?;
    let mut moov = None;
    children(r, 0, len, |_, h| { if &h.typ == b"moov" { moov = Some((h.body_start, h.end)); } Ok(()) })?;
    let Some((ms, me)) = moov else { return Ok(None) };

    let mut movie_ts = 0u32;
    let mut smpb = None;
    let mut audio_elst: Option<(u64, u64, u32)> = None; // (media_time, movie_duration, media_ts)

    children(r, ms, me, |r, h| {
        match &h.typ {
            b"mvhd" => movie_ts = read_timescale(r, h)?,
            b"trak" => {
                let (mut is_audio, mut media_ts, mut edit) = (false, 0u32, None);
                children(r, h.body_start, h.end, |r, t| {
                    match &t.typ {
                        b"edts" => children(r, t.body_start, t.end, |r, e| {
                            if &e.typ == b"elst" { edit = read_first_elst(r, e)?; }
                            Ok(())
                        })?,
                        b"mdia" => children(r, t.body_start, t.end, |r, m| {
                            match &m.typ {
                                b"mdhd" => media_ts = read_timescale(r, m)?,
                                b"hdlr" => {
                                    r.seek(SeekFrom::Start(m.body_start + 8))?;
                                    let mut ty = [0u8; 4];
                                    r.read_exact(&mut ty)?;
                                    is_audio = &ty == b"soun";
                                }
                                _ => {}
                            }
                            Ok(())
                        })?,
                        _ => {}
                    }
                    Ok(())
                })?;
                if is_audio && audio_elst.is_none() {
                    if let Some((mt, dur)) = edit { audio_elst = Some((mt, dur, media_ts)); }
                }
            }
            b"udta" => smpb = find_itunsmpb(r, h)?,
            _ => {}
        }
        Ok(())
    })?;

    if let Some(g) = smpb { return Ok(Some(g)); }
    Ok(audio_elst.map(|(media_time, movie_dur, media_ts)| Mp4Gapless::EditList {
        media_time,
        duration: if movie_ts == 0 { 0 } else { (movie_dur as u128 * media_ts as u128 / movie_ts as u128) as u64 },
        timescale: media_ts,
    }))
}

/// First edit with media_time >= 0 → (media_time, segment_duration in movie timescale).
fn read_first_elst<R: Read + Seek>(r: &mut R, h: &BoxHdr) -> io::Result<Option<(u64, u64)>> {
    r.seek(SeekFrom::Start(h.body_start))?;
    let version = read_u32(r)? >> 24;
    let count = read_u32(r)?;
    for _ in 0..count {
        let (dur, mt) = if version == 1 { (read_u64(r)?, read_u64(r)? as i64) } else { (read_u32(r)? as u64, read_u32(r)? as i32 as i64) };
        read_u32(r)?; // media_rate
        if mt >= 0 { return Ok(Some((mt as u64, dur))); }
    }
    Ok(None)
}

/// udta → meta → ilst → "----" with name "iTunSMPB" → data.
fn find_itunsmpb<R: Read + Seek>(r: &mut R, udta: &BoxHdr) -> io::Result<Option<Mp4Gapless>> {
    let mut found = None;
    children(r, udta.body_start, udta.end, |r, m| {
        if &m.typ != b"meta" { return Ok(()); }
        // ISO-style meta is a FullBox (4 zero bytes); QuickTime-style isn't.
        r.seek(SeekFrom::Start(m.body_start))?;
        let skip = if read_u32(r)? == 0 { 4 } else { 0 };
        children(r, m.body_start + skip, m.end, |r, i| {
            if &i.typ != b"ilst" { return Ok(()); }
            children(r, i.body_start, i.end, |r, item| {
                if &item.typ != b"----" { return Ok(()); }
                let (mut name, mut value) = (String::new(), String::new());
                children(r, item.body_start, item.end, |r, c| {
                    let n = (c.end - c.body_start) as usize;
                    let mut buf = vec![0u8; n];
                    r.seek(SeekFrom::Start(c.body_start))?;
                    r.read_exact(&mut buf)?;
                    match &c.typ {
                        b"name" if n > 4 => name = String::from_utf8_lossy(&buf[4..]).into_owned(),
                        b"data" if n > 8 => value = String::from_utf8_lossy(&buf[8..]).into_owned(),
                        _ => {}
                    }
                    Ok(())
                })?;
                if name == "iTunSMPB" { found = parse_itunsmpb(&value); }
                Ok(())
            })
        })
    })?;
    Ok(found)
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml mp4_gapless`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core/src/decode
git commit -m "feat(decode): MP4 gapless parsing (iTunSMPB, elst)"
```

---
### Task 5: Symphonia decoder (MP3, AAC-LC, ALAC, FLAC, WAV, AIFF, Vorbis) with gapless

**Files:**
- Create: `engine/crates/engine-core/src/decode/symphonia_dec.rs`
- Modify: `src/decode/mod.rs` (add `pub mod symphonia_dec;` and `pub mod mp4_gapless;` if not already there)
- Test: `engine/crates/engine-core/tests/decode_lossless.rs`, `tests/decode_lossy.rs`, `tests/gapless.rs`

**Interfaces:**
- Consumes: `PcmBlock`, `PcmSpec`, `SampleKind` (Task 3); `Trimmer` (Task 3); `read_mp4_gapless`, `Mp4Gapless` (Task 4).
- Produces:
```rust
pub struct Probed {
    pub format: Box<dyn FormatReader>, pub track_id: u32, pub params: AudioCodecParameters,
    pub time_base: Option<TimeBase>, pub num_frames: Option<u64>, pub container: String,
    pub file_len: u64, pub mp4_gapless: Option<Mp4Gapless>,
}
pub fn probe_file(path: &Path) -> Result<Probed, DecodeError>;
pub(crate) fn frames_of(units: u64, tb: Option<TimeBase>, rate: u32) -> u64;   // timebase units → frames
pub(crate) fn units_of(frames: u64, tb: Option<TimeBase>, rate: u32) -> u64;   // frames → timebase units
pub(crate) fn map_sym_err(e: symphonia::core::errors::Error) -> DecodeError;
pub fn codec_of(id: AudioCodecId) -> Option<protocol::Codec>;
pub struct SymphoniaDecoder;  impl SymphoniaDecoder { pub fn new(p: Probed) -> Result<Self, DecodeError>; }  impl TrackDecoder
```

- [ ] **Step 1: Write the failing lossless tests**

`tests/decode_lossless.rs`:
```rust
mod common;
use common::*;
use engine_core::decode::{symphonia_dec::{probe_file, SymphoniaDecoder}, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn open(name: &str) -> SymphoniaDecoder { SymphoniaDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap() }

fn decode_all_int(dec: &mut dyn TrackDecoder) -> Vec<i32> {
    let mut out = vec![];
    while let Some(b) = dec.next_block().unwrap() {
        let PcmData::Int(v) = b.data else { panic!("expected integer samples") };
        out.extend(v);
    }
    out
}

#[test]
fn flac16_is_bit_exact() {
    let mut d = open("flac16_44k.flac");
    assert_eq!(d.info().spec.kind, SampleKind::Int { bits: 16 });
    assert_eq!(d.info().spec.sample_rate, 44100);
    assert_eq!(decode_all_int(&mut d), read_s32le("pcm16_44k.s32le"));
}

#[test]
fn flac24_96k_is_bit_exact() {
    let mut d = open("flac24_96k.flac");
    assert_eq!(d.info().spec.kind, SampleKind::Int { bits: 24 });
    assert_eq!(decode_all_int(&mut d), read_s32le("pcm24_96k.s32le"));
}

#[test]
fn alac_16_and_24_are_bit_exact() {
    assert_eq!(decode_all_int(&mut open("alac16_44k.m4a")), read_s32le("pcm16_44k.s32le"));
    assert_eq!(decode_all_int(&mut open("alac24_96k.m4a")), read_s32le("pcm24_96k.s32le"));
}

#[test]
fn wav_and_aiff_are_bit_exact() {
    assert_eq!(decode_all_int(&mut open("pcm24_96k.wav")), read_s32le("pcm24_96k.s32le"));
    assert_eq!(decode_all_int(&mut open("aiff16_44k.aiff")), read_s32le("pcm16_44k.s32le"));
}

#[test]
fn flac_seek_is_sample_accurate() {
    let reference = read_s32le("pcm16_44k.s32le");
    let mut d = open("flac16_44k.flac");
    assert_eq!(d.seek(44_100).unwrap(), 44_100);
    let got = decode_all_int(&mut d);
    assert_eq!(got, reference[44_100 * 2..]);
    assert_eq!(d.seek(0).unwrap(), 0);
    assert_eq!(decode_all_int(&mut d), reference);
}

#[test]
fn reports_source_info() {
    let d = open("flac24_96k.flac");
    let i = d.info();
    assert_eq!(i.container, "flac");
    assert_eq!(i.codec, protocol::Codec::Flac);
    assert_eq!(i.decoder_name, "symphonia");
    assert_eq!(i.total_frames, Some(192_000));
    assert!(i.bitrate_kbps.unwrap() > 500);
}
```

`tests/decode_lossy.rs`:
```rust
mod common;
use common::*;
use engine_core::decode::{symphonia_dec::{probe_file, SymphoniaDecoder}, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn decode_f32(name: &str) -> (u32, u16, Vec<f32>) {
    let mut d = SymphoniaDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap();
    assert_eq!(d.info().spec.kind, SampleKind::Float);
    let (rate, ch) = (d.info().spec.sample_rate, d.info().spec.channels);
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() {
        let PcmData::Float(v) = b.data else { panic!() };
        out.extend(v);
    }
    (rate, ch, out)
}

fn check(name: &str, min_snr: f64) {
    let reference = read_f32le("pcm16_44k.f32le");
    let (rate, ch, got) = decode_f32(name);
    assert_eq!((rate, ch), (44100, 2));
    assert_eq!(got.len(), reference.len(), "{name}: gapless trim must give the exact source length");
    let snr = snr_db(&reference, &got);
    assert!(snr >= min_snr, "{name}: SNR {snr:.1} dB < {min_snr}");
}

#[test] fn mp3_320_matches_source() { check("mp3_320.mp3", 30.0); }
#[test] fn mp3_v0_matches_source() { check("mp3_v0.mp3", 25.0); }
#[test] fn aac_lc_m4a_matches_source() { check("aac_lc_256.m4a", 25.0); }
#[test] fn vorbis_matches_source() { check("vorbis_q6.ogg", 25.0); }

#[test]
fn corrupt_mp3_decodes_past_damage() {
    let (_, _, got) = decode_f32("mp3_corrupt.mp3");
    let full = read_f32le("pcm16_44k.f32le").len();
    assert!(got.len() as f64 > full as f64 * 0.95, "stopped early: {} of {}", got.len(), full);
    // The final quarter after the damage is still correct audio.
    let tail_ref = &read_f32le("pcm16_44k.f32le")[full * 3 / 4..];
    let tail_got = &got[got.len() - tail_ref.len()..];
    assert!(snr_db(tail_ref, tail_got) > 20.0);
}
```

`tests/gapless.rs`:
```rust
mod common;
use common::*;
use engine_core::decode::{symphonia_dec::{probe_file, SymphoniaDecoder}, TrackDecoder};
use engine_core::pcm::PcmData;

const A_FRAMES: usize = 110_583;
const TOTAL_FRAMES: usize = 6 * 44_100;

fn decode_f64(name: &str) -> Vec<f32> {
    let mut d = SymphoniaDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap();
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() {
        match b.data {
            PcmData::Float(v) => out.extend(v),
            PcmData::Int(v) => out.extend(v.iter().map(|&s| s as f32 / 2_147_483_648.0)),
        }
    }
    out
}

fn check_pair(ext: &str, min_snr: f64) {
    let a = decode_f64(&format!("gap_a.{ext}"));
    let b = decode_f64(&format!("gap_b.{ext}"));
    assert_eq!(a.len() / 2, A_FRAMES, "{ext}: part A length");
    assert_eq!((a.len() + b.len()) / 2, TOTAL_FRAMES, "{ext}: total length");
    let joined: Vec<f32> = a.into_iter().chain(b).collect();
    let reference = read_f32le("gap_src.f32le");
    // Look at ±2048 frames around the join: inserted silence or offsets wreck the SNR here.
    let s = (A_FRAMES - 2048) * 2;
    let e = (A_FRAMES + 2048) * 2;
    let snr = snr_db(&reference[s..e], &joined[s..e]);
    assert!(snr >= min_snr, "{ext}: boundary SNR {snr:.1} dB");
}

#[test] fn flac_pair_is_seamless() { check_pair("flac", 90.0); }
#[test] fn mp3_pair_is_seamless() { check_pair("mp3", 25.0); }
#[test] fn aac_pair_is_seamless() { check_pair("m4a", 20.0); }
#[test] fn vorbis_pair_is_seamless() { check_pair("ogg", 20.0); }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test decode_lossless --test decode_lossy --test gapless`
Expected: compile errors (`symphonia_dec` not found).

- [ ] **Step 3: Implement `symphonia_dec.rs`**

```rust
//! Symphonia-backed decoding. Gapless: the Ogg/MP3/FLAC demuxers put trim info on each packet,
//! which we apply. ISO-MP4 doesn't, so we apply the iTunSMPB/elst info via `Trimmer`. If an MP4
//! packet ever carries trims (a future Symphonia), we stop using the Trimmer to avoid trimming twice.
use std::{fs::File, path::Path};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::well_known::*;
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{TimeBase, Timestamp};

use super::mp4_gapless::{read_mp4_gapless, Mp4Gapless};
use super::trim::Trimmer;
use super::{DecodeError, GaplessInfo, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};
use protocol::Codec;

pub struct Probed {
    pub format: Box<dyn FormatReader>,
    pub track_id: u32,
    pub params: AudioCodecParameters,
    pub time_base: Option<TimeBase>,
    pub num_frames: Option<u64>,
    pub container: String,
    pub file_len: u64,
    pub mp4_gapless: Option<Mp4Gapless>,
}

pub(crate) fn map_sym_err(e: SymErr) -> DecodeError {
    match e {
        SymErr::IoError(io) => DecodeError::Io(io),
        SymErr::Unsupported(m) => DecodeError::Unsupported(m.to_string()),
        other => DecodeError::Corrupt(other.to_string()),
    }
}

fn container_of(ext: &str) -> String {
    match ext {
        "aif" | "aiff" | "aifc" => "aiff",
        "m4a" | "m4b" | "mp4" | "m4p" | "alac" => "m4a",
        "oga" | "ogg" => "ogg",
        other => other,
    }
    .to_string()
}

pub fn probe_file(path: &Path) -> Result<Probed, DecodeError> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    if file_len == 0 {
        return Err(DecodeError::Unsupported("empty file".into()));
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let container = container_of(&ext);
    let mp4_gapless = if container == "m4a" { read_mp4_gapless(&mut File::open(path)?)? } else { None };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(&ext);
    let format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| match e {
            SymErr::IoError(io) => DecodeError::Io(io),
            other => DecodeError::Unsupported(other.to_string()),
        })?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .cloned()
        .ok_or_else(|| DecodeError::Unsupported("track has no audio codec parameters".into()))?;
    let (track_id, time_base, num_frames) = (track.id, track.time_base, track.num_frames);
    Ok(Probed { format, track_id, params, time_base, num_frames, container, file_len, mp4_gapless })
}

pub(crate) fn frames_of(units: u64, tb: Option<TimeBase>, rate: u32) -> u64 {
    match tb {
        Some(tb) => (units as u128 * tb.numer.get() as u128 * rate as u128 / tb.denom.get() as u128) as u64,
        None => units,
    }
}

pub(crate) fn units_of(frames: u64, tb: Option<TimeBase>, rate: u32) -> u64 {
    match tb {
        Some(tb) => (frames as u128 * tb.denom.get() as u128 / (tb.numer.get() as u128 * rate.max(1) as u128)) as u64,
        None => frames,
    }
}

pub fn codec_of(id: AudioCodecId) -> Option<Codec> {
    if id == CODEC_ID_MP3 { Some(Codec::Mp3) }
    else if id == CODEC_ID_AAC { Some(Codec::AacLc) }
    else if id == CODEC_ID_FLAC { Some(Codec::Flac) }
    else if id == CODEC_ID_ALAC { Some(Codec::Alac) }
    else if id == CODEC_ID_VORBIS { Some(Codec::Vorbis) }
    else if id == CODEC_ID_OPUS { Some(Codec::Opus) }
    else if (CODEC_ID_PCM_S32LE..=CODEC_ID_PCM_MULAW).contains(&id) { Some(Codec::Pcm) }
    else { None }
}

fn to_block(buf: &GenericAudioBufferRef<'_>, bits_hint: Option<u32>) -> PcmBlock {
    let sample_rate = buf.spec().rate();
    let channels = buf.spec().channels().count() as u16;
    match buf {
        GenericAudioBufferRef::F32(_) | GenericAudioBufferRef::F64(_) => {
            let mut v = Vec::new();
            buf.copy_to_vec_interleaved::<f32>(&mut v);
            PcmBlock { spec: PcmSpec { sample_rate, channels, kind: SampleKind::Float }, data: PcmData::Float(v) }
        }
        _ => {
            let intrinsic = match buf {
                GenericAudioBufferRef::U8(_) | GenericAudioBufferRef::S8(_) => 8,
                GenericAudioBufferRef::U16(_) | GenericAudioBufferRef::S16(_) => 16,
                GenericAudioBufferRef::U24(_) | GenericAudioBufferRef::S24(_) => 24,
                _ => 32,
            };
            let bits = bits_hint.map_or(intrinsic, |b| b.min(intrinsic)) as u16;
            let mut v = Vec::new();
            buf.copy_to_vec_interleaved::<i32>(&mut v); // Symphonia left-justifies when widening
            PcmBlock { spec: PcmSpec { sample_rate, channels, kind: SampleKind::Int { bits } }, data: PcmData::Int(v) }
        }
    }
}

pub struct SymphoniaDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    tb: Option<TimeBase>,
    rate: u32,
    bits_hint: Option<u32>,
    info: SourceInfo,
    trimmer: Option<Trimmer>,
    mp4_delay: u64,
    saw_packet_trim: bool,
    skip: u64,
    pending: Option<PcmBlock>,
    decode_errors: u32,
}

impl SymphoniaDecoder {
    pub fn new(p: Probed) -> Result<Self, DecodeError> {
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&p.params, &AudioDecoderOptions::default())
            .map_err(|e| DecodeError::Unsupported(e.to_string()))?;
        let codec = codec_of(p.params.codec).ok_or_else(|| DecodeError::Unsupported(format!("codec {:?}", p.params.codec)))?;
        let rate = p.params.sample_rate.unwrap_or(44_100);
        let (mp4_delay, mp4_valid) = p.mp4_gapless.map(|g| g.to_frames(rate)).unwrap_or((0, None));
        let trimmer = p.mp4_gapless.map(|_| Trimmer::new(mp4_delay, mp4_valid));
        let raw_frames = p.num_frames.map(|n| frames_of(n, p.time_base, rate));
        let total_frames = mp4_valid.or(raw_frames.map(|n| n.saturating_sub(mp4_delay)));
        let gapless = match p.mp4_gapless {
            Some(Mp4Gapless::ITunSmpb { delay, padding, .. }) => Some(GaplessInfo { delay: delay as u32, padding: padding as u32 }),
            Some(Mp4Gapless::EditList { .. }) => Some(GaplessInfo {
                delay: mp4_delay as u32,
                padding: raw_frames.zip(mp4_valid).map_or(0, |(r, v)| r.saturating_sub(v + mp4_delay)) as u32,
            }),
            None => None,
        };
        let bitrate_kbps = total_frames
            .filter(|&f| f > 0)
            .map(|f| (p.file_len as f64 * 8.0 / (f as f64 / rate as f64) / 1000.0).round() as u32);
        let placeholder = PcmSpec { sample_rate: rate, channels: 2, kind: SampleKind::Float };
        let mut me = Self {
            format: p.format,
            decoder,
            track_id: p.track_id,
            tb: p.time_base,
            rate,
            bits_hint: p.params.bits_per_sample,
            info: SourceInfo {
                container: p.container, codec, bitrate_kbps, spec: placeholder, total_frames, gapless,
                decoder_name: "symphonia", dsd_rate: None,
            },
            trimmer,
            mp4_delay,
            saw_packet_trim: false,
            skip: 0,
            pending: None,
            decode_errors: 0,
        };
        // Decode the first block now so `info().spec` reports the real output format.
        let first = me.decode_next()?.ok_or_else(|| DecodeError::Unsupported("no audio frames".into()))?;
        me.info.spec = first.spec;
        me.pending = Some(first);
        Ok(me)
    }

    fn decode_next(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(SymErr::ResetRequired) => { self.decoder.reset(); continue; }
                Err(e) => return Err(map_sym_err(e)),
            };
            if packet.track_id != self.track_id { continue; }
            let trim_start = frames_of(packet.trim_start.get(), self.tb, self.rate) as usize;
            let trim_end = frames_of(packet.trim_end.get(), self.tb, self.rate) as usize;
            if trim_start > 0 || trim_end > 0 { self.saw_packet_trim = true; }
            let mut block = match self.decoder.decode(&packet) {
                Ok(buf) => to_block(&buf, self.bits_hint),
                Err(SymErr::DecodeError(msg)) => {
                    self.decode_errors += 1;
                    tracing::warn!(error = msg, "skipping undecodable packet");
                    if self.decode_errors > 500 { return Err(DecodeError::Corrupt(msg.to_string())); }
                    continue;
                }
                Err(SymErr::IoError(_)) => continue,
                Err(e) => return Err(map_sym_err(e)),
            };
            block.trim(trim_start, trim_end);
            if !self.saw_packet_trim {
                if let Some(t) = self.trimmer.as_mut() { t.apply(&mut block); }
            }
            if self.skip > 0 {
                let n = (self.skip as usize).min(block.frames());
                block.trim(n, 0);
                self.skip -= n as u64;
            }
            if block.is_empty() { continue; }
            return Ok(Some(block));
        }
    }
}

impl TrackDecoder for SymphoniaDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        if let Some(b) = self.pending.take() { return Ok(Some(b)); }
        self.decode_next()
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        self.pending = None;
        let use_trimmer = self.trimmer.is_some() && !self.saw_packet_trim;
        let delay = if use_trimmer { self.mp4_delay } else { 0 };
        let ts = units_of(frame + delay, self.tb, self.rate);
        let seeked = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(ts as i64), track_id: self.track_id })
            .map_err(map_sym_err)?;
        self.decoder.reset();
        let landed_raw = frames_of(seeked.actual_ts.get().max(0) as u64, self.tb, self.rate);
        let landed = landed_raw.saturating_sub(delay).min(frame);
        if use_trimmer {
            if let Some(t) = self.trimmer.as_mut() { t.seek_to(frame, landed); }
            self.skip = 0;
        } else {
            self.skip = frame - landed;
        }
        Ok(frame)
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test decode_lossless --test decode_lossy --test gapless`
Expected: all PASS.
If a **length** assertion fails for MP3/M4A, print `d.info().gapless` and the first packet's `trim_start` before changing any thresholds. The source length is the contract, so fix the trimming rather than the test. If `flac16_is_bit_exact` fails with values off by a factor of 65536, Symphonia returned right-justified samples: shift left by `32 - bits` in `to_block` for integer buffers whose `bits_per_sample` is below the intrinsic width.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core
git commit -m "feat(decode): Symphonia decoder with packet + MP4 gapless trimming, accurate seek"
```

---

### Task 6: Opus decoder (libopus)

**Files:**
- Create: `engine/crates/engine-core/src/decode/opus_dec.rs`
- Modify: `src/decode/mod.rs` (add `pub mod opus_dec;`)
- Test: `engine/crates/engine-core/tests/opus.rs`

**Interfaces:**
- Consumes: `Probed`, `frames_of`, `units_of`, `map_sym_err` (Task 5).
- Produces: `OpusDecoder::new(p: Probed) -> Result<OpusDecoder, DecodeError>` implementing `TrackDecoder`. Output is `SampleKind::Float` at 48 kHz, `decoder_name = "libopus"`, `codec = Codec::Opus`. More than 2 channels → `DecodeError::Unsupported("Opus with more than 2 channels")`.

- [ ] **Step 1: Write the failing test**

`tests/opus.rs`:
```rust
mod common;
use common::*;
use engine_core::decode::{opus_dec::OpusDecoder, symphonia_dec::probe_file, TrackDecoder};
use engine_core::pcm::PcmData;

fn decode(name: &str) -> (OpusDecoder, Vec<f32>) {
    let mut d = OpusDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap();
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() { let PcmData::Float(v) = b.data else { panic!() }; out.extend(v); }
    (d, out)
}

#[test]
fn decodes_at_48k_with_preskip_removed() {
    let (d, out) = decode("opus_160.opus");
    assert_eq!(d.info().spec.sample_rate, 48_000);
    assert_eq!(d.info().decoder_name, "libopus");
    let frames = out.len() / 2;
    assert!((frames as i64 - 96_000).abs() <= 48, "2 s at 48 kHz ±1 ms, got {frames}");
    assert_eq!(dominant_freq(&out, 2, 0, 48_000), 440.0);
    assert_eq!(dominant_freq(&out, 2, 1, 48_000), 1000.0);
    // Pre-skip removed: the signal starts immediately (no ~6.5 ms of leading silence).
    assert!(rms(&channel(&out[..960 * 2], 2, 0)) > 0.02);
}

#[test]
fn seek_lands_on_requested_frame() {
    let (_, full) = decode("opus_160.opus");
    let mut d = OpusDecoder::new(probe_file(&fixture("opus_160.opus")).unwrap()).unwrap();
    d.seek(48_000).unwrap();
    let PcmData::Float(v) = d.next_block().unwrap().unwrap().data else { panic!() };
    let n = v.len().min(1920);
    assert!(snr_db(&full[96_000..96_000 + n], &v[..n]) > 15.0);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test opus`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
use opus::{Channels, Decoder};
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::{TimeBase, Timestamp};

use super::symphonia_dec::{frames_of, map_sym_err, units_of, Probed};
use super::{DecodeError, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};

const RATE: u32 = 48_000;
const MAX_FRAME: usize = 5_760; // 120 ms at 48 kHz
const PREROLL: u64 = 3_840;     // 80 ms decoder convergence after a seek (RFC 7845 §4.6)

pub struct OpusDecoder {
    format: Box<dyn FormatReader>,
    track_id: u32,
    tb: Option<TimeBase>,
    dec: Decoder,
    channels: usize,
    buf: Vec<f32>,
    skip: u64,
    info: SourceInfo,
}

impl OpusDecoder {
    pub fn new(p: Probed) -> Result<Self, DecodeError> {
        let channels = p.params.channels.as_ref().map_or(2, |c| c.count());
        let ch = match channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(DecodeError::Unsupported("Opus with more than 2 channels".into())),
        };
        let dec = Decoder::new(RATE, ch).map_err(|e| DecodeError::Corrupt(e.to_string()))?;
        let total_frames = p.num_frames.map(|n| frames_of(n, p.time_base, RATE));
        let bitrate_kbps = total_frames.filter(|&f| f > 0).map(|f| (p.file_len as f64 * 8.0 / (f as f64 / RATE as f64) / 1000.0).round() as u32);
        Ok(Self {
            format: p.format,
            track_id: p.track_id,
            tb: p.time_base,
            dec,
            channels,
            buf: vec![0.0; MAX_FRAME * channels],
            skip: 0,
            info: SourceInfo {
                container: p.container,
                codec: protocol::Codec::Opus,
                bitrate_kbps,
                spec: PcmSpec { sample_rate: RATE, channels: channels as u16, kind: SampleKind::Float },
                total_frames,
                gapless: None,
                decoder_name: "libopus",
                dsd_rate: None,
            },
        })
    }
}

impl TrackDecoder for OpusDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(e) => return Err(map_sym_err(e)),
            };
            if packet.track_id != self.track_id { continue; }
            let n = match self.dec.decode_float(&packet.data[..], &mut self.buf, false) {
                Ok(n) => n,
                Err(e) => { tracing::warn!(%e, "skipping bad Opus packet"); continue; }
            };
            let mut block = PcmBlock { spec: self.info.spec, data: PcmData::Float(self.buf[..n * self.channels].to_vec()) };
            block.trim(frames_of(packet.trim_start.get(), self.tb, RATE) as usize, frames_of(packet.trim_end.get(), self.tb, RATE) as usize);
            if self.skip > 0 {
                let k = (self.skip as usize).min(block.frames());
                block.trim(k, 0);
                self.skip -= k as u64;
            }
            if !block.is_empty() { return Ok(Some(block)); }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let target = frame.saturating_sub(PREROLL);
        let seeked = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(units_of(target, self.tb, RATE) as i64), track_id: self.track_id })
            .map_err(map_sym_err)?;
        self.dec.reset_state().map_err(|e| DecodeError::Corrupt(e.to_string()))?;
        let landed = frames_of(seeked.actual_ts.get().max(0) as u64, self.tb, RATE).min(frame);
        self.skip = frame - landed;
        Ok(frame)
    }
}
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test opus`
Expected: 2 PASS. (The first build compiles libopus through `opusic-sys`; if CMake isn't found, revisit Task 1 Step 1.)

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core
git commit -m "feat(decode): Ogg Opus decoding via libopus with pre-skip and seek pre-roll"
```

---

### Task 7: HE-AAC / HE-AACv2 via fdk-aac (feature-gated)

**Files:**
- Create: `engine/crates/engine-core/src/decode/aac_config.rs`, `src/decode/fdk_dec.rs`
- Modify: `src/decode/mod.rs` (add `pub mod aac_config;` and `#[cfg(feature = "fdk-aac")] pub mod fdk_dec;`)
- Test: `engine/crates/engine-core/tests/he_aac.rs`

**Interfaces:**
- Produces:
```rust
// aac_config.rs
pub struct AacConfig { pub object_type: u8, pub sample_rate: u32, pub channels: u8, pub sbr: bool, pub ps: bool, pub output_rate: u32 }
pub fn parse_asc(asc: &[u8]) -> Option<AacConfig>;
impl AacConfig { pub fn codec(&self) -> protocol::Codec }   // HeAacV2 if ps, HeAac if sbr, else AacLc
// fdk_dec.rs (feature "fdk-aac")
pub struct FdkAacDecoder;
impl FdkAacDecoder {
    pub fn new_mp4(p: Probed, cfg: AacConfig) -> Result<Self, DecodeError>;
    pub fn open_adts(path: &Path) -> Result<Self, DecodeError>;
}   // impl TrackDecoder; output Int{bits:16}; decoder_name "fdk-aac"
```
- **Routing rule (used in Task 9):** MP4 AAC whose ASC says SBR or PS goes to fdk. Raw ADTS `.aac` files always go to fdk when the feature is on, because ADTS can't signal SBR explicitly. Everything else uses Symphonia AAC-LC.

- [ ] **Step 1: Write the failing ASC unit tests**

At the bottom of `aac_config.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lc_44k_stereo() {
        let c = parse_asc(&[0x12, 0x10]).unwrap();
        assert_eq!((c.object_type, c.sample_rate, c.channels, c.sbr, c.ps, c.output_rate), (2, 44_100, 2, false, false, 44_100));
        assert_eq!(c.codec(), protocol::Codec::AacLc);
    }

    #[test]
    fn explicit_he_aac() {
        let c = parse_asc(&[0x2B, 0x11, 0x88, 0x00]).unwrap();
        assert!(c.sbr && !c.ps);
        assert_eq!((c.sample_rate, c.output_rate), (24_000, 48_000));
        assert_eq!(c.codec(), protocol::Codec::HeAac);
    }

    #[test]
    fn explicit_he_aac_v2() {
        let c = parse_asc(&[0xEB, 0x09, 0x88, 0x00]).unwrap();
        assert!(c.sbr && c.ps);
        assert_eq!(c.codec(), protocol::Codec::HeAacV2);
    }

    #[test]
    fn backward_compatible_sync_extension() {
        let c = parse_asc(&[0x13, 0x10, 0x56, 0xE5, 0x98]).unwrap();
        assert_eq!(c.object_type, 2);
        assert!(c.sbr);
        assert_eq!(c.output_rate, 48_000);
    }

    #[test]
    fn garbage_is_none() { assert!(parse_asc(&[]).is_none()); }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml aac_config`
Expected: compile error.

- [ ] **Step 3: Implement `aac_config.rs`**

```rust
//! MPEG-4 AudioSpecificConfig (ISO 14496-3 §1.6.2.1), just enough to detect SBR/PS.
use protocol::Codec;

const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AacConfig { pub object_type: u8, pub sample_rate: u32, pub channels: u8, pub sbr: bool, pub ps: bool, pub output_rate: u32 }

impl AacConfig {
    pub fn codec(&self) -> Codec {
        if self.ps { Codec::HeAacV2 } else if self.sbr { Codec::HeAac } else { Codec::AacLc }
    }
}

struct Bits<'a> { d: &'a [u8], pos: usize }
impl Bits<'_> {
    fn left(&self) -> usize { self.d.len() * 8 - self.pos }
    fn get(&mut self, n: usize) -> Option<u32> {
        if self.left() < n { return None; }
        let mut v = 0u32;
        for _ in 0..n {
            let bit = (self.d[self.pos / 8] >> (7 - self.pos % 8)) & 1;
            v = (v << 1) | bit as u32;
            self.pos += 1;
        }
        Some(v)
    }
    fn aot(&mut self) -> Option<u8> { let a = self.get(5)?; Some(if a == 31 { 32 + self.get(6)? as u8 } else { a as u8 }) }
    fn rate(&mut self) -> Option<u32> { let i = self.get(4)?; if i == 15 { self.get(24) } else { RATES.get(i as usize).copied() } }
}

pub fn parse_asc(asc: &[u8]) -> Option<AacConfig> {
    let mut b = Bits { d: asc, pos: 0 };
    let mut aot = b.aot()?;
    let sample_rate = b.rate()?;
    let channels = b.get(4)? as u8;
    let (mut sbr, mut ps, mut ext_rate) = (false, false, None);
    if aot == 5 || aot == 29 {
        sbr = true;
        ps = aot == 29;
        ext_rate = Some(b.rate()?);
        aot = b.aot()?;
    }
    // GASpecificConfig for the AAC object types we care about (1, 2, 3, 4, 6, 7, 17, 19..23).
    if channels != 0 && matches!(aot, 1..=4 | 6 | 7 | 17 | 19..=23) {
        let _frame_length = b.get(1)?;
        if b.get(1)? == 1 { b.get(14)?; } // dependsOnCoreCoder → coreCoderDelay
        let ext_flag = b.get(1)?;
        if aot == 6 || aot == 20 { b.get(3)?; }
        if ext_flag == 1 && matches!(aot, 17 | 19..=23) { b.get(3)?; }
        // Backward-compatible (implicit) signaling via sync extensions.
        if !sbr && b.left() >= 16 && b.get(11)? == 0x2B7 {
            if b.aot()? == 5 && b.get(1)? == 1 {
                sbr = true;
                ext_rate = Some(b.rate()?);
                if b.left() >= 12 && b.get(11)? == 0x548 { ps = b.get(1)? == 1; }
            }
        }
    }
    let output_rate = ext_rate.unwrap_or(if sbr { sample_rate * 2 } else { sample_rate });
    Some(AacConfig { object_type: aot, sample_rate, channels, sbr, ps, output_rate })
}
```
Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml aac_config` → 5 PASS.

- [ ] **Step 4: Write the failing fdk integration tests**

`tests/he_aac.rs`:
```rust
#![cfg(feature = "fdk-aac")]
mod common;
use common::*;
use engine_core::decode::{aac_config::parse_asc, fdk_dec::FdkAacDecoder, symphonia_dec::probe_file, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn drain(d: &mut dyn TrackDecoder) -> Vec<f32> {
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() {
        let PcmData::Int(v) = b.data else { panic!() };
        out.extend(v.iter().map(|&s| s as f32 / 2_147_483_648.0));
    }
    out
}

fn mp4(name: &str) -> FdkAacDecoder {
    let p = probe_file(&fixture(name)).unwrap();
    let cfg = parse_asc(p.params.extra_data.as_deref().unwrap()).unwrap();
    FdkAacDecoder::new_mp4(p, cfg).unwrap()
}

#[test]
fn he_aac_m4a_decodes_at_full_sbr_rate() {
    let mut d = mp4("he_aac.m4a");
    assert_eq!(d.info().codec, protocol::Codec::HeAac);
    assert_eq!(d.info().spec.sample_rate, 48_000);
    assert_eq!(d.info().spec.kind, SampleKind::Int { bits: 16 });
    assert_eq!(d.info().decoder_name, "fdk-aac");
    let out = drain(&mut d);
    assert!(out.len() / 2 >= 96_000);
    assert_eq!(dominant_freq(&out[9600..], 2, 0, 48_000), 1000.0);
    assert_eq!(dominant_freq(&out[9600..], 2, 1, 48_000), 440.0);
}

#[test]
fn he_aac_v2_m4a_outputs_stereo() {
    let mut d = mp4("he_aac_v2.m4a");
    assert_eq!(d.info().codec, protocol::Codec::HeAacV2);
    assert_eq!(d.info().spec.channels, 2);
    let out = drain(&mut d);
    assert!(rms(&channel(&out, 2, 0)) > 0.05);
}

#[test]
fn raw_adts_decodes_and_seeks() {
    let mut d = FdkAacDecoder::open_adts(&fixture("he_aac.aac")).unwrap();
    assert_eq!(d.info().spec.sample_rate, 48_000);
    let full = drain(&mut d);
    let mut d = FdkAacDecoder::open_adts(&fixture("he_aac.aac")).unwrap();
    d.seek(48_000).unwrap();
    let after = drain(&mut d);
    let diff = (full.len() as i64 / 2 - 48_000) - after.len() as i64 / 2;
    assert!(diff.abs() <= 2048, "seek landed {diff} frames off");
}
```

- [ ] **Step 5: Run to verify it fails**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test he_aac`
Expected: compile error (`fdk_dec` missing).

- [ ] **Step 6: Implement `fdk_dec.rs`**

```rust
//! HE-AAC (SBR/PS) via Fraunhofer FDK AAC. fdk outputs interleaved i16 at the full SBR rate.
use std::path::Path;

use fdk_aac::dec::{Decoder, DecoderError, Transport};
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::{TimeBase, Timestamp};

use super::aac_config::AacConfig;
use super::symphonia_dec::{frames_of, map_sym_err, units_of, Probed};
use super::trim::Trimmer;
use super::{DecodeError, GaplessInfo, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};

const PCM_CAP: usize = 2048 * 8;

enum Source {
    Mp4 { format: Box<dyn FormatReader>, track_id: u32, tb: Option<TimeBase>, core_rate: u32 },
    Adts { data: Vec<u8>, frames: Vec<(usize, usize)>, next: usize },
}

pub struct FdkAacDecoder {
    src: Source,
    dec: Decoder,
    asc: Option<Vec<u8>>,
    pcm: Vec<i16>,
    info: SourceInfo,
    trimmer: Option<Trimmer>,
    skip: u64,
    pending: Option<PcmBlock>,
}

fn fdk_err(e: DecoderError) -> DecodeError { DecodeError::Corrupt(e.message().to_string()) }

/// Byte ranges of every ADTS frame (sync 0xFFF, 13-bit frame_length).
fn scan_adts(data: &[u8]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut i = 0;
    while i + 7 <= data.len() {
        if data[i] == 0xFF && data[i + 1] & 0xF0 == 0xF0 {
            let len = (((data[i + 3] & 0x03) as usize) << 11) | ((data[i + 4] as usize) << 3) | ((data[i + 5] as usize) >> 5);
            if len >= 7 && i + len <= data.len() { out.push((i, len)); i += len; continue; }
        }
        i += 1;
    }
    out
}

impl FdkAacDecoder {
    pub fn new_mp4(p: Probed, cfg: AacConfig) -> Result<Self, DecodeError> {
        let asc = p.params.extra_data.as_deref().map(<[u8]>::to_vec).ok_or_else(|| DecodeError::Unsupported("MP4 AAC without AudioSpecificConfig".into()))?;
        let mut dec = Decoder::new(Transport::Raw);
        dec.config_raw(&asc).map_err(fdk_err)?;
        let core_rate = cfg.sample_rate;
        let (delay, valid) = p.mp4_gapless.map(|g| g.to_frames(cfg.output_rate)).unwrap_or((0, None));
        let trimmer = p.mp4_gapless.map(|_| Trimmer::new(delay, valid));
        let raw_frames = p.num_frames.map(|n| frames_of(n, p.time_base, core_rate) * (cfg.output_rate / core_rate.max(1)) as u64);
        let total_frames = valid.or(raw_frames.map(|n| n.saturating_sub(delay)));
        let bitrate_kbps = total_frames.filter(|&f| f > 0).map(|f| (p.file_len as f64 * 8.0 / (f as f64 / cfg.output_rate as f64) / 1000.0).round() as u32);
        let mut me = Self {
            src: Source::Mp4 { format: p.format, track_id: p.track_id, tb: p.time_base, core_rate },
            dec,
            asc: Some(asc),
            pcm: vec![0; PCM_CAP],
            info: SourceInfo {
                container: p.container, codec: cfg.codec(), bitrate_kbps,
                spec: PcmSpec { sample_rate: cfg.output_rate, channels: if cfg.ps { 2 } else { cfg.channels.max(1) as u16 }, kind: SampleKind::Int { bits: 16 } },
                total_frames, gapless: (delay > 0).then_some(GaplessInfo { delay: delay as u32, padding: 0 }),
                decoder_name: "fdk-aac", dsd_rate: None,
            },
            trimmer,
            skip: 0,
            pending: None,
        };
        me.prime()?;
        Ok(me)
    }

    pub fn open_adts(path: &Path) -> Result<Self, DecodeError> {
        let data = std::fs::read(path)?;
        let frames = scan_adts(&data);
        if frames.is_empty() { return Err(DecodeError::Unsupported("no ADTS frames".into())); }
        let file_len = data.len() as u64;
        let n = frames.len() as u64;
        let mut me = Self {
            src: Source::Adts { data, frames, next: 0 },
            dec: Decoder::new(Transport::Adts),
            asc: None,
            pcm: vec![0; PCM_CAP],
            info: SourceInfo {
                container: "aac".into(), codec: protocol::Codec::AacLc, bitrate_kbps: None,
                spec: PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Int { bits: 16 } },
                total_frames: None, gapless: None, decoder_name: "fdk-aac", dsd_rate: None,
            },
            trimmer: None,
            skip: 0,
            pending: None,
        };
        me.prime()?;
        // Each ADTS frame decodes to stream_info.frameSize output frames (1024 LC, 2048 with SBR).
        let si = me.dec.stream_info();
        let per = si.frameSize.max(1) as u64;
        me.info.codec = match si.aot as i32 { 29 => protocol::Codec::HeAacV2, 5 => protocol::Codec::HeAac, _ => if per == 2048 { protocol::Codec::HeAac } else { protocol::Codec::AacLc } };
        me.info.total_frames = Some(n * per);
        me.info.bitrate_kbps = Some((file_len as f64 * 8.0 / ((n * per) as f64 / me.info.spec.sample_rate as f64) / 1000.0).round() as u32);
        Ok(me)
    }

    /// Decodes the first block so `info().spec` holds fdk's real output rate and channel count.
    fn prime(&mut self) -> Result<(), DecodeError> {
        let first = self.decode_next()?.ok_or_else(|| DecodeError::Unsupported("no AAC frames".into()))?;
        self.info.spec = first.spec;
        self.pending = Some(first);
        Ok(())
    }

    fn decode_frame(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        match self.dec.decode_frame(&mut self.pcm) {
            Ok(()) => {
                let si = self.dec.stream_info();
                let (ch, rate, n) = (si.numChannels.max(1) as u16, si.sampleRate as u32, self.dec.decoded_frame_size());
                let data = self.pcm[..n].iter().map(|&s| (s as i32) << 16).collect();
                Ok(Some(PcmBlock { spec: PcmSpec { sample_rate: rate, channels: ch, kind: SampleKind::Int { bits: 16 } }, data: PcmData::Int(data) }))
            }
            Err(e) if e == DecoderError::NOT_ENOUGH_BITS => Ok(None),
            Err(e) => { tracing::warn!(err = e.message(), "fdk frame error"); Ok(None) }
        }
    }

    fn decode_next(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            let input: Vec<u8> = match &mut self.src {
                Source::Mp4 { format, track_id, .. } => match format.next_packet() {
                    Ok(Some(p)) if p.track_id == *track_id => p.data.to_vec(),
                    Ok(Some(_)) => continue,
                    Ok(None) => return Ok(None),
                    Err(e) => return Err(map_sym_err(e)),
                },
                Source::Adts { data, frames, next } => {
                    let Some(&(off, len)) = frames.get(*next) else { return Ok(None) };
                    *next += 1;
                    data[off..off + len].to_vec()
                }
            };
            self.dec.fill(&input).map_err(fdk_err)?;
            let Some(mut block) = self.decode_frame()? else { continue };
            if let Some(t) = self.trimmer.as_mut() { t.apply(&mut block); }
            if self.skip > 0 {
                let k = (self.skip as usize).min(block.frames());
                block.trim(k, 0);
                self.skip -= k as u64;
            }
            if !block.is_empty() { return Ok(Some(block)); }
        }
    }

    fn reset_decoder(&mut self) -> Result<(), DecodeError> {
        self.dec = match &self.asc {
            Some(asc) => { let mut d = Decoder::new(Transport::Raw); d.config_raw(asc).map_err(fdk_err)?; d }
            None => Decoder::new(Transport::Adts),
        };
        Ok(())
    }
}

impl TrackDecoder for FdkAacDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        if let Some(b) = self.pending.take() { return Ok(Some(b)); }
        self.decode_next()
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        self.pending = None;
        let out_rate = self.info.spec.sample_rate;
        let delay = self.info.gapless.map_or(0, |g| g.delay as u64);
        // fdk's output frame count per input frame: 2048 with SBR, 1024 without.
        let per = self.dec.stream_info().frameSize.max(1) as u64;
        self.reset_decoder()?;
        match &mut self.src {
            Source::Mp4 { format, track_id, tb, core_rate } => {
                let ratio = (out_rate / (*core_rate).max(1)).max(1) as u64;
                let raw_out = frame + delay;
                let ts = units_of(raw_out / ratio, *tb, *core_rate);
                let s = format.seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(ts as i64), track_id: *track_id }).map_err(map_sym_err)?;
                let landed = (frames_of(s.actual_ts.get().max(0) as u64, *tb, *core_rate) * ratio).saturating_sub(delay).min(frame);
                if let Some(t) = self.trimmer.as_mut() { t.seek_to(frame, landed); self.skip = 0; } else { self.skip = frame - landed; }
            }
            Source::Adts { next, frames, .. } => {
                let idx = ((frame / per) as usize).min(frames.len());
                *next = idx;
                self.skip = frame - idx as u64 * per;
            }
        }
        Ok(frame)
    }
}
```

- [ ] **Step 7: Run all HE-AAC tests and check the no-fdk build compiles**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test he_aac aac_config`
Expected: all PASS.
Run: `cargo check -p engine-core --manifest-path engine/Cargo.toml --no-default-features`
Expected: compiles (`he_aac.rs` is cfg'd out).

- [ ] **Step 8: Commit**

```bash
git add engine/crates/engine-core
git commit -m "feat(decode): HE-AAC/HE-AACv2 via fdk-aac (MP4 + ADTS) behind fdk-aac feature"
```

---

### Task 8: DSD — DSF/DFF readers, DSD→PCM decimation and DoP

**Files:**
- Create: `engine/crates/engine-core/src/decode/dsd/dsf.rs`, `dff.rs`, `decimate.rs`, `dop.rs`
- Modify: `engine/crates/engine-core/src/decode/dsd/mod.rs`
- Test: `engine/crates/engine-core/tests/dsd.rs` (+ unit tests in `decimate.rs`, `dop.rs`)

**Interfaces:**
- Consumes: `TrackDecoder`, `SourceInfo`, `DsdOutput` (Task 3); `sdm2` (Task 2, tests only).
- Produces:
```rust
pub trait DsdSource: Send {
    fn channels(&self) -> u16;
    fn dsd_rate(&self) -> u32;
    fn total_bytes_per_channel(&self) -> u64;
    /// Fills one Vec per channel with up to `max` bytes, MSB-first. Returns bytes per channel (0 = EOF).
    fn read(&mut self, out: &mut [Vec<u8>], max: usize) -> std::io::Result<usize>;
    fn seek_byte(&mut self, byte_per_channel: u64) -> std::io::Result<()>;
}
pub fn dsf::open(path) -> Result<Box<dyn DsdSource>, DecodeError>;  pub fn dff::open(path) -> Result<Box<dyn DsdSource>, DecodeError>;
pub struct Decimator; impl Decimator { pub fn new(channels: usize, dsd_rate: u32) -> Self; pub fn output_rate(&self) -> u32; pub fn process(&mut self, input: &[Vec<u8>], out: &mut Vec<f32>); pub fn reset(&mut self); }
pub struct DopPacker; impl DopPacker { pub fn new() -> Self; pub fn pack(&mut self, input: &[Vec<u8>], out: &mut Vec<i32>); }
pub struct DsdDecoder; impl DsdDecoder { pub fn open(path: &Path) -> Result<Self, DecodeError>; }  // impl TrackDecoder, set_dsd_output switches PCM↔DoP
```
- Rates: the PCM path gives 88.2 kHz for DSD64 and 176.4 kHz for DSD128 and above. DoP rate = `dsd_rate / 16`. Gain = ×0.5 (−6 dB, spec §3.3).

- [ ] **Step 1: Write the failing unit tests**

Bottom of `decimate.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::dsd::modulator::sdm2;

    fn tone(rate: u32, f: f64, secs: f64) -> Vec<u8> {
        let n = (rate as f64 * secs) as usize / 8 * 8;
        sdm2(&(0..n).map(|i| 0.5 * (2.0 * std::f64::consts::PI * f * i as f64 / rate as f64).sin()).collect::<Vec<_>>())
    }

    #[test]
    fn rates() {
        assert_eq!(Decimator::new(2, 2_822_400).output_rate(), 88_200);
        assert_eq!(Decimator::new(2, 5_644_800).output_rate(), 176_400);
        assert_eq!(Decimator::new(2, 11_289_600).output_rate(), 176_400);
    }

    #[test]
    fn dsd64_tone_survives_at_minus_6db() {
        let mut d = Decimator::new(1, 2_822_400);
        let mut out = vec![];
        d.process(&[tone(2_822_400, 1000.0, 0.5)], &mut out);
        assert_eq!(out.len(), 44_100); // 0.5 s of mono at 88.2 kHz
        let tail = &out[4096..];
        let peak = tail.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!((peak - 0.25).abs() < 0.03, "peak {peak}"); // 0.5 modulation × 0.5 gain
        // Silence check on a ~DC-free signal: mean ≈ 0.
        assert!((tail.iter().map(|&s| s as f64).sum::<f64>() / tail.len() as f64).abs() < 0.01);
    }
}
```

Bottom of `dop.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_two_bytes_per_frame_with_alternating_markers() {
        let mut p = DopPacker::new();
        let mut out = vec![];
        p.pack(&[vec![0xA1, 0xA2, 0xA3, 0xA4], vec![0xB1, 0xB2, 0xB3, 0xB4]], &mut out);
        assert_eq!(out.len(), 4); // 2 frames × 2 channels
        let w = |v: i32| (v as u32) >> 8;
        assert_eq!(w(out[0]), 0x05A1A2);
        assert_eq!(w(out[1]), 0x05B1B2);
        assert_eq!(w(out[2]), 0xFAA3A4);
        assert_eq!(w(out[3]), 0xFAB3B4);
        let mut more = vec![];
        p.pack(&[vec![0x01, 0x02], vec![0x03, 0x04]], &mut more);
        assert_eq!(w(more[0]) >> 16, 0x05); // the marker keeps alternating across calls
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml dsd::`
Expected: compile error.

- [ ] **Step 3: Implement `decimate.rs` and `dop.rs`**

`decimate.rs` (above the tests):
```rust
//! DSD → PCM: a ÷8 FIR using per-byte lookup tables, then ÷2 half-band FIR stages, then ×0.5.

const STAGE1_TAPS: usize = 96; // 12 bytes of history
const HB_TAPS: usize = 63;

fn lowpass(taps: usize, cutoff: f64) -> Vec<f64> {
    // Windowed sinc (Blackman), `cutoff` in cycles/sample, unity DC gain.
    let m = (taps - 1) as f64;
    let mut h: Vec<f64> = (0..taps).map(|n| {
        let x = n as f64 - m / 2.0;
        let sinc = if x == 0.0 { 2.0 * cutoff } else { (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x) };
        let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / m).cos() + 0.08 * (4.0 * std::f64::consts::PI * n as f64 / m).cos();
        sinc * w
    }).collect();
    let sum: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= sum);
    h
}

struct Halfband { h: Vec<f64>, hist: Vec<f64>, pos: usize, phase: bool }
impl Halfband {
    fn new() -> Self { Self { h: lowpass(HB_TAPS, 0.25), hist: vec![0.0; HB_TAPS], pos: 0, phase: false } }
    fn push(&mut self, x: f64) -> Option<f64> {
        self.hist[self.pos] = x;
        self.pos = (self.pos + 1) % HB_TAPS;
        self.phase = !self.phase;
        if !self.phase { return None; }
        let mut acc = 0.0;
        for (k, &c) in self.h.iter().enumerate() {
            acc += c * self.hist[(self.pos + HB_TAPS - 1 - k) % HB_TAPS];
        }
        Some(acc)
    }
    fn reset(&mut self) { self.hist.iter_mut().for_each(|v| *v = 0.0); self.pos = 0; self.phase = false; }
}

pub struct Decimator {
    tables: Vec<[f64; 256]>,     // [byte position][byte value]; position 0 = newest byte
    hist: Vec<[u8; STAGE1_TAPS / 8]>,
    stages: Vec<Vec<Halfband>>,  // per channel
    out_rate: u32,
}

impl Decimator {
    pub fn new(channels: usize, dsd_rate: u32) -> Self {
        let stage1_rate = dsd_rate / 8;
        let target = if dsd_rate <= 2_822_400 { 88_200 } else { 176_400 };
        let n_half = (stage1_rate / target).trailing_zeros() as usize;
        // Stage-1 passband edge at half its output Nyquist; later stages finish the job.
        let h = lowpass(STAGE1_TAPS, 0.5 / 16.0);
        let mut tables = vec![[0.0f64; 256]; STAGE1_TAPS / 8];
        for (k, table) in tables.iter_mut().enumerate() {
            for (byte, slot) in table.iter_mut().enumerate() {
                // Bit j (0 = MSB, the oldest sample in the byte) of byte k uses tap 8k + 7 - j.
                *slot = (0..8).map(|j| { let bit = (byte >> (7 - j)) & 1; h[8 * k + 7 - j] * if bit == 1 { 1.0 } else { -1.0 } }).sum();
            }
        }
        Self {
            tables,
            hist: vec![[0x69; STAGE1_TAPS / 8]; channels], // 0x69 = DSD idle pattern (≈ silence)
            stages: (0..channels).map(|_| (0..n_half).map(|_| Halfband::new()).collect()).collect(),
            out_rate: stage1_rate >> n_half,
        }
    }

    pub fn output_rate(&self) -> u32 { self.out_rate }

    /// `input[ch]` holds MSB-first DSD bytes; all channels have the same length. Appends interleaved f32.
    pub fn process(&mut self, input: &[Vec<u8>], out: &mut Vec<f32>) {
        let chs = input.len();
        let len = input.first().map_or(0, Vec::len);
        let mut per_ch: Vec<Vec<f32>> = vec![Vec::with_capacity(len >> self.stages[0].len()); chs];
        for ch in 0..chs {
            for &byte in &input[ch] {
                let h = &mut self.hist[ch];
                h.copy_within(0..h.len() - 1, 1);
                h[0] = byte;
                let mut y: f64 = h.iter().enumerate().map(|(k, &b)| self.tables[k][b as usize]).sum();
                let mut emitted = true;
                for st in self.stages[ch].iter_mut() {
                    match st.push(y) { Some(v) => y = v, None => { emitted = false; break; } }
                }
                if emitted { per_ch[ch].push((y * 0.5) as f32); }
            }
        }
        let frames = per_ch.iter().map(Vec::len).min().unwrap_or(0);
        out.reserve(frames * chs);
        for i in 0..frames { for ch in &per_ch { out.push(ch[i]); } }
    }

    pub fn reset(&mut self) {
        self.hist.iter_mut().for_each(|h| *h = [0x69; STAGE1_TAPS / 8]);
        self.stages.iter_mut().flatten().for_each(Halfband::reset);
    }
}
```

`dop.rs` (above the tests):
```rust
//! DSD-over-PCM v1.1: each 24-bit sample = marker (0x05/0xFA alternating per frame) + 16 DSD bits.
pub struct DopPacker { marker_hi: bool }

impl DopPacker {
    pub fn new() -> Self { Self { marker_hi: false } }

    /// `input[ch]` holds MSB-first DSD bytes; an odd trailing byte is padded with 0x69 (idle).
    pub fn pack(&mut self, input: &[Vec<u8>], out: &mut Vec<i32>) {
        let len = input.first().map_or(0, Vec::len);
        let frames = len.div_ceil(2);
        out.reserve(frames * input.len());
        for f in 0..frames {
            let marker: u32 = if self.marker_hi { 0xFA } else { 0x05 };
            for ch in input {
                let b0 = ch[2 * f] as u32;
                let b1 = *ch.get(2 * f + 1).unwrap_or(&0x69) as u32;
                out.push(((marker << 24) | (b0 << 16) | (b1 << 8)) as i32);
            }
            self.marker_hi = !self.marker_hi;
        }
    }
}

impl Default for DopPacker { fn default() -> Self { Self::new() } }
```
Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml dsd::` → unit tests PASS.

- [ ] **Step 4: Write the failing container + decoder tests**

`tests/dsd.rs`:
```rust
mod common;
use common::*;
use engine_core::decode::dsd::{dff, dsf, DsdDecoder, DsdSource};
use engine_core::decode::{DsdOutput, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn read_all(src: &mut dyn DsdSource) -> Vec<Vec<u8>> {
    let mut all = vec![vec![]; src.channels() as usize];
    let mut buf = vec![vec![]; src.channels() as usize];
    while src.read(&mut buf, 4096).unwrap() > 0 { for (a, b) in all.iter_mut().zip(&buf) { a.extend_from_slice(b); } }
    all
}

#[test]
fn dsf_and_dff_carry_identical_bitstreams() {
    let mut s = dsf::open(&fixture("dsd64.dsf")).unwrap();
    let mut d = dff::open(&fixture("dsd64.dff")).unwrap();
    assert_eq!((s.channels(), s.dsd_rate()), (2, 2_822_400));
    assert_eq!((d.channels(), d.dsd_rate()), (2, 2_822_400));
    let (a, b) = (read_all(s.as_mut()), read_all(d.as_mut()));
    assert_eq!(a[0].len() as u64, s.total_bytes_per_channel());
    assert_eq!(a, b, "DSF (LSB-first, reversed on read) must equal DFF (MSB-first)");
}

fn pcm(name: &str) -> (u32, Vec<f32>) {
    let mut dec = DsdDecoder::open(&fixture(name)).unwrap();
    assert_eq!(dec.info().spec.kind, SampleKind::Float);
    let rate = dec.info().spec.sample_rate;
    let mut out = vec![];
    while let Some(b) = dec.next_block().unwrap() { let PcmData::Float(v) = b.data else { panic!() }; out.extend(v); }
    (rate, out)
}

#[test]
fn dsd64_to_pcm_keeps_tones() {
    let (rate, out) = pcm("dsd64.dsf");
    assert_eq!(rate, 88_200);
    let tail = &out[8192..];
    assert_eq!(dominant_freq(tail, 2, 0, rate), 1000.0);
    assert_eq!(dominant_freq(tail, 2, 1, rate), 3000.0);
    let r = rms(&channel(tail, 2, 0));
    assert!((r - 0.25 / 2f64.sqrt()).abs() < 0.02, "rms {r}");
}

#[test]
fn dsd128_decodes_at_176k() {
    let (rate, out) = pcm("dsd128.dsf");
    assert_eq!(rate, 176_400);
    assert_eq!(dominant_freq(&out[16384..], 2, 0, rate), 1000.0);
}

#[test]
fn dff_decodes() {
    let (rate, out) = pcm("dsd64.dff");
    assert_eq!(rate, 88_200);
    assert_eq!(dominant_freq(&out[8192..], 2, 1, rate), 3000.0);
}

#[test]
fn dop_mode_switches_spec_and_carries_source_bits() {
    let mut dec = DsdDecoder::open(&fixture("dsd64.dsf")).unwrap();
    assert!(dec.set_dsd_output(DsdOutput::Dop));
    assert_eq!(dec.info().spec.sample_rate, 176_400);
    assert_eq!(dec.info().spec.kind, SampleKind::Dop);
    let b = dec.next_block().unwrap().unwrap();
    let PcmData::Int(v) = b.data else { panic!() };
    let bits = read_all(dsf::open(&fixture("dsd64.dsf")).unwrap().as_mut());
    let w = |x: i32| (x as u32) >> 8;
    assert_eq!(w(v[0]) >> 16, 0x05);
    assert_eq!(w(v[2]) >> 16, 0xFA);
    assert_eq!(((w(v[0]) >> 8) & 0xFF) as u8, bits[0][0]);
    assert_eq!((w(v[0]) & 0xFF) as u8, bits[0][1]);
    assert_eq!(((w(v[1]) >> 8) & 0xFF) as u8, bits[1][0]);
}

#[test]
fn seek_moves_to_requested_output_frame() {
    let mut dec = DsdDecoder::open(&fixture("dsd64.dsf")).unwrap();
    let total = dec.info().total_frames.unwrap();
    dec.seek(88_200).unwrap();
    let mut n = 0u64;
    while let Some(b) = dec.next_block().unwrap() { n += b.frames() as u64; }
    assert!((n as i64 - (total as i64 - 88_200)).abs() <= 64, "{n} vs {}", total - 88_200);
}
```

- [ ] **Step 5: Run to verify it fails**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test dsd`
Expected: compile error.

- [ ] **Step 6: Implement the containers and `DsdDecoder`**

`dsd/mod.rs`:
```rust
pub mod decimate;
pub mod dff;
pub mod dop;
pub mod dsf;
#[cfg(any(test, feature = "test-support"))]
pub mod modulator;

use std::path::Path;

use self::{decimate::Decimator, dop::DopPacker};
use super::{DecodeError, DsdOutput, SourceInfo, TrackDecoder};
use crate::pcm::{PcmBlock, PcmData, PcmSpec, SampleKind};

pub trait DsdSource: Send {
    fn channels(&self) -> u16;
    fn dsd_rate(&self) -> u32;
    fn total_bytes_per_channel(&self) -> u64;
    fn read(&mut self, out: &mut [Vec<u8>], max: usize) -> std::io::Result<usize>;
    fn seek_byte(&mut self, byte_per_channel: u64) -> std::io::Result<()>;
}

const CHUNK: usize = 4096;

pub struct DsdDecoder {
    src: Box<dyn DsdSource>,
    mode: DsdOutput,
    dec: Decimator,
    dop: DopPacker,
    bufs: Vec<Vec<u8>>,
    info: SourceInfo,
}

impl DsdDecoder {
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let src = match ext.as_str() {
            "dsf" => dsf::open(path)?,
            "dff" => dff::open(path)?,
            _ => return Err(DecodeError::Unsupported(format!("not a DSD file: .{ext}"))),
        };
        let (ch, rate) = (src.channels(), src.dsd_rate());
        let dec = Decimator::new(ch as usize, rate);
        let mut info = SourceInfo {
            container: ext,
            codec: protocol::Codec::Dsd,
            bitrate_kbps: Some(rate * ch as u32 / 1000),
            spec: PcmSpec { sample_rate: dec.output_rate(), channels: ch, kind: SampleKind::Float },
            total_frames: None,
            gapless: None,
            decoder_name: "dsd",
            dsd_rate: Some(rate),
        };
        info.total_frames = Some(src.total_bytes_per_channel() * 8 * dec.output_rate() as u64 / rate as u64);
        Ok(Self { bufs: vec![Vec::new(); ch as usize], src, mode: DsdOutput::Pcm, dec, dop: DopPacker::new(), info })
    }

    fn bytes_to_frames(&self, bytes: u64) -> u64 {
        match self.mode {
            DsdOutput::Pcm => bytes * 8 * self.info.spec.sample_rate as u64 / self.src.dsd_rate() as u64,
            DsdOutput::Dop => bytes / 2,
        }
    }
}

impl TrackDecoder for DsdDecoder {
    fn info(&self) -> &SourceInfo { &self.info }

    fn next_block(&mut self) -> Result<Option<PcmBlock>, DecodeError> {
        loop {
            let n = self.src.read(&mut self.bufs, CHUNK)?;
            if n == 0 { return Ok(None); }
            let block = match self.mode {
                DsdOutput::Pcm => { let mut v = Vec::new(); self.dec.process(&self.bufs, &mut v); PcmBlock { spec: self.info.spec, data: PcmData::Float(v) } }
                DsdOutput::Dop => { let mut v = Vec::new(); self.dop.pack(&self.bufs, &mut v); PcmBlock { spec: self.info.spec, data: PcmData::Int(v) } }
            };
            if !block.is_empty() { return Ok(Some(block)); }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let per_byte = self.bytes_to_frames(1_000_000) as f64 / 1_000_000.0;
        let byte = ((frame as f64 / per_byte) as u64 / 2 * 2).min(self.src.total_bytes_per_channel());
        self.src.seek_byte(byte)?;
        self.dec.reset();
        self.dop = DopPacker::new();
        Ok(self.bytes_to_frames(byte))
    }

    fn set_dsd_output(&mut self, out: DsdOutput) -> bool {
        self.mode = out;
        let rate = self.src.dsd_rate();
        self.info.spec = match out {
            DsdOutput::Pcm => PcmSpec { sample_rate: self.dec.output_rate(), channels: self.src.channels(), kind: SampleKind::Float },
            DsdOutput::Dop => PcmSpec { sample_rate: rate / 16, channels: self.src.channels(), kind: SampleKind::Dop },
        };
        self.info.total_frames = Some(self.bytes_to_frames(self.src.total_bytes_per_channel()));
        true
    }
}
```

`dsf.rs`:
```rust
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::DsdSource;
use crate::decode::DecodeError;

pub struct DsfReader {
    r: BufReader<File>,
    channels: u16,
    rate: u32,
    lsb_first: bool,
    block: usize,
    data_start: u64,
    total_bytes: u64, // per channel, valid bytes only
    pos: u64,         // per-channel byte position
    group: Vec<u8>,
}

fn u32le(b: &[u8]) -> u32 { u32::from_le_bytes(b[..4].try_into().unwrap()) }
fn u64le(b: &[u8]) -> u64 { u64::from_le_bytes(b[..8].try_into().unwrap()) }

pub fn open(path: &Path) -> Result<Box<dyn DsdSource>, DecodeError> {
    let mut r = BufReader::new(File::open(path)?);
    let mut hdr = [0u8; 28 + 52 + 12];
    r.read_exact(&mut hdr).map_err(|_| DecodeError::Unsupported("truncated DSF".into()))?;
    if &hdr[0..4] != b"DSD " || &hdr[28..32] != b"fmt " || &hdr[80..84] != b"data" {
        return Err(DecodeError::Unsupported("not a DSF file".into()));
    }
    let fmt = &hdr[40..80];
    let (format_id, channels, rate, bps) = (u32le(&fmt[4..]), u32le(&fmt[12..]) as u16, u32le(&fmt[16..]), u32le(&fmt[20..]));
    if format_id != 0 { return Err(DecodeError::Unsupported("DSF: compressed DSD".into())); }
    let sample_count = u64le(&fmt[24..]);
    let block = u32le(&fmt[32..]) as usize;
    if channels == 0 || block == 0 { return Err(DecodeError::Corrupt("DSF: bad fmt chunk".into())); }
    Ok(Box::new(DsfReader {
        r, channels, rate, lsb_first: bps == 1, block,
        data_start: 92, total_bytes: sample_count / 8, pos: 0, group: vec![0; block * channels as usize],
    }))
}

impl DsdSource for DsfReader {
    fn channels(&self) -> u16 { self.channels }
    fn dsd_rate(&self) -> u32 { self.rate }
    fn total_bytes_per_channel(&self) -> u64 { self.total_bytes }

    fn read(&mut self, out: &mut [Vec<u8>], max: usize) -> io::Result<usize> {
        out.iter_mut().for_each(Vec::clear);
        if self.pos >= self.total_bytes { return Ok(0); }
        let block_idx = self.pos / self.block as u64;
        let in_block = (self.pos % self.block as u64) as usize;
        self.r.seek(SeekFrom::Start(self.data_start + block_idx * (self.block * self.channels as usize) as u64))?;
        self.r.read_exact(&mut self.group)?;
        let n = (self.block - in_block).min(max).min((self.total_bytes - self.pos) as usize);
        for (ch, o) in out.iter_mut().enumerate() {
            let s = ch * self.block + in_block;
            o.extend(self.group[s..s + n].iter().map(|&b| if self.lsb_first { b.reverse_bits() } else { b }));
        }
        self.pos += n as u64;
        Ok(n)
    }

    fn seek_byte(&mut self, byte: u64) -> io::Result<()> { self.pos = byte.min(self.total_bytes); Ok(()) }
}
```

`dff.rs`:
```rust
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::DsdSource;
use crate::decode::DecodeError;

pub struct DffReader { r: BufReader<File>, channels: u16, rate: u32, data_start: u64, total_bytes: u64, pos: u64, buf: Vec<u8> }

fn hdr<R: Read>(r: &mut R) -> io::Result<([u8; 4], u64)> {
    let mut b = [0u8; 12];
    r.read_exact(&mut b)?;
    Ok((b[0..4].try_into().unwrap(), u64::from_be_bytes(b[4..12].try_into().unwrap())))
}

pub fn open(path: &Path) -> Result<Box<dyn DsdSource>, DecodeError> {
    let bad = |m: &str| DecodeError::Unsupported(format!("DFF: {m}"));
    let mut r = BufReader::new(File::open(path)?);
    let (id, _) = hdr(&mut r)?;
    let mut form = [0u8; 4];
    r.read_exact(&mut form)?;
    if &id != b"FRM8" || &form != b"DSD " { return Err(bad("not a DSDIFF file")); }
    let (mut channels, mut rate, mut data) = (0u16, 0u32, None);
    loop {
        let Ok((id, size)) = hdr(&mut r) else { break };
        let start = r.stream_position()?;
        match &id {
            b"PROP" => {
                let mut kind = [0u8; 4];
                r.read_exact(&mut kind)?;
                let end = start + size;
                while r.stream_position()? + 12 <= end {
                    let (sid, ssize) = hdr(&mut r)?;
                    let sstart = r.stream_position()?;
                    let mut body = vec![0u8; ssize as usize];
                    r.read_exact(&mut body)?;
                    match &sid {
                        b"FS  " => rate = u32::from_be_bytes(body[..4].try_into().unwrap()),
                        b"CHNL" => channels = u16::from_be_bytes(body[..2].try_into().unwrap()),
                        b"CMPR" if &body[..4] != b"DSD " => return Err(bad("DST-compressed DSD is not supported")),
                        _ => {}
                    }
                    r.seek(SeekFrom::Start(sstart + ssize + (ssize & 1)))?;
                }
            }
            b"DSD " => { data = Some((start, size)); break; }
            _ => {}
        }
        r.seek(SeekFrom::Start(start + size + (size & 1)))?;
    }
    let (data_start, len) = data.ok_or_else(|| bad("no DSD data chunk"))?;
    if channels == 0 || rate == 0 { return Err(bad("missing FS/CHNL")); }
    Ok(Box::new(DffReader { r, channels, rate, data_start, total_bytes: len / channels as u64, pos: 0, buf: Vec::new() }))
}

impl DsdSource for DffReader {
    fn channels(&self) -> u16 { self.channels }
    fn dsd_rate(&self) -> u32 { self.rate }
    fn total_bytes_per_channel(&self) -> u64 { self.total_bytes }

    fn read(&mut self, out: &mut [Vec<u8>], max: usize) -> io::Result<usize> {
        out.iter_mut().for_each(Vec::clear);
        let n = (max as u64).min(self.total_bytes - self.pos) as usize;
        if n == 0 { return Ok(0); }
        let ch = self.channels as usize;
        self.r.seek(SeekFrom::Start(self.data_start + self.pos * ch as u64))?;
        self.buf.resize(n * ch, 0);
        self.r.read_exact(&mut self.buf)?;
        for (c, o) in out.iter_mut().enumerate() { o.extend(self.buf.iter().skip(c).step_by(ch)); }
        self.pos += n as u64;
        Ok(n)
    }

    fn seek_byte(&mut self, byte: u64) -> io::Result<()> { self.pos = byte.min(self.total_bytes); Ok(()) }
}
```

- [ ] **Step 7: Run all DSD tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test dsd dsd::`
Expected: all PASS. Then run the release-mode speed check: `cargo test --release -p engine-core --manifest-path engine/Cargo.toml --test dsd`. It should finish in seconds; the debug build is much slower because of the per-byte inner loops.

- [ ] **Step 8: Commit**

```bash
git add engine/crates/engine-core
git commit -m "feat(decode): DSF/DFF readers, DSD→PCM decimator and DoP packer"
```

---

### Task 9: Decoder router `open()` and the `aurora-engine probe` CLI

**Files:**
- Modify: `engine/crates/engine-core/src/decode/mod.rs` (add `open`)
- Create: `engine/crates/aurora-engine/Cargo.toml`, `engine/crates/aurora-engine/src/main.rs`
- Test: `engine/crates/engine-core/tests/open.rs`, `engine/crates/aurora-engine/tests/cli.rs`

**Interfaces:**
- Consumes: `probe_file`, `SymphoniaDecoder`, `OpusDecoder`, `FdkAacDecoder`, `parse_asc`, `DsdDecoder`.
- Produces: `engine_core::decode::open(path: &Path) -> Result<Box<dyn TrackDecoder>, DecodeError>` and `engine_core::decode::source_summary(info: &SourceInfo) -> protocol::SourceSummary`. CLI: `aurora-engine probe <file>` prints one JSON object `{ "source": SourceSummary, "decoder": "<name>", "totalFrames": n|null }` to stdout and exits 0, or prints the error to stderr and exits 2.

- [ ] **Step 1: Write the failing routing tests**

`tests/open.rs`:
```rust
mod common;
use common::*;
use engine_core::decode::open;
use protocol::{Codec, ErrorCode};

#[test]
fn routes_each_format_to_the_right_decoder() {
    let cases = [
        ("flac16_44k.flac", Codec::Flac, "symphonia"),
        ("mp3_320.mp3", Codec::Mp3, "symphonia"),
        ("aac_lc_256.m4a", Codec::AacLc, "symphonia"),
        ("alac16_44k.m4a", Codec::Alac, "symphonia"),
        ("vorbis_q6.ogg", Codec::Vorbis, "symphonia"),
        ("opus_160.opus", Codec::Opus, "libopus"),
        ("dsd64.dsf", Codec::Dsd, "dsd"),
        ("dsd64.dff", Codec::Dsd, "dsd"),
    ];
    for (name, codec, dec) in cases {
        let d = open(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((d.info().codec, d.info().decoder_name), (codec, dec), "{name}");
    }
}

#[cfg(feature = "fdk-aac")]
#[test]
fn he_aac_goes_to_fdk() {
    assert_eq!(open(&fixture("he_aac.m4a")).unwrap().info().decoder_name, "fdk-aac");
    assert_eq!(open(&fixture("he_aac_v2.m4a")).unwrap().info().codec, Codec::HeAacV2);
    assert_eq!(open(&fixture("he_aac.aac")).unwrap().info().decoder_name, "fdk-aac");
}

#[cfg(not(feature = "fdk-aac"))]
#[test]
fn he_aac_without_fdk_reports_codec_not_in_build() {
    let e = open(&fixture("he_aac.m4a")).err().unwrap();
    assert_eq!(e.code(), ErrorCode::CodecNotInBuild);
}

#[test]
fn error_codes() {
    assert_eq!(open(&fixture("empty.mp3")).err().unwrap().code(), ErrorCode::UnsupportedFormat);
    let missing = std::path::Path::new("definitely/not/here.flac");
    assert_eq!(open(missing).err().unwrap().code(), ErrorCode::FileMissing);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test open`
Expected: compile error (`open` not found).

- [ ] **Step 3: Implement `open()` and `source_summary()`** (append to `decode/mod.rs`)

```rust
pub mod aac_config;
#[cfg(feature = "fdk-aac")]
pub mod fdk_dec;
pub mod mp4_gapless;
pub mod opus_dec;
pub mod symphonia_dec;

use std::path::Path;
use symphonia::core::codecs::audio::well_known::{CODEC_ID_AAC, CODEC_ID_OPUS};

pub fn open(path: &Path) -> Result<Box<dyn TrackDecoder>, DecodeError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "dsf" || ext == "dff" {
        return Ok(Box::new(dsd::DsdDecoder::open(path)?));
    }
    if ext == "aac" {
        #[cfg(feature = "fdk-aac")]
        return Ok(Box::new(fdk_dec::FdkAacDecoder::open_adts(path)?));
    }
    let probed = symphonia_dec::probe_file(path)?;
    let codec = probed.params.codec;
    if codec == CODEC_ID_OPUS {
        return Ok(Box::new(opus_dec::OpusDecoder::new(probed)?));
    }
    if codec == CODEC_ID_AAC {
        let cfg = probed.params.extra_data.as_deref().and_then(aac_config::parse_asc);
        if let Some(cfg) = cfg.filter(|c| c.sbr || c.ps) {
            #[cfg(feature = "fdk-aac")]
            return Ok(Box::new(fdk_dec::FdkAacDecoder::new_mp4(probed, cfg)?));
            #[cfg(not(feature = "fdk-aac"))]
            {
                let _ = cfg;
                return Err(DecodeError::CodecNotInBuild("HE-AAC"));
            }
        }
    }
    Ok(Box::new(symphonia_dec::SymphoniaDecoder::new(probed)?))
}

pub fn source_summary(info: &SourceInfo) -> protocol::SourceSummary {
    protocol::SourceSummary {
        container: info.container.clone(),
        codec: info.codec,
        bitrate_kbps: info.bitrate_kbps,
        sample_rate: info.spec.sample_rate,
        bit_depth: match info.spec.kind { crate::pcm::SampleKind::Int { bits } => Some(bits), _ => None },
        channels: info.spec.channels,
        dsd_rate: info.dsd_rate,
    }
}
```
Remove the duplicate `pub mod` lines added in earlier tasks so each module is declared once.

- [ ] **Step 4: Run the routing tests (both feature sets)**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test open`
Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --no-default-features --test open`
Expected: both PASS.

- [ ] **Step 5: Write the failing CLI test**

`engine/crates/aurora-engine/Cargo.toml`:
```toml
[package]
name = "aurora-engine"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[features]
default = ["fdk-aac"]
fdk-aac = ["engine-core/fdk-aac"]

[dependencies]
engine-core.workspace = true
protocol.workspace = true
serde.workspace = true
serde_json.workspace = true
clap.workspace = true
anyhow.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
crossbeam-channel.workspace = true
```

`tests/cli.rs`:
```rust
use std::process::Command;

fn fixture(name: &str) -> String {
    format!("{}/../../../fixtures/out/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn probe_prints_source_json() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).args(["probe", &fixture("flac24_96k.flac")]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["source"]["codec"], "flac");
    assert_eq!(v["source"]["sampleRate"], 96000);
    assert_eq!(v["source"]["bitDepth"], 24);
    assert_eq!(v["decoder"], "symphonia");
}

#[test]
fn probe_unsupported_exits_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).args(["probe", &fixture("empty.mp3")]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "errors must never be written to stdout");
}
```

- [ ] **Step 6: Implement `main.rs` (the `devices`, `play` and `rpc` subcommands are added in Tasks 15 and 19)**

```rust
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aurora-engine", version, about = "Aurora audio engine")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print what Aurora sees in a file.
    Probe { file: PathBuf },
}

fn init_logging() {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("AURORA_LOG").unwrap_or_else(|_| "info".into()))
        .init();
}

fn main() {
    init_logging();
    let cli = Cli::parse();
    let code = match cli.cmd {
        Some(Cmd::Probe { file }) => probe(&file),
        None => { eprintln!("rpc mode arrives in a later task"); 1 }
    };
    std::process::exit(code);
}

fn probe(file: &std::path::Path) -> i32 {
    match engine_core::decode::open(file) {
        Ok(d) => {
            let i = d.info();
            let v = serde_json::json!({
                "source": engine_core::decode::source_summary(i),
                "decoder": i.decoder_name,
                "totalFrames": i.total_frames,
            });
            println!("{v}");
            0
        }
        Err(e) => { eprintln!("{}: {e}", e.code().name()); 2 }
    }
}
```

- [ ] **Step 7: Run the CLI tests**

Run: `cargo test -p aurora-engine --manifest-path engine/Cargo.toml --test cli`
Expected: 2 PASS. Also try `cargo run -q --manifest-path engine/Cargo.toml -p aurora-engine -- probe fixtures/out/he_aac.m4a` and confirm it prints `"codec":"he-aac"`.

- [ ] **Step 8: Commit**

```bash
git add engine/crates
git commit -m "feat: decoder router and aurora-engine probe CLI"
```

---
### Task 10: DSP primitives — sample conversion, channel mapping, volume, resampler

**Files:**
- Create: `engine/crates/engine-core/src/output/format.rs`, `src/dsp/convert.rs`, `src/dsp/channels.rs`, `src/dsp/volume.rs`, `src/dsp/resample.rs`
- Modify: `src/dsp/mod.rs`, `src/output/mod.rs` (add `pub mod format;`)

**Interfaces:**
- Produces:
```rust
// output::format
pub enum DeviceSample { I16, I24 /* packed 3-byte LE */, I32 /* left-justified; also used for 24-in-32 */, F32 }
impl DeviceSample { pub fn bytes(self) -> usize; pub fn bits(self) -> u16; pub fn is_int(self) -> bool; pub fn wire_name(self) -> &'static str }
pub struct OutputFormat { pub sample_rate: u32, pub channels: u16, pub sample: DeviceSample }
impl OutputFormat { pub fn bytes_per_frame(&self) -> usize }
// dsp::convert
pub fn write_int_passthrough(src: &[i32], dst: DeviceSample, out: &mut Vec<u8>);   // dst must be integer
pub fn write_f32_passthrough(src: &[f32], out: &mut Vec<u8>);
pub struct Ditherer; impl Ditherer { pub fn new(seed: u64) -> Self; pub fn tpdf(&mut self) -> f64 }  // (-1, 1) LSB
pub fn write_f64(src: &[f64], dst: DeviceSample, dither: Option<&mut Ditherer>, out: &mut Vec<u8>);
// dsp::channels
pub enum ChannelMap { Identity, Matrix { from: u16, to: u16, m: Vec<f64> } }
impl ChannelMap { pub fn plan(from: u16, to: u16) -> Self; pub fn is_identity(&self) -> bool; pub fn apply(&self, input: &[f64], out: &mut Vec<f64>); pub fn describe(&self) -> String }
// dsp::volume
pub fn volume_gain(v: f32) -> f64;   // v² (perceptual), clamped
// dsp::resample
pub struct StreamResampler; impl StreamResampler {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, String>;
    pub fn config(&self) -> (u32, u32, usize);
    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>);
    pub fn flush(&mut self, out: &mut Vec<f64>);
}
```

- [ ] **Step 1: Write the failing unit tests**

Bottom of `convert.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_passthrough_is_lossless() {
        let s24 = [0x7FFF_FF00u32 as i32, -256, 0x0012_3400];
        let mut out = vec![];
        write_int_passthrough(&s24, DeviceSample::I24, &mut out);
        assert_eq!(out, vec![0xFF, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF, 0x34, 0x12, 0x00]);
        out.clear();
        write_int_passthrough(&[1 << 16, -(1 << 16)], DeviceSample::I16, &mut out);
        assert_eq!(out, vec![1, 0, 0xFF, 0xFF]);
        out.clear();
        write_int_passthrough(&[0x1234_0000], DeviceSample::I32, &mut out);
        assert_eq!(out, 0x1234_0000i32.to_le_bytes());
    }

    #[test]
    fn f64_to_i16_dithers_within_one_lsb_and_clips() {
        let mut d = Ditherer::new(7);
        let mut out = vec![];
        write_f64(&vec![0.25; 10_000], DeviceSample::I16, Some(&mut d), &mut out);
        let vals: Vec<i16> = out.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert!(vals.iter().all(|&v| (8191..=8193).contains(&v)));
        let mean = vals.iter().map(|&v| v as f64).sum::<f64>() / vals.len() as f64;
        assert!((mean - 8192.0).abs() < 0.05);
        out.clear();
        write_f64(&[1.5, -1.5], DeviceSample::I16, None, &mut out);
        assert_eq!(out, vec![0xFF, 0x7F, 0x00, 0x80]);
    }

    #[test]
    fn f32_paths_are_exact() {
        let mut out = vec![];
        write_f32_passthrough(&[0.5, -0.25], &mut out);
        assert_eq!(out, [0.5f32.to_le_bytes(), (-0.25f32).to_le_bytes()].concat());
        out.clear();
        write_f64(&[0.125], DeviceSample::F32, None, &mut out);
        assert_eq!(out, 0.125f32.to_le_bytes());
    }
}
```

Bottom of `channels.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_passes_through() {
        let m = ChannelMap::plan(2, 2);
        assert!(m.is_identity());
        let mut out = vec![];
        m.apply(&[0.1, 0.2], &mut out);
        assert_eq!(out, vec![0.1, 0.2]);
    }

    #[test]
    fn mono_duplicates_to_stereo() {
        let m = ChannelMap::plan(1, 2);
        let mut out = vec![];
        m.apply(&[0.5, -0.5], &mut out);
        assert_eq!(out, vec![0.5, 0.5, -0.5, -0.5]);
        assert_eq!(m.describe(), "mono → stereo");
    }

    #[test]
    fn downmix_51_never_clips() {
        let m = ChannelMap::plan(6, 2);
        let mut out = vec![];
        m.apply(&[1.0; 6], &mut out);
        assert!(out.iter().all(|&v| v <= 1.0 + 1e-12 && v > 0.99), "{out:?}");
        out.clear();
        m.apply(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0], &mut out); // FL only → left only
        assert!(out[0] > 0.3 && out[1] == 0.0);
        assert_eq!(m.describe(), "5.1 → stereo downmix");
    }
}
```

Bottom of `volume.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curve() {
        assert_eq!(volume_gain(1.0), 1.0);
        assert_eq!(volume_gain(0.5), 0.25);
        assert_eq!(volume_gain(0.0), 0.0);
        assert_eq!(volume_gain(3.0), 1.0);
    }
}
```

Bottom of `resample.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsamples_44k1_to_48k_keeping_length_and_pitch() {
        let mut r = StreamResampler::new(44_100, 48_000, 2).unwrap();
        let input: Vec<f64> = (0..44_100).flat_map(|i| { let s = (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44_100.0).sin() * 0.5; [s, s] }).collect();
        let mut out = vec![];
        for chunk in input.chunks(2 * 777) { r.process(chunk, &mut out); }
        r.flush(&mut out);
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() <= 2, "{frames}");
        // Zero crossings over the middle 0.5 s ≈ 2 per cycle → 1000 Hz.
        let left: Vec<f64> = out.iter().step_by(2).copied().collect();
        let mid = &left[12_000..36_000];
        let crossings = mid.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((crossings as i64 - 1000).abs() <= 2, "{crossings}");
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml dsp::`
Expected: compile errors.

- [ ] **Step 3: Implement**

`output/format.rs`:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceSample { I16, I24, I32, F32 }

impl DeviceSample {
    pub fn bytes(self) -> usize { match self { Self::I16 => 2, Self::I24 => 3, Self::I32 | Self::F32 => 4 } }
    pub fn bits(self) -> u16 { match self { Self::I16 => 16, Self::I24 => 24, Self::I32 | Self::F32 => 32 } }
    pub fn is_int(self) -> bool { !matches!(self, Self::F32) }
    pub fn wire_name(self) -> &'static str { match self { Self::I16 => "s16", Self::I24 => "s24", Self::I32 => "s32", Self::F32 => "f32" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat { pub sample_rate: u32, pub channels: u16, pub sample: DeviceSample }

impl OutputFormat {
    pub fn bytes_per_frame(&self) -> usize { self.sample.bytes() * self.channels as usize }
}
```

`dsp/mod.rs`:
```rust
pub mod channels;
pub mod convert;
pub mod resample;
pub mod volume;
```

`dsp/convert.rs`:
```rust
use crate::output::format::DeviceSample;

/// Bit-perfect path: left-justified i32 → device integer format, by truncating low (zero) bits.
pub fn write_int_passthrough(src: &[i32], dst: DeviceSample, out: &mut Vec<u8>) {
    out.reserve(src.len() * dst.bytes());
    match dst {
        DeviceSample::I16 => src.iter().for_each(|&s| out.extend_from_slice(&((s >> 16) as i16).to_le_bytes())),
        DeviceSample::I24 => src.iter().for_each(|&s| out.extend_from_slice(&(s >> 8).to_le_bytes()[..3])),
        DeviceSample::I32 => src.iter().for_each(|&s| out.extend_from_slice(&s.to_le_bytes())),
        DeviceSample::F32 => panic!("write_int_passthrough needs an integer device format"),
    }
}

pub fn write_f32_passthrough(src: &[f32], out: &mut Vec<u8>) {
    out.reserve(src.len() * 4);
    src.iter().for_each(|&s| out.extend_from_slice(&s.to_le_bytes()));
}

/// xorshift64* — fast, deterministic, good enough for dither.
pub struct Ditherer { state: u64 }

impl Ditherer {
    pub fn new(seed: u64) -> Self { Self { state: seed.max(1) } }
    fn uniform(&mut self) -> f64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let v = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }
    /// Triangular PDF in (-1, 1) LSB.
    pub fn tpdf(&mut self) -> f64 { self.uniform() + self.uniform() }
}

pub fn write_f64(src: &[f64], dst: DeviceSample, mut dither: Option<&mut Ditherer>, out: &mut Vec<u8>) {
    out.reserve(src.len() * dst.bytes());
    if dst == DeviceSample::F32 {
        src.iter().for_each(|&s| out.extend_from_slice(&(s as f32).to_le_bytes()));
        return;
    }
    let scale = (1u64 << (dst.bits() - 1)) as f64;
    for &s in src {
        let d = if dst == DeviceSample::I32 { 0.0 } else { dither.as_deref_mut().map_or(0.0, Ditherer::tpdf) };
        let v = (s * scale + d).round().clamp(-scale, scale - 1.0) as i64;
        match dst {
            DeviceSample::I16 => out.extend_from_slice(&(v as i16).to_le_bytes()),
            DeviceSample::I24 => out.extend_from_slice(&(v as i32).to_le_bytes()[..3]),
            DeviceSample::I32 => out.extend_from_slice(&(v as i32).to_le_bytes()),
            DeviceSample::F32 => unreachable!(),
        }
    }
}
```

`dsp/channels.rs`:
```rust
//! Channel layout adaptation. Input layouts follow WAV/FLAC order.
const H: f64 = std::f64::consts::FRAC_1_SQRT_2;

#[derive(Clone, Debug, PartialEq)]
pub enum ChannelMap { Identity, Matrix { from: u16, to: u16, m: Vec<f64> } }

#[derive(Clone, Copy)]
enum Pos { FL, FR, FC, LFE, BL, BR, BC, SL, SR }

/// (left, right) weight for input channel `i` of an `n`-channel source.
fn lr_weight(n: u16, i: u16) -> (f64, f64) {
    use Pos::*;
    let layout: &[Pos] = match n {
        3 => &[FL, FR, FC],
        4 => &[FL, FR, BL, BR],
        5 => &[FL, FR, FC, BL, BR],
        6 => &[FL, FR, FC, LFE, BL, BR],
        7 => &[FL, FR, FC, LFE, BC, SL, SR],
        _ => &[FL, FR, FC, LFE, BL, BR, SL, SR],
    };
    match layout.get(i as usize).copied().unwrap_or(LFE) {
        FL => (1.0, 0.0), FR => (0.0, 1.0), FC => (H, H), LFE => (0.0, 0.0),
        BL | SL => (H, 0.0), BR | SR => (0.0, H), BC => (0.5, 0.5),
    }
}

impl ChannelMap {
    pub fn plan(from: u16, to: u16) -> Self {
        if from == to { return Self::Identity; }
        let (f, t) = (from as usize, to as usize);
        let mut m = vec![0.0; f * t];
        if from == 1 {
            for o in 0..t { m[o * f] = 1.0; }
        } else if to == 2 {
            for i in 0..from { let (l, r) = lr_weight(from, i); m[i as usize] = l; m[f + i as usize] = r; }
            for o in 0..2 {
                let sum: f64 = m[o * f..(o + 1) * f].iter().sum();
                if sum > 0.0 { m[o * f..(o + 1) * f].iter_mut().for_each(|v| *v /= sum); }
            }
        } else if to == 1 {
            m.iter_mut().for_each(|v| *v = 1.0 / f as f64);
        } else {
            for o in 0..t.min(f) { m[o * f + o] = 1.0; }
        }
        Self::Matrix { from, to, m }
    }

    pub fn is_identity(&self) -> bool { matches!(self, Self::Identity) }

    pub fn apply(&self, input: &[f64], out: &mut Vec<f64>) {
        match self {
            Self::Identity => out.extend_from_slice(input),
            Self::Matrix { from, to, m } => {
                let (f, t) = (*from as usize, *to as usize);
                out.reserve(input.len() / f * t);
                for frame in input.chunks_exact(f) {
                    for o in 0..t {
                        out.push(frame.iter().zip(&m[o * f..(o + 1) * f]).map(|(x, w)| x * w).sum());
                    }
                }
            }
        }
    }

    pub fn describe(&self) -> String {
        let name = |n: u16| match n { 1 => "mono".to_string(), 2 => "stereo".into(), 6 => "5.1".into(), 8 => "7.1".into(), n => format!("{n} ch") };
        match self {
            Self::Identity => "unchanged".into(),
            Self::Matrix { from: 1, to, .. } => format!("mono → {}", name(*to)),
            Self::Matrix { from, to, .. } if from > to => format!("{} → {} downmix", name(*from), name(*to)),
            Self::Matrix { from, to, .. } => format!("{} → {}", name(*from), name(*to)),
        }
    }
}
```

`dsp/volume.rs`:
```rust
/// Perceptual volume curve: gain = v².
pub fn volume_gain(v: f32) -> f64 {
    let v = v.clamp(0.0, 1.0) as f64;
    v * v
}
```

`dsp/resample.rs`:
```rust
use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};

/// Streaming high-quality sinc resampler over interleaved f64.
/// Removes the resampler's own delay at the start and pads/truncates at `flush` so that
/// output length = round(input length × ratio).
pub struct StreamResampler {
    r: Async<f64>,
    from: u32,
    to: u32,
    ch: usize,
    pending: Vec<f64>,
    out_buf: Vec<f64>,
    to_drop: usize,
    in_total: u64,
    out_total: u64,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let params = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
            .oversampling_factor(256)
            .interpolation(SincInterpolationType::Cubic);
        let r = Async::<f64>::new_sinc(to as f64 / from as f64, 1.0, &params, 1024, channels, FixedAsync::Input).map_err(|e| e.to_string())?;
        let to_drop = r.output_delay();
        let out_buf = vec![0.0; r.output_frames_max() * channels];
        Ok(Self { r, from, to, ch: channels, pending: Vec::new(), out_buf, to_drop, in_total: 0, out_total: 0 })
    }

    pub fn config(&self) -> (u32, u32, usize) { (self.from, self.to, self.ch) }

    fn run(&mut self, frames: usize, partial: Option<usize>, out: &mut Vec<f64>) -> usize {
        let ch = self.ch;
        let (n_in, n_out) = {
            let inp = InterleavedSlice::new(&self.pending[..frames * ch], ch, frames).expect("input adapter");
            let cap = self.out_buf.len() / ch;
            let mut o = InterleavedSlice::new_mut(&mut self.out_buf, ch, cap).expect("output adapter");
            let mut idx = Indexing::new();
            idx.partial_len = partial;
            self.r.process_into_buffer(&inp, &mut o, Some(&idx)).expect("resample")
        };
        let drop = self.to_drop.min(n_out);
        self.to_drop -= drop;
        out.extend_from_slice(&self.out_buf[drop * ch..n_out * ch]);
        self.out_total += (n_out - drop) as u64;
        n_in.min(frames)
    }

    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        self.pending.extend_from_slice(input);
        self.in_total += (input.len() / self.ch) as u64;
        loop {
            let need = self.r.input_frames_next();
            if self.pending.len() / self.ch < need { break; }
            let used = self.run(need, None, out);
            self.pending.drain(..used * self.ch);
        }
    }

    pub fn flush(&mut self, out: &mut Vec<f64>) {
        let expected = (self.in_total as f64 * self.to as f64 / self.from as f64).round() as u64;
        let start_len = out.len();
        let start_total = self.out_total;
        loop {
            let have = self.pending.len() / self.ch;
            let need = self.r.input_frames_next();
            if self.pending.len() < need * self.ch { self.pending.resize(need * self.ch, 0.0); }
            self.run(need, Some(have.min(need)), out);
            self.pending.drain(..need * self.ch);
            if self.out_total >= expected { break; }
        }
        let keep = (expected - start_total) as usize * self.ch;
        out.truncate(start_len + keep);
        self.out_total = expected;
        self.pending.clear();
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml dsp::`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core/src
git commit -m "feat(dsp): bit-perfect/dithered conversion, channel maps, volume curve, streaming resampler"
```

---

### Task 11: Output core — backend traits, lock-free ring and NullBackend

**Files:**
- Create: `engine/crates/engine-core/src/output/ring.rs`, `src/output/null.rs`
- Modify: `engine/crates/engine-core/src/output/mod.rs`

**Interfaces:**
- Consumes: `DeviceSample`, `OutputFormat` (Task 10).
- Produces:
```rust
// output
pub struct DeviceCaps { pub rates: Vec<u32>, pub max_channels: u16, pub samples: Vec<DeviceSample>, pub exclusive: bool }
impl DeviceCaps { pub fn supports_rate(&self, r: u32) -> bool; pub fn supports(&self, s: DeviceSample) -> bool; pub fn best_int_for(&self, bits: u16) -> Option<DeviceSample>; pub fn fallback() -> Self }
pub struct StreamInfo { pub device_id: String, pub device_name: String, pub transport: Transport, pub share: ShareMode, pub format: OutputFormat }
pub enum OutputError { DeviceUnavailable(String), ExclusiveDenied(String), UnsupportedFormat(String), Other(String) }  // + fn code() -> ErrorCode
pub trait OutputBackend: Send + Sync {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError>;
    fn caps(&self, device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError>;
    fn open(&self, device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError>;
    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> { (stream.transport == Transport::Bluetooth).then(BluetoothInfo::default) }
}
pub trait OutputStream { fn info(&self) -> &StreamInfo; fn start(&mut self) -> Result<(), OutputError>; }
pub const COMMON_RATES: [u32; 10] = [44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000, 705600, 768000];
// output::ring
pub struct StreamShared { pub consumed_frames: AtomicU64, pub discard_to: AtomicU64, pub underruns: AtomicU64, pub playing: AtomicBool, pub draining: AtomicBool, pub device_lost: AtomicBool, pub apply_gain: AtomicBool, /* + set_gain/gain */ }
pub struct RingSource; impl RingSource { pub fn fill(&mut self, out: &mut [u8]) -> usize /* bytes taken from ring */; pub fn shared(&self) -> &Arc<StreamShared>; pub fn bytes_per_frame(&self) -> usize }
pub fn ring_pair(capacity_frames: usize, format: OutputFormat) -> (rtrb::Producer<u8>, RingSource, Arc<StreamShared>);
// output::null
pub enum NullMode { Manual, Realtime { speed: f64 } }
pub struct NullBackend; impl NullBackend { pub fn new(mode: NullMode) -> (NullBackend, NullHandle) }
#[derive(Clone)] pub struct NullHandle; impl NullHandle {
    pub fn pull(&self, frames: usize) -> usize; pub fn recorded(&self) -> Vec<u8>; pub fn clear_recorded(&self);
    pub fn opened(&self) -> Vec<(OutputFormat, bool)>; pub fn set_caps(&self, c: DeviceCaps); pub fn set_transport(&self, t: Transport);
    pub fn set_bluetooth(&self, b: Option<BluetoothInfo>); pub fn set_fail_exclusive(&self, on: bool); pub fn simulate_device_lost(&self);
}
```
- **Ring semantics:** the producer is the player thread and the consumer is the output callback. To discard queued audio (on seek), the player stores the absolute frame index it has written so far in `discard_to`; the callback silently drops frames below that index. No handshake is needed. `consumed_frames` is the absolute index of the next frame to be heard, and positions are computed from it. While `playing == false`, the callback outputs silence and keeps the queued data. Software gain is applied in the callback only when `apply_gain` is set (processed mode), so volume changes are instant.

- [ ] **Step 1: Write the failing tests**

Bottom of `ring.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::format::{DeviceSample, OutputFormat};
    use std::sync::atomic::Ordering::*;

    fn fmt() -> OutputFormat { OutputFormat { sample_rate: 48_000, channels: 2, sample: DeviceSample::I16 } }

    fn push(p: &mut rtrb::Producer<u8>, bytes: &[u8]) {
        let mut c = p.write_chunk(bytes.len()).unwrap();
        let (a, b) = c.as_mut_slices();
        a.copy_from_slice(&bytes[..a.len()]);
        b.copy_from_slice(&bytes[a.len()..]);
        c.commit_all();
    }

    #[test]
    fn copies_whole_frames_and_zero_fills_with_underrun() {
        let (mut p, mut s, sh) = ring_pair(16, fmt());
        sh.playing.store(true, Relaxed);
        push(&mut p, &[1, 2, 3, 4, 5, 6, 7, 8, 9]); // 2 frames + 1 stray byte
        let mut out = [0xEEu8; 16];
        assert_eq!(s.fill(&mut out), 8);
        assert_eq!(&out[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(out[8..].iter().all(|&b| b == 0));
        assert_eq!(sh.underruns.load(Relaxed), 1);
        assert_eq!(sh.consumed_frames.load(Relaxed), 2);
    }

    #[test]
    fn paused_outputs_silence_and_keeps_data() {
        let (mut p, mut s, sh) = ring_pair(16, fmt());
        push(&mut p, &[1; 8]);
        let mut out = [9u8; 8];
        assert_eq!(s.fill(&mut out), 0);
        assert_eq!(out, [0; 8]);
        sh.playing.store(true, Relaxed);
        assert_eq!(s.fill(&mut out), 8);
    }

    #[test]
    fn discard_to_drops_queued_frames_only() {
        let (mut p, mut s, sh) = ring_pair(16, fmt());
        sh.playing.store(true, Relaxed);
        push(&mut p, &[1; 12]);      // frames 0..3 (old)
        sh.discard_to.store(3, Release);
        push(&mut p, &[2; 8]);       // frames 3..5 (new)
        let mut out = [0u8; 8];
        assert_eq!(s.fill(&mut out), 8);
        assert_eq!(out, [2; 8]);
        assert_eq!(sh.consumed_frames.load(Relaxed), 5);
    }

    #[test]
    fn draining_does_not_count_underruns() {
        let (_p, mut s, sh) = ring_pair(16, fmt());
        sh.playing.store(true, Relaxed);
        sh.draining.store(true, Relaxed);
        s.fill(&mut [0u8; 8]);
        assert_eq!(sh.underruns.load(Relaxed), 0);
    }

    #[test]
    fn gain_applies_only_when_enabled() {
        let f = OutputFormat { sample_rate: 48_000, channels: 1, sample: DeviceSample::F32 };
        let (mut p, mut s, sh) = ring_pair(16, f);
        sh.playing.store(true, Relaxed);
        sh.set_gain(0.5);
        push(&mut p, &1.0f32.to_le_bytes());
        let mut out = [0u8; 4];
        s.fill(&mut out);
        assert_eq!(f32::from_le_bytes(out), 1.0);
        sh.apply_gain.store(true, Relaxed);
        push(&mut p, &1.0f32.to_le_bytes());
        s.fill(&mut out);
        assert_eq!(f32::from_le_bytes(out), 0.5);
    }
}
```

Bottom of `null.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::ring::ring_pair;
    use std::sync::atomic::Ordering::*;

    #[test]
    fn manual_pull_records_ring_bytes_and_tracks_opens() {
        let (b, h) = NullBackend::new(NullMode::Manual);
        let f = OutputFormat { sample_rate: 44_100, channels: 2, sample: DeviceSample::I16 };
        let (mut p, src, sh) = ring_pair(64, f);
        let mut s = b.open(None, f, true, src).unwrap();
        s.start().unwrap();
        sh.playing.store(true, Relaxed);
        let mut c = p.write_chunk(8).unwrap();
        c.as_mut_slices().0.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        c.commit_all();
        assert_eq!(h.pull(4), 2);
        assert_eq!(h.recorded(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(h.opened(), vec![(f, true)]);
    }

    #[test]
    fn exclusive_denial_can_be_simulated() {
        let (b, h) = NullBackend::new(NullMode::Manual);
        h.set_fail_exclusive(true);
        let f = OutputFormat { sample_rate: 44_100, channels: 2, sample: DeviceSample::I16 };
        let (_p, src, _) = ring_pair(64, f);
        assert!(matches!(b.open(None, f, true, src), Err(OutputError::ExclusiveDenied(_))));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml output::`
Expected: compile errors.

- [ ] **Step 3: Implement**

`output/mod.rs`:
```rust
pub mod format;
pub mod null;
pub mod ring;

use format::{DeviceSample, OutputFormat};
use protocol::{BluetoothInfo, DeviceInfo, ErrorCode, ShareMode, Transport};
pub use ring::RingSource;

pub const COMMON_RATES: [u32; 10] = [44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000, 705600, 768000];

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceCaps { pub rates: Vec<u32>, pub max_channels: u16, pub samples: Vec<DeviceSample>, pub exclusive: bool }

impl DeviceCaps {
    pub fn supports_rate(&self, r: u32) -> bool { self.rates.contains(&r) }
    pub fn supports(&self, s: DeviceSample) -> bool { self.samples.contains(&s) }
    /// Narrowest integer format that carries `bits` without loss.
    pub fn best_int_for(&self, bits: u16) -> Option<DeviceSample> {
        [DeviceSample::I16, DeviceSample::I24, DeviceSample::I32].into_iter().find(|&s| s.bits() >= bits && self.supports(s))
    }
    pub fn fallback() -> Self { Self { rates: vec![48_000], max_channels: 2, samples: vec![DeviceSample::F32], exclusive: false } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamInfo { pub device_id: String, pub device_name: String, pub transport: Transport, pub share: ShareMode, pub format: OutputFormat }

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("device unavailable: {0}")] DeviceUnavailable(String),
    #[error("exclusive access denied: {0}")] ExclusiveDenied(String),
    #[error("unsupported output format: {0}")] UnsupportedFormat(String),
    #[error("{0}")] Other(String),
}

impl OutputError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::ExclusiveDenied(_) => ErrorCode::ExclusiveDenied,
            Self::UnsupportedFormat(_) => ErrorCode::UnsupportedFormat,
            _ => ErrorCode::DeviceUnavailable,
        }
    }
}

pub trait OutputBackend: Send + Sync {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError>;
    fn caps(&self, device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError>;
    fn open(&self, device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError>;
    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> {
        (stream.transport == Transport::Bluetooth).then(BluetoothInfo::default)
    }
}

/// Created and dropped on the player thread; dropping it closes the device.
pub trait OutputStream {
    fn info(&self) -> &StreamInfo;
    fn start(&mut self) -> Result<(), OutputError>;
}
```

`output/ring.rs`:
```rust
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use super::format::{DeviceSample, OutputFormat};

pub struct StreamShared {
    pub consumed_frames: AtomicU64,
    pub discard_to: AtomicU64,
    pub underruns: AtomicU64,
    pub playing: AtomicBool,
    pub draining: AtomicBool,
    pub device_lost: AtomicBool,
    pub apply_gain: AtomicBool,
    gain_bits: AtomicU32,
}

impl StreamShared {
    pub fn set_gain(&self, g: f32) { self.gain_bits.store(g.to_bits(), Relaxed); }
    pub fn gain(&self) -> f32 { f32::from_bits(self.gain_bits.load(Relaxed)) }
}

pub struct RingSource { cons: Consumer<u8>, shared: Arc<StreamShared>, bpf: usize, sample: DeviceSample, consumed: u64 }

pub fn ring_pair(capacity_frames: usize, format: OutputFormat) -> (Producer<u8>, RingSource, Arc<StreamShared>) {
    let bpf = format.bytes_per_frame();
    let (prod, cons) = RingBuffer::new(capacity_frames.max(1) * bpf);
    let shared = Arc::new(StreamShared {
        consumed_frames: AtomicU64::new(0), discard_to: AtomicU64::new(0), underruns: AtomicU64::new(0),
        playing: AtomicBool::new(false), draining: AtomicBool::new(false), device_lost: AtomicBool::new(false),
        apply_gain: AtomicBool::new(false), gain_bits: AtomicU32::new(1.0f32.to_bits()),
    });
    (prod, RingSource { cons, shared: shared.clone(), bpf, sample: format.sample, consumed: 0 }, shared)
}

impl RingSource {
    pub fn shared(&self) -> &Arc<StreamShared> { &self.shared }
    pub fn bytes_per_frame(&self) -> usize { self.bpf }

    /// Realtime-safe (no allocation, no locks). Returns the number of bytes taken from the ring.
    pub fn fill(&mut self, out: &mut [u8]) -> usize {
        let bpf = self.bpf;
        let discard_to = self.shared.discard_to.load(Acquire);
        if self.consumed < discard_to {
            let k = ((self.cons.slots() / bpf) as u64).min(discard_to - self.consumed) as usize;
            if k > 0 {
                if let Ok(c) = self.cons.read_chunk(k * bpf) { c.commit_all(); }
                self.consumed += k as u64;
            }
        }
        let mut n = 0;
        if self.shared.playing.load(Acquire) && self.consumed >= discard_to {
            let want = out.len() / bpf;
            let frames = want.min(self.cons.slots() / bpf);
            n = frames * bpf;
            if n > 0 {
                let chunk = self.cons.read_chunk(n).expect("slots checked above");
                let (a, b) = chunk.as_slices();
                out[..a.len()].copy_from_slice(a);
                out[a.len()..n].copy_from_slice(b);
                chunk.commit_all();
                self.consumed += frames as u64;
                if self.shared.apply_gain.load(Relaxed) { apply_gain(&mut out[..n], self.sample, self.shared.gain()); }
            }
            if frames < want && !self.shared.draining.load(Relaxed) { self.shared.underruns.fetch_add(1, Relaxed); }
        }
        out[n..].fill(0);
        self.shared.consumed_frames.store(self.consumed, Release);
        n
    }
}

fn apply_gain(buf: &mut [u8], s: DeviceSample, g: f32) {
    if g == 1.0 { return; }
    match s {
        DeviceSample::F32 => for c in buf.chunks_exact_mut(4) {
            let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]) * g;
            c.copy_from_slice(&v.to_le_bytes());
        },
        DeviceSample::I16 => for c in buf.chunks_exact_mut(2) {
            let v = (i16::from_le_bytes([c[0], c[1]]) as f32 * g) as i16;
            c.copy_from_slice(&v.to_le_bytes());
        },
        DeviceSample::I24 => for c in buf.chunks_exact_mut(3) {
            let v = ((i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 * g) as i32;
            c.copy_from_slice(&v.to_le_bytes()[..3]);
        },
        DeviceSample::I32 => for c in buf.chunks_exact_mut(4) {
            let v = (i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64 * g as f64) as i32;
            c.copy_from_slice(&v.to_le_bytes());
        },
    }
}
```

`output/null.rs`:
```rust
//! Test/headless output. Manual mode: the test drives the "callback" with `pull`.
//! Realtime mode: a thread pulls at (speed × real time). Both record every byte taken from the ring.
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use protocol::{BluetoothInfo, DeviceInfo, ShareMode, Transport};

use super::format::{DeviceSample, OutputFormat};
use super::{DeviceCaps, OutputBackend, OutputError, OutputStream, RingSource, StreamInfo};

#[derive(Clone, Copy, Debug)]
pub enum NullMode { Manual, Realtime { speed: f64 } }

struct Inner {
    mode: NullMode,
    caps: Mutex<DeviceCaps>,
    transport: Mutex<Transport>,
    bluetooth: Mutex<Option<BluetoothInfo>>,
    recorded: Mutex<Vec<u8>>,
    opened: Mutex<Vec<(OutputFormat, bool)>>,
    source: Mutex<Option<RingSource>>,
    fail_exclusive: AtomicBool,
}

pub struct NullBackend { inner: Arc<Inner> }
#[derive(Clone)]
pub struct NullHandle { inner: Arc<Inner> }

impl NullBackend {
    pub fn new(mode: NullMode) -> (NullBackend, NullHandle) {
        let inner = Arc::new(Inner {
            mode,
            caps: Mutex::new(DeviceCaps {
                rates: vec![44_100, 48_000, 88_200, 96_000, 176_400, 192_000],
                max_channels: 2,
                samples: vec![DeviceSample::I16, DeviceSample::I24, DeviceSample::I32, DeviceSample::F32],
                exclusive: true,
            }),
            transport: Mutex::new(Transport::Other),
            bluetooth: Mutex::new(None),
            recorded: Mutex::new(Vec::new()),
            opened: Mutex::new(Vec::new()),
            source: Mutex::new(None),
            fail_exclusive: AtomicBool::new(false),
        });
        (NullBackend { inner: inner.clone() }, NullHandle { inner })
    }
}

impl NullHandle {
    pub fn pull(&self, frames: usize) -> usize {
        let mut guard = self.inner.source.lock().unwrap();
        let Some(src) = guard.as_mut() else { return 0 };
        let bpf = src.bytes_per_frame();
        let mut buf = vec![0u8; frames * bpf];
        let n = src.fill(&mut buf);
        self.inner.recorded.lock().unwrap().extend_from_slice(&buf[..n]);
        n / bpf
    }
    pub fn recorded(&self) -> Vec<u8> { self.inner.recorded.lock().unwrap().clone() }
    pub fn clear_recorded(&self) { self.inner.recorded.lock().unwrap().clear(); }
    pub fn opened(&self) -> Vec<(OutputFormat, bool)> { self.inner.opened.lock().unwrap().clone() }
    pub fn set_caps(&self, c: DeviceCaps) { *self.inner.caps.lock().unwrap() = c; }
    pub fn set_transport(&self, t: Transport) { *self.inner.transport.lock().unwrap() = t; }
    pub fn set_bluetooth(&self, b: Option<BluetoothInfo>) { *self.inner.bluetooth.lock().unwrap() = b; }
    pub fn set_fail_exclusive(&self, on: bool) { self.inner.fail_exclusive.store(on, Relaxed); }
    pub fn simulate_device_lost(&self) {
        if let Some(src) = self.inner.source.lock().unwrap().as_ref() { src.shared().device_lost.store(true, Release); }
    }
}

pub struct NullStream { info: StreamInfo, alive: Arc<AtomicBool>, inner: Arc<Inner> }

impl OutputStream for NullStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> {
        if let NullMode::Realtime { speed } = self.inner.mode {
            let (alive, h, rate) = (self.alive.clone(), NullHandle { inner: self.inner.clone() }, self.info.format.sample_rate);
            std::thread::Builder::new().name("aurora-null-out".into()).spawn(move || {
                let per_tick = ((rate as f64 * 0.01 * speed) as usize).max(1);
                while alive.load(Relaxed) { h.pull(per_tick); std::thread::sleep(Duration::from_millis(10)); }
            }).map_err(|e| OutputError::Other(e.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for NullStream {
    fn drop(&mut self) {
        self.alive.store(false, Relaxed);
        *self.inner.source.lock().unwrap() = None;
    }
}

impl OutputBackend for NullBackend {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        let caps = self.inner.caps.lock().unwrap().clone();
        Ok(vec![DeviceInfo {
            id: "null".into(), name: "Null Output".into(), transport: *self.inner.transport.lock().unwrap(),
            is_default: true, sample_rates: caps.rates, exclusive_capable: caps.exclusive,
        }])
    }

    fn caps(&self, _device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError> {
        let caps = self.inner.caps.lock().unwrap().clone();
        if exclusive && !caps.exclusive { return Err(OutputError::Other("exclusive mode not available".into())); }
        Ok(caps)
    }

    fn open(&self, _device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
        if exclusive && self.inner.fail_exclusive.load(Relaxed) { return Err(OutputError::ExclusiveDenied("device busy".into())); }
        self.inner.opened.lock().unwrap().push((format, exclusive));
        *self.inner.source.lock().unwrap() = Some(source);
        Ok(Box::new(NullStream {
            info: StreamInfo {
                device_id: "null".into(), device_name: "Null Output".into(), transport: *self.inner.transport.lock().unwrap(),
                share: if exclusive { ShareMode::Exclusive } else { ShareMode::Shared }, format,
            },
            alive: Arc::new(AtomicBool::new(true)),
            inner: self.inner.clone(),
        }))
    }

    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> {
        (stream.transport == Transport::Bluetooth).then(|| self.inner.bluetooth.lock().unwrap().clone().unwrap_or_default())
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml output::`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core/src/output
git commit -m "feat(output): backend traits, lock-free ring with discard/gain, null backend"
```

---

### Task 12: Pipeline planning and the per-track processing chain

**Files:**
- Create: `engine/crates/engine-core/src/player/pipeline.rs`, `src/player/chain.rs`
- Modify: `engine/crates/engine-core/src/player/mod.rs` (add `pub mod chain; pub mod pipeline;`)

**Interfaces:**
- Consumes: `PcmSpec`, `SampleKind`, `PcmBlock` (Task 3); `DeviceCaps`, `OutputFormat`, `DeviceSample` (Tasks 10–11); `ChannelMap`, `StreamResampler`, `write_*`, `Ditherer` (Task 10); `DsdOutput` (Task 3).
- Produces:
```rust
// pipeline
pub enum Passthrough { None, Int, Float, Dop }
pub struct PipelinePlan {
    pub format: OutputFormat, pub exclusive: bool, pub bit_perfect: bool, pub passthrough: Passthrough,
    pub resample_from: Option<u32>, pub channel_map: ChannelMap, pub dither: bool,
    pub dsd: Option<DsdOutput>, pub notes: Vec<String>,
}
pub struct PlanInput<'a> {
    pub source: &'a PcmSpec, pub dsd_rate: Option<u32>, pub mode: OutputMode, pub transport: Transport,
    pub shared_caps: &'a DeviceCaps, pub exclusive_caps: Option<&'a DeviceCaps>,
    pub current: Option<(OutputFormat, bool)>,   // open stream (format, exclusive), for gapless continuation
}
pub fn plan_output(i: &PlanInput) -> PipelinePlan;
// chain
pub struct ProcessChain;
impl ProcessChain {
    pub fn new(plan: PipelinePlan, src: PcmSpec, reuse: Option<StreamResampler>) -> ProcessChain;
    pub fn process(&mut self, block: &PcmBlock, out: &mut Vec<u8>);
    pub fn flush(&mut self, out: &mut Vec<u8>);
    pub fn take_resampler(&mut self) -> Option<StreamResampler>;
    pub fn plan(&self) -> &PipelinePlan;
}
```
- **Rules `plan_output` must implement** (each has a test):
  1. **Bit-perfect + exclusive caps.** DSD whose DoP rate is supported → DoP. Int source whose rate is supported, whose channels fit and whose bits fit an integer format → Int passthrough. Float source with F32 supported → Float passthrough. Float source on an integer-only device → exclusive at the source rate with dither, `bit_perfect = false`, note `"Lossy source converted to integer"`.
  2. **Bit-perfect, but the device can't take the stream exclusively** (rate or channels unsupported) → processed, with note `"Device can't play {rate} Hz / {ch} ch exclusively — using processed mode"`.
  3. **Bit-perfect, no exclusive caps, Bluetooth transport** → shared passthrough at the source rate, `bit_perfect = true` (the signal path reports scope `os`).
  4. **Bit-perfect, no exclusive caps, other transport** → processed, note `"Exclusive mode unavailable — using processed mode"`.
  5. **Processed.** If `current` is a shared stream, keep its format (gapless; resample if rates differ). Otherwise use the source rate if supported, else 48000 if supported, else the first supported rate. Prefer F32, else `best_int_for(24)`, else I16. Channels = source channels if ≤ max, else 2. A mono source goes to 2 channels when the device allows it. DSD sources get `dsd = Some(Pcm)`.

- [ ] **Step 1: Write the failing tests**

Bottom of `pipeline.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::format::DeviceSample::*;

    fn caps(exclusive: bool) -> DeviceCaps {
        DeviceCaps { rates: vec![44_100, 48_000, 88_200, 96_000, 176_400, 192_000], max_channels: 2, samples: vec![I16, I24, I32, F32], exclusive }
    }
    fn int(rate: u32, ch: u16, bits: u16) -> PcmSpec { PcmSpec { sample_rate: rate, channels: ch, kind: SampleKind::Int { bits } } }
    fn input<'a>(src: &'a PcmSpec, mode: OutputMode, shared: &'a DeviceCaps, ex: Option<&'a DeviceCaps>) -> PlanInput<'a> {
        PlanInput { source: src, dsd_rate: None, mode, transport: Transport::Usb, shared_caps: shared, exclusive_caps: ex, current: None }
    }

    #[test]
    fn bit_perfect_int_uses_narrowest_lossless_format() {
        let (sh, ex) = (caps(false), caps(true));
        let s = int(96_000, 2, 24);
        let p = plan_output(&input(&s, OutputMode::BitPerfect, &sh, Some(&ex)));
        assert!(p.bit_perfect && p.exclusive);
        assert_eq!(p.passthrough, Passthrough::Int);
        assert_eq!(p.format, OutputFormat { sample_rate: 96_000, channels: 2, sample: I24 });
        let s16 = int(44_100, 2, 16);
        assert_eq!(plan_output(&input(&s16, OutputMode::BitPerfect, &sh, Some(&ex))).format.sample, I16);
    }

    #[test]
    fn dsd_goes_dop_when_rate_supported() {
        let (sh, ex) = (caps(false), caps(true));
        let s = PcmSpec { sample_rate: 88_200, channels: 2, kind: SampleKind::Float };
        let mut i = input(&s, OutputMode::BitPerfect, &sh, Some(&ex));
        i.dsd_rate = Some(2_822_400);
        let p = plan_output(&i);
        assert_eq!(p.dsd, Some(DsdOutput::Dop));
        assert_eq!(p.passthrough, Passthrough::Dop);
        assert_eq!(p.format.sample_rate, 176_400);
        assert!(p.bit_perfect);
    }

    #[test]
    fn channel_mismatch_falls_back_to_processed() {
        let (sh, ex) = (caps(false), caps(true));
        let s = int(48_000, 6, 16);
        let p = plan_output(&input(&s, OutputMode::BitPerfect, &sh, Some(&ex)));
        assert!(!p.bit_perfect && !p.exclusive);
        assert_eq!(p.format.channels, 2);
        assert!(!p.channel_map.is_identity());
        assert!(p.notes.iter().any(|n| n.contains("6 ch")), "{:?}", p.notes);
    }

    #[test]
    fn lossy_on_integer_only_exclusive_device_is_not_bit_perfect() {
        let sh = caps(false);
        let ex = DeviceCaps { samples: vec![I16, I24], ..caps(true) };
        let s = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Float };
        let p = plan_output(&input(&s, OutputMode::BitPerfect, &sh, Some(&ex)));
        assert!(!p.bit_perfect && p.exclusive && p.dither);
        assert!(p.notes.iter().any(|n| n == "Lossy source converted to integer"));
    }

    #[test]
    fn bluetooth_without_exclusive_is_bit_perfect_to_os() {
        let sh = caps(false);
        let s = int(48_000, 2, 16);
        let mut i = input(&s, OutputMode::BitPerfect, &sh, None);
        i.transport = Transport::Bluetooth;
        let p = plan_output(&i);
        assert!(p.bit_perfect && !p.exclusive);
        assert_eq!(p.passthrough, Passthrough::Int);
    }

    #[test]
    fn processed_keeps_current_shared_format_for_gapless() {
        let sh = caps(false);
        let s = int(96_000, 2, 24);
        let mut i = input(&s, OutputMode::Processed, &sh, None);
        i.current = Some((OutputFormat { sample_rate: 44_100, channels: 2, sample: F32 }, false));
        let p = plan_output(&i);
        assert_eq!(p.format.sample_rate, 44_100);
        assert_eq!(p.resample_from, Some(96_000));
        assert_eq!(p.passthrough, Passthrough::None);
    }

    #[test]
    fn processed_prefers_source_rate_then_48k() {
        let sh = DeviceCaps { rates: vec![48_000], ..caps(false) };
        let s = int(44_100, 1, 16);
        let p = plan_output(&input(&s, OutputMode::Processed, &sh, None));
        assert_eq!(p.format, OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 });
        assert_eq!(p.channel_map.describe(), "mono → stereo");
    }
}
```

Bottom of `chain.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::format::DeviceSample::*;
    use crate::pcm::*;
    use crate::player::pipeline::Passthrough;

    fn plan(pass: Passthrough, fmt: OutputFormat, resample_from: Option<u32>, map: ChannelMap) -> PipelinePlan {
        PipelinePlan { format: fmt, exclusive: false, bit_perfect: pass != Passthrough::None, passthrough: pass, resample_from, channel_map: map, dither: false, dsd: None, notes: vec![] }
    }

    #[test]
    fn int_passthrough_writes_exact_bytes() {
        let src = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Int { bits: 16 } };
        let mut c = ProcessChain::new(plan(Passthrough::Int, OutputFormat { sample_rate: 44_100, channels: 2, sample: I32 }, None, ChannelMap::Identity), src, None);
        let mut out = vec![];
        c.process(&PcmBlock { spec: src, data: PcmData::Int(vec![1 << 16, -(1 << 16)]) }, &mut out);
        assert_eq!(out, [(1i32 << 16).to_le_bytes(), (-(1i32 << 16)).to_le_bytes()].concat());
    }

    #[test]
    fn processed_maps_channels_and_resamples() {
        let src = PcmSpec { sample_rate: 44_100, channels: 1, kind: SampleKind::Float };
        let fmt = OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 };
        let mut c = ProcessChain::new(plan(Passthrough::None, fmt, Some(44_100), ChannelMap::plan(1, 2)), src, None);
        let mut out = vec![];
        c.process(&PcmBlock { spec: src, data: PcmData::Float(vec![0.25; 44_100]) }, &mut out);
        c.flush(&mut out);
        let frames = out.len() / 8;
        assert!((frames as i64 - 48_000).abs() <= 2);
    }

    #[test]
    fn resampler_can_be_handed_to_the_next_track() {
        let src = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Float };
        let fmt = OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 };
        let mut a = ProcessChain::new(plan(Passthrough::None, fmt, Some(44_100), ChannelMap::Identity), src, None);
        let r = a.take_resampler().unwrap();
        assert_eq!(r.config(), (44_100, 48_000, 2));
        let b = ProcessChain::new(plan(Passthrough::None, fmt, Some(44_100), ChannelMap::Identity), src, Some(r));
        assert!(b.plan().resample_from.is_some());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml player::`
Expected: compile errors.

- [ ] **Step 3: Implement `pipeline.rs`**

```rust
use protocol::{OutputMode, Transport};

use crate::decode::DsdOutput;
use crate::dsp::channels::ChannelMap;
use crate::output::format::{DeviceSample, OutputFormat};
use crate::output::DeviceCaps;
use crate::pcm::{PcmSpec, SampleKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passthrough { None, Int, Float, Dop }

#[derive(Clone, Debug, PartialEq)]
pub struct PipelinePlan {
    pub format: OutputFormat,
    pub exclusive: bool,
    pub bit_perfect: bool,
    pub passthrough: Passthrough,
    pub resample_from: Option<u32>,
    pub channel_map: ChannelMap,
    pub dither: bool,
    pub dsd: Option<DsdOutput>,
    pub notes: Vec<String>,
}

pub struct PlanInput<'a> {
    pub source: &'a PcmSpec,
    pub dsd_rate: Option<u32>,
    pub mode: OutputMode,
    pub transport: Transport,
    pub shared_caps: &'a DeviceCaps,
    pub exclusive_caps: Option<&'a DeviceCaps>,
    pub current: Option<(OutputFormat, bool)>,
}

fn passthrough(fmt: OutputFormat, exclusive: bool, kind: Passthrough, dsd: Option<DsdOutput>) -> PipelinePlan {
    PipelinePlan { format: fmt, exclusive, bit_perfect: true, passthrough: kind, resample_from: None, channel_map: ChannelMap::Identity, dither: false, dsd, notes: vec![] }
}

/// Lossless passthrough onto `caps` at the source rate, if possible.
fn try_passthrough(i: &PlanInput, caps: &DeviceCaps, exclusive: bool) -> Option<PipelinePlan> {
    let s = i.source;
    if s.channels > caps.max_channels { return None; }
    if let Some(dsd_rate) = i.dsd_rate {
        let dop_rate = dsd_rate / 16;
        let sample = caps.best_int_for(24)?;
        return (exclusive && caps.supports_rate(dop_rate)).then(|| passthrough(OutputFormat { sample_rate: dop_rate, channels: s.channels, sample }, true, Passthrough::Dop, Some(DsdOutput::Dop)));
    }
    if !caps.supports_rate(s.sample_rate) { return None; }
    match s.kind {
        SampleKind::Int { bits } => caps.best_int_for(bits).map(|sample| passthrough(OutputFormat { sample_rate: s.sample_rate, channels: s.channels, sample }, exclusive, Passthrough::Int, None)),
        SampleKind::Float if caps.supports(DeviceSample::F32) => Some(passthrough(OutputFormat { sample_rate: s.sample_rate, channels: s.channels, sample: DeviceSample::F32 }, exclusive, Passthrough::Float, None)),
        _ => None,
    }
}

fn processed(i: &PlanInput, caps: &DeviceCaps, exclusive: bool, keep: Option<OutputFormat>) -> PipelinePlan {
    let s = i.source;
    let format = keep.unwrap_or_else(|| {
        let sample_rate = if caps.supports_rate(s.sample_rate) { s.sample_rate } else if caps.supports_rate(48_000) { 48_000 } else { caps.rates.first().copied().unwrap_or(48_000) };
        let sample = if caps.supports(DeviceSample::F32) { DeviceSample::F32 } else { caps.best_int_for(24).unwrap_or(DeviceSample::I16) };
        let channels = if s.channels == 1 && caps.max_channels >= 2 { 2 } else if s.channels <= caps.max_channels { s.channels } else { 2 };
        OutputFormat { sample_rate, channels, sample }
    });
    PipelinePlan {
        format,
        exclusive,
        bit_perfect: false,
        passthrough: Passthrough::None,
        resample_from: (s.sample_rate != format.sample_rate).then_some(s.sample_rate),
        channel_map: ChannelMap::plan(s.channels, format.channels),
        dither: format.sample.is_int(),
        dsd: i.dsd_rate.map(|_| DsdOutput::Pcm),
        notes: vec![],
    }
}

pub fn plan_output(i: &PlanInput) -> PipelinePlan {
    let s = i.source;
    if i.mode == OutputMode::BitPerfect {
        match i.exclusive_caps {
            Some(ex) => {
                if let Some(p) = try_passthrough(i, ex, true) { return p; }
                if s.kind == SampleKind::Float && i.dsd_rate.is_none() && ex.supports_rate(s.sample_rate) && s.channels <= ex.max_channels {
                    let mut p = processed(i, ex, true, None);
                    p.notes.push("Lossy source converted to integer".into());
                    return p;
                }
                let mut p = processed(i, i.shared_caps, false, None);
                p.notes.push(format!("Device can't play {} Hz / {} ch exclusively — using processed mode", s.sample_rate, s.channels));
                return p;
            }
            None if i.transport == Transport::Bluetooth => {
                if let Some(p) = try_passthrough(i, i.shared_caps, false) { return p; }
            }
            None => {
                let mut p = processed(i, i.shared_caps, false, None);
                p.notes.push("Exclusive mode unavailable — using processed mode".into());
                return p;
            }
        }
    }
    let keep = i.current.filter(|&(_, ex)| !ex).map(|(f, _)| f);
    processed(i, i.shared_caps, false, keep)
}
```

- [ ] **Step 4: Implement `chain.rs`**

```rust
use crate::dsp::convert::{write_f32_passthrough, write_f64, write_int_passthrough, Ditherer};
use crate::dsp::resample::StreamResampler;
use crate::pcm::{PcmBlock, PcmData, PcmSpec};
use crate::player::pipeline::{Passthrough, PipelinePlan};

/// Turns decoded blocks into device-format bytes according to a `PipelinePlan`.
/// Volume isn't applied here: the ring applies gain in the callback so changes are instant.
pub struct ProcessChain {
    plan: PipelinePlan,
    src: PcmSpec,
    resampler: Option<StreamResampler>,
    dither: Ditherer,
    f: Vec<f64>,
    mapped: Vec<f64>,
    resampled: Vec<f64>,
}

impl ProcessChain {
    pub fn new(plan: PipelinePlan, src: PcmSpec, reuse: Option<StreamResampler>) -> Self {
        let resampler = plan.resample_from.and_then(|from| {
            let want = (from, plan.format.sample_rate, plan.format.channels as usize);
            match reuse {
                Some(r) if r.config() == want => Some(r),
                _ => StreamResampler::new(want.0, want.1, want.2).ok(),
            }
        });
        Self { plan, src, resampler, dither: Ditherer::new(0x5EED), f: vec![], mapped: vec![], resampled: vec![] }
    }

    pub fn plan(&self) -> &PipelinePlan { &self.plan }
    pub fn take_resampler(&mut self) -> Option<StreamResampler> { self.resampler.take() }

    pub fn process(&mut self, block: &PcmBlock, out: &mut Vec<u8>) {
        debug_assert_eq!(block.spec.channels, self.src.channels);
        match (self.plan.passthrough, &block.data) {
            (Passthrough::Int | Passthrough::Dop, PcmData::Int(v)) => return write_int_passthrough(v, self.plan.format.sample, out),
            (Passthrough::Float, PcmData::Float(v)) => return write_f32_passthrough(v, out),
            _ => {}
        }
        self.f.clear();
        block.to_f64(&mut self.f);
        self.mapped.clear();
        self.plan.channel_map.apply(&self.f, &mut self.mapped);
        let samples: &[f64] = match self.resampler.as_mut() {
            Some(r) => { self.resampled.clear(); r.process(&self.mapped, &mut self.resampled); &self.resampled }
            None => &self.mapped,
        };
        let dither = self.plan.dither.then_some(&mut self.dither);
        write_f64(samples, self.plan.format.sample, dither, out);
    }

    pub fn flush(&mut self, out: &mut Vec<u8>) {
        if let Some(r) = self.resampler.as_mut() {
            self.resampled.clear();
            r.flush(&mut self.resampled);
            let dither = self.plan.dither.then_some(&mut self.dither);
            write_f64(&self.resampled, self.plan.format.sample, dither, out);
        }
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml player::`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/engine-core/src/player
git commit -m "feat(player): output pipeline planning and processing chain"
```

---

### Task 13: Signal-path builder and Bluetooth codec probe

**Files:**
- Create: `engine/crates/engine-core/src/player/signal_path.rs`, `src/probe/pipewire.rs`
- Modify: `src/probe/mod.rs`, `src/player/mod.rs` (add `pub mod signal_path;`)

**Interfaces:**
- Consumes: `SourceInfo`, `source_summary` (Task 9); `PipelinePlan` (Task 12); `StreamInfo` (Task 11).
- Produces:
```rust
pub fn build_signal_path(src: &SourceInfo, plan: &PipelinePlan, stream: &StreamInfo, volume: f32, bluetooth: Option<BluetoothInfo>, underruns: u64) -> SignalPath;
pub fn dsd_label(rate: u32) -> String;                       // 2822400 → "DSD64"
// probe
pub fn pipewire::parse_pw_dump(json: &str, hint: &str) -> Option<BluetoothInfo>;
#[cfg(target_os = "linux")] pub fn pipewire::query(hint: &str) -> Option<BluetoothInfo>;
pub fn probe::bluetooth_for(stream: &StreamInfo) -> Option<BluetoothInfo>;   // used by SystemBackend::bluetooth (Task 15)
```
- **Scope rule:** `bitPerfectScope = "device"` only when bit-perfect **and** exclusive **and** the transport isn't Bluetooth. Otherwise, if bit-perfect, `"os"`.

- [ ] **Step 1: Write the failing tests**

Bottom of `signal_path.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::SourceInfo;
    use crate::dsp::channels::ChannelMap;
    use crate::output::format::{DeviceSample, OutputFormat};
    use crate::pcm::{PcmSpec, SampleKind};
    use crate::player::pipeline::Passthrough;
    use protocol::*;

    fn src() -> SourceInfo {
        SourceInfo { container: "flac".into(), codec: Codec::Flac, bitrate_kbps: Some(2400), spec: PcmSpec { sample_rate: 96_000, channels: 2, kind: SampleKind::Int { bits: 24 } }, total_frames: Some(1), gapless: None, decoder_name: "symphonia", dsd_rate: None }
    }
    fn plan(bp: bool, ex: bool, resample: Option<u32>) -> PipelinePlan {
        PipelinePlan { format: OutputFormat { sample_rate: 96_000, channels: 2, sample: DeviceSample::I24 }, exclusive: ex, bit_perfect: bp, passthrough: if bp { Passthrough::Int } else { Passthrough::None }, resample_from: resample, channel_map: ChannelMap::Identity, dither: !bp, dsd: None, notes: vec!["n".into()] }
    }
    fn stream(t: Transport, share: ShareMode) -> StreamInfo {
        StreamInfo { device_id: "d".into(), device_name: "DAC".into(), transport: t, share, format: OutputFormat { sample_rate: 96_000, channels: 2, sample: DeviceSample::I24 } }
    }

    #[test]
    fn wired_exclusive_bit_perfect_is_device_scope() {
        let sp = build_signal_path(&src(), &plan(true, true, None), &stream(Transport::Usb, ShareMode::Exclusive), 0.5, None, 0);
        assert!(sp.bit_perfect);
        assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Device));
        assert!(sp.bluetooth.is_none());
        let vol = sp.stages.iter().find(|s| s.kind == StageKind::Volume).unwrap();
        assert!(!vol.active);
        assert_eq!(vol.detail, "bypassed (bit-perfect)");
        assert_eq!(sp.output.format, "s24");
        assert_eq!(sp.notes, vec!["n".to_string()]);
    }

    #[test]
    fn bluetooth_is_os_scope_and_always_has_bluetooth_block() {
        let sp = build_signal_path(&src(), &plan(true, true, None), &stream(Transport::Bluetooth, ShareMode::Exclusive), 1.0, None, 3);
        assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Os));
        assert_eq!(sp.bluetooth, Some(BluetoothInfo::default()));
        assert_eq!(sp.underruns, 3);
    }

    #[test]
    fn processed_shows_resampler_and_volume() {
        let sp = build_signal_path(&src(), &plan(false, false, Some(44_100)), &stream(Transport::Builtin, ShareMode::Shared), 0.82, None, 0);
        assert!(!sp.bit_perfect && sp.bit_perfect_scope.is_none());
        let rs = sp.stages.iter().find(|s| s.kind == StageKind::Resampler).unwrap();
        assert!(rs.active);
        assert_eq!(rs.detail, "44.1 → 96 kHz (sinc)");
        assert_eq!(sp.stages.iter().find(|s| s.kind == StageKind::Volume).unwrap().detail, "82%");
    }

    #[test]
    fn dsd_labels() { assert_eq!(dsd_label(2_822_400), "DSD64"); assert_eq!(dsd_label(11_289_600), "DSD256"); }
}
```

Bottom of `probe/pipewire.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id": 40, "type": "PipeWire:Interface:Device", "info": {"props": {"device.name": "bluez_card.AC_80_0A", "bluez5.a2dp.ldac.quality": "hq"}}},
      {"id": 55, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink", "node.name": "bluez_output.AC_80_0A.1", "node.description": "WH-1000XM5",
        "device.id": 40, "api.bluez5.codec": "ldac", "api.bluez5.profile": "a2dp-sink"}}},
      {"id": 60, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.usb-dac", "node.description": "USB DAC"}}}
    ]"#;

    #[test]
    fn finds_ldac_with_quality_from_device_props() {
        let b = parse_pw_dump(DUMP, "WH-1000XM5").unwrap();
        assert_eq!(b.codec.as_deref(), Some("ldac"));
        assert_eq!(b.quality.as_deref(), Some("hq"));
        assert_eq!(b.bitrate_kbps, Some(990));
    }

    #[test]
    fn single_bluez_sink_matches_without_hint() {
        assert_eq!(parse_pw_dump(DUMP, "unrelated").unwrap().codec.as_deref(), Some("ldac"));
    }

    #[test]
    fn non_bluetooth_dump_is_none() {
        assert!(parse_pw_dump(r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Audio/Sink"}}}]"#, "x").is_none());
        assert!(parse_pw_dump("not json", "x").is_none());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml signal_path pipewire`
Expected: compile errors.

- [ ] **Step 3: Implement `signal_path.rs`**

```rust
use protocol::*;

use crate::decode::{source_summary, SourceInfo};
use crate::output::StreamInfo;
use crate::player::pipeline::{Passthrough, PipelinePlan};

pub fn dsd_label(rate: u32) -> String { format!("DSD{}", rate / 44_100) }

fn khz(r: u32) -> String {
    let k = r as f64 / 1000.0;
    if (k - k.round()).abs() < 1e-9 { format!("{}", k as u32) } else { format!("{k:.1}") }
}

pub fn build_signal_path(src: &SourceInfo, plan: &PipelinePlan, stream: &StreamInfo, volume: f32, bluetooth: Option<BluetoothInfo>, underruns: u64) -> SignalPath {
    let mut stages = Vec::new();
    if let Some(rate) = src.dsd_rate {
        let detail = match plan.passthrough {
            Passthrough::Dop => format!("{} as DoP @ {} kHz", dsd_label(rate), khz(plan.format.sample_rate)),
            _ => format!("{} → PCM {} kHz (−6 dB)", dsd_label(rate), khz(src.spec.sample_rate)),
        };
        stages.push(Stage { kind: StageKind::Dsd, active: true, detail });
    }
    stages.push(Stage { kind: StageKind::Channels, active: !plan.channel_map.is_identity(), detail: plan.channel_map.describe() });
    stages.push(Stage {
        kind: StageKind::Resampler,
        active: plan.resample_from.is_some(),
        detail: plan.resample_from.map_or("rate matched".into(), |from| format!("{} → {} kHz (sinc)", khz(from), khz(plan.format.sample_rate))),
    });
    let processed = plan.passthrough == Passthrough::None;
    stages.push(Stage {
        kind: StageKind::Volume,
        active: processed && volume < 1.0,
        detail: if processed { format!("{}%", (volume * 100.0).round() as u32) } else { "bypassed (bit-perfect)".into() },
    });
    stages.push(Stage {
        kind: StageKind::Dither,
        active: plan.dither,
        detail: if plan.dither { format!("TPDF → {}-bit", plan.format.sample.bits()) } else { "off".into() },
    });

    let is_bt = stream.transport == Transport::Bluetooth;
    let scope = plan.bit_perfect.then(|| if plan.exclusive && !is_bt { BitPerfectScope::Device } else { BitPerfectScope::Os });
    SignalPath {
        source: source_summary(src),
        decoder: DecoderSummary { name: src.decoder_name.into(), gapless: src.gapless.map(|g| GaplessSummary { delay: g.delay, padding: g.padding }) },
        stages,
        output: OutputSummary {
            device_id: stream.device_id.clone(), name: stream.device_name.clone(), transport: stream.transport,
            mode: stream.share, sample_rate: stream.format.sample_rate, format: stream.format.sample.wire_name().into(),
        },
        bluetooth: is_bt.then(|| bluetooth.unwrap_or_default()),
        bit_perfect: plan.bit_perfect,
        bit_perfect_scope: scope,
        underruns,
        notes: plan.notes.clone(),
    }
}
```

- [ ] **Step 4: Implement the probe**

`probe/mod.rs`:
```rust
pub mod pipewire;

use protocol::{BluetoothInfo, Transport};

use crate::output::StreamInfo;

/// Codec details for a Bluetooth output. `codec: None` means "not reported by the OS". Never guess.
pub fn bluetooth_for(stream: &StreamInfo) -> Option<BluetoothInfo> {
    if stream.transport != Transport::Bluetooth { return None; }
    #[cfg(target_os = "linux")]
    if let Some(info) = pipewire::query(&stream.device_name) { return Some(info); }
    Some(BluetoothInfo::default())
}
```

`probe/pipewire.rs` (above the tests):
```rust
use protocol::BluetoothInfo;
use serde_json::Value;

fn props(obj: &Value) -> Option<&serde_json::Map<String, Value>> { obj.get("info")?.get("props")?.as_object() }

fn ldac_kbps(q: &str) -> Option<u32> { match q { "hq" => Some(990), "sq" => Some(660), "mq" => Some(330), _ => None } }

pub fn parse_pw_dump(json: &str, hint: &str) -> Option<BluetoothInfo> {
    let all: Vec<Value> = serde_json::from_str(json).ok()?;
    let sinks: Vec<&Value> = all.iter().filter(|o| {
        o.get("type").and_then(Value::as_str) == Some("PipeWire:Interface:Node")
            && props(o).is_some_and(|p| p.get("media.class").and_then(Value::as_str) == Some("Audio/Sink") && p.contains_key("api.bluez5.codec"))
    }).collect();
    let matches_hint = |o: &Value| props(o).is_some_and(|p| ["node.description", "node.nick", "node.name"].iter().any(|k| p.get(*k).and_then(Value::as_str).is_some_and(|v| !hint.is_empty() && (v.contains(hint) || hint.contains(v)))));
    let node: &Value = sinks.iter().copied().find(|o| matches_hint(o)).or_else(|| (sinks.len() == 1).then(|| sinks[0]))?;
    let p = props(node)?;
    let codec = p.get("api.bluez5.codec").and_then(Value::as_str)?.to_ascii_lowercase();
    let device_id = p.get("device.id").and_then(Value::as_u64);
    let quality = p.get("bluez5.a2dp.ldac.quality").and_then(Value::as_str).map(str::to_owned).or_else(|| {
        let dev = all.iter().find(|o| o.get("id").and_then(Value::as_u64) == device_id && device_id.is_some())?;
        props(dev)?.get("bluez5.a2dp.ldac.quality").and_then(Value::as_str).map(str::to_owned)
    });
    let bitrate_kbps = if codec == "ldac" { quality.as_deref().and_then(ldac_kbps) } else { None };
    Some(BluetoothInfo { codec: Some(codec), quality, bitrate_kbps })
}

#[cfg(target_os = "linux")]
pub fn query(hint: &str) -> Option<BluetoothInfo> {
    let out = std::process::Command::new("pw-dump").output().ok()?;
    parse_pw_dump(&String::from_utf8_lossy(&out.stdout), hint)
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml signal_path pipewire`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/engine-core/src
git commit -m "feat: signal-path builder and PipeWire Bluetooth codec probe"
```

---

### Task 14: Player engine — queue, gapless feeding, seek, errors, device loss

**Files:**
- Modify: `engine/crates/engine-core/src/player/mod.rs`
- Test: `engine/crates/engine-core/tests/player.rs`

**Interfaces:**
- Consumes: `decode::open` (Task 9), `ProcessChain`, `plan_output`, `PlanInput`, `Passthrough` (Task 12), `build_signal_path` (Task 13), `ring_pair`, `OutputBackend`, `OutputStream`, `StreamInfo`, `DeviceCaps` (Task 11), `volume_gain` (Task 10).
- Produces:
```rust
pub enum Command { SetQueue { paths: Vec<PathBuf>, start: usize }, Play(Option<u32>), Pause, Toggle, Stop, Next, Prev, Seek(u64), SetVolume(f32), SetMode(OutputMode), SetDevice(Option<String>), Shutdown }
pub enum PlayerEvent { State(PlayerState), SignalPath(SignalPath), Queue(QueueChanged), Error(EngineErrorEvent), DevicesChanged(Vec<DeviceInfo>), Ended }
pub struct PlayerConfig { pub buffer_secs: f64, pub preroll_secs: f64 }   // Default: 2.0, 10.0
pub struct PlayerHandle; impl PlayerHandle { pub fn send(&self, c: Command); pub fn state(&self) -> PlayerState; pub fn shutdown(self); }
pub fn spawn_player(backend: Arc<dyn OutputBackend>, cfg: PlayerConfig, events: Sender<PlayerEvent>) -> PlayerHandle;
```
- **Behaviour contract:**
  - `SetQueue` stops playback and selects `start`; it doesn't start playing.
  - `Play(None)` resumes when paused, or starts at the selected entry when stopped.
  - The next track's decoder is pre-opened when ≤ `preroll_secs` remain. At EOF the next track keeps feeding the same stream if `plan_output` returns the same format and exclusivity (gapless). Otherwise the ring drains, the stream reopens and the plan gets the note `"Rate change: brief gap"` (or `"Format change: brief gap"` when the rate is the same).
  - `PlayerEvent::State` is emitted on every change and at 4 Hz while playing. `SignalPath` is emitted when a track becomes audible and whenever the underrun count changes (at most once a second).
  - A track that fails to open or decode emits `Error` (with `path`) and is skipped.
  - When the device is lost, the player pauses, switches to the default device, and emits `Error(DEVICE_UNAVAILABLE)` and `DevicesChanged`. `Play` then resumes at the same position.
  - `Prev` restarts the current track if the position is > 3 s.
  - Seeking at or past the end of a track starts the next track.
  - When the queue ends, the player emits `Ended`, then `State(Stopped)`.
  - **Buffer growth (spec §6):** 3 or more underruns within 10 s grow the ring buffer by 1 s, up to 8 s. The new size applies to the next stream open, and the current signal path gets the note `"Buffer increased to N s after repeated underruns"`.

- [ ] **Step 1: Write the failing tests**

`tests/player.rs`:
```rust
mod common;
use common::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use engine_core::output::null::{NullBackend, NullHandle, NullMode};
use engine_core::player::{spawn_player, Command, PlayerConfig, PlayerEvent, PlayerHandle};
use protocol::*;

fn setup(mode: OutputMode) -> (PlayerHandle, NullHandle, Receiver<PlayerEvent>) {
    let (backend, h) = NullBackend::new(NullMode::Manual);
    let (tx, rx) = crossbeam_channel::unbounded();
    let p = spawn_player(Arc::new(backend), PlayerConfig::default(), tx);
    p.send(Command::SetMode(mode));
    (p, h, rx)
}

fn queue(p: &PlayerHandle, names: &[&str]) {
    p.send(Command::SetQueue { paths: names.iter().map(|n| fixture(n)).collect(), start: 0 });
    p.send(Command::Play(None));
}

/// Pulls audio until the player reports Ended. Returns every event seen.
fn run_to_end(h: &NullHandle, rx: &Receiver<PlayerEvent>) -> Vec<PlayerEvent> {
    let start = Instant::now();
    let mut events = vec![];
    loop {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out; events: {events:?}");
        let n = h.pull(4096);
        events.extend(rx.try_iter());
        if events.iter().any(|e| matches!(e, PlayerEvent::Ended)) { return events; }
        if n == 0 { std::thread::sleep(Duration::from_millis(1)); }
    }
}

fn wait_for(p: &PlayerHandle, cond: impl Fn(&PlayerState) -> bool) {
    let start = Instant::now();
    while !cond(&p.state()) { assert!(start.elapsed() < Duration::from_secs(10), "state: {:?}", p.state()); std::thread::sleep(Duration::from_millis(2)); }
}

fn pull_frames(h: &NullHandle, frames: usize) {
    let mut got = 0;
    let start = Instant::now();
    while got < frames { got += h.pull((frames - got).min(4096)); assert!(start.elapsed() < Duration::from_secs(10)); std::thread::sleep(Duration::from_micros(200)); }
}

fn f32s(bytes: &[u8]) -> Vec<f32> { bytes.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect() }

#[test]
fn bit_perfect_bytes_match_source_exactly() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    queue(&p, &["flac24_96k.flac"]);
    let events = run_to_end(&h, &rx);
    let expected: Vec<u8> = read_s32le("pcm24_96k.s32le").iter().flat_map(|s| (s >> 8).to_le_bytes()[..3].to_vec()).collect();
    assert_eq!(h.recorded(), expected);
    let sp = events.iter().find_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s.clone()) } else { None }).unwrap();
    assert!(sp.bit_perfect);
    assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Device));
    assert_eq!(sp.output.format, "s24");
}

#[test]
fn gapless_pair_is_sample_exact_in_processed_mode() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le"));
    assert_eq!(h.opened().len(), 1);
}

#[test]
fn rate_change_in_bit_perfect_reopens_with_note() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    queue(&p, &["flac16_44k.flac", "flac24_96k.flac"]);
    let events = run_to_end(&h, &rx);
    let rates: Vec<u32> = h.opened().iter().map(|(f, _)| f.sample_rate).collect();
    assert_eq!(rates, vec![44_100, 96_000]);
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::SignalPath(s) if s.notes.iter().any(|n| n == "Rate change: brief gap"))));
    let expected_len = 88_200 * 2 * 2 + 192_000 * 2 * 3;
    assert_eq!(h.recorded().len(), expected_len);
}

#[test]
fn processed_mode_resamples_second_track_without_reopening() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac", "flac24_96k.flac"]);
    run_to_end(&h, &rx);
    assert_eq!(h.opened().len(), 1);
    let frames = h.recorded().len() / 8;
    assert!((frames as i64 - 176_400).abs() <= 4, "{frames}");
}

#[test]
fn seek_is_sample_accurate() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 4_410);
    p.send(Command::Seek(1_000));
    wait_for(&p, |s| s.position_ms == 1_000);
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("pcm16_44k.f32le")[44_100 * 2..]);
}

#[test]
fn seek_during_preroll_discards_next() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    pull_frames(&h, 44_100);
    std::thread::sleep(Duration::from_millis(300)); // let the ring refill well into gap_b
    p.send(Command::Seek(500));
    wait_for(&p, |s| s.position_ms == 500 && s.queue_index == Some(0));
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le")[22_050 * 2..]);
}

#[test]
fn seek_past_end_advances() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["short_a.flac", "short_b.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Seek(5_000));
    wait_for(&p, |s| s.queue_index == Some(1));
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(h.recorded().len() / 8, 22_050);
}

#[test]
fn short_tracks_play_back_to_back() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["short_a.flac", "short_b.flac"]);
    run_to_end(&h, &rx);
    assert_eq!(h.recorded().len() / 8, 2 * 22_050);
}

#[test]
fn empty_file_is_skipped() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["empty.mp3", "short_a.flac"]);
    let events = run_to_end(&h, &rx);
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::Error(err) if err.name == "UNSUPPORTED_FORMAT" && err.path.as_deref().is_some_and(|p| p.ends_with("empty.mp3")))));
    assert_eq!(h.recorded().len() / 8, 22_050);
}

#[test]
fn pause_holds_position_and_resume_continues() {
    let (p, h, _rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 4_410);
    p.send(Command::Pause);
    wait_for(&p, |s| s.status == PlaybackStatus::Paused);
    let before = h.recorded().len();
    for _ in 0..10 { h.pull(4096); }
    assert_eq!(h.recorded().len(), before);
    assert!((p.state().position_ms as i64 - 100).abs() <= 2);
    p.send(Command::Play(None));
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    pull_frames(&h, 100);
}

#[test]
fn volume_scales_processed_output_only() {
    let (p, h, _rx) = setup(OutputMode::Processed);
    p.send(Command::SetVolume(0.5));
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 44_100);
    let got = f32s(&h.recorded());
    let reference = read_f32le("pcm16_44k.f32le");
    for i in (0..got.len()).step_by(997) { assert!((got[i] - reference[i] * 0.25).abs() < 1e-6); }
}

#[test]
fn device_loss_pauses_and_play_resumes_on_default() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 4_410);
    h.simulate_device_lost();
    wait_for(&p, |s| s.status == PlaybackStatus::Paused);
    let events: Vec<_> = rx.try_iter().collect();
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::Error(err) if err.name == "DEVICE_UNAVAILABLE")));
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::DevicesChanged(_))));
    p.send(Command::Play(None));
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    assert_eq!(h.opened().len(), 2);
    assert!((p.state().position_ms as i64 - 100).abs() <= 2);
}

#[test]
fn exclusive_denied_falls_back_to_shared_with_note() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_fail_exclusive(true);
    queue(&p, &["short_a.flac"]);
    let events = run_to_end(&h, &rx);
    assert_eq!(h.opened(), vec![(h.opened()[0].0, false)]);
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::SignalPath(s) if !s.bit_perfect && s.notes.iter().any(|n| n.contains("device busy")))));
}

#[test]
fn bluetooth_bit_perfect_reports_os_scope() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_transport(Transport::Bluetooth);
    queue(&p, &["short_a.flac"]);
    let events = run_to_end(&h, &rx);
    let sp = events.iter().find_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s.clone()) } else { None }).unwrap();
    assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Os));
    assert_eq!(sp.bluetooth.unwrap().codec, None);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test player`
Expected: compile errors (`spawn_player` not found).

- [ ] **Step 3: Implement `player/mod.rs`**

```rust
pub mod chain;
pub mod pipeline;
pub mod signal_path;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering::*;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use protocol::*;

use crate::decode::{self, DecodeError, TrackDecoder};
use crate::dsp::volume::volume_gain;
use crate::output::format::OutputFormat;
use crate::output::ring::{ring_pair, StreamShared};
use crate::output::{DeviceCaps, OutputBackend, OutputError, OutputStream, StreamInfo};
use chain::ProcessChain;
use pipeline::{plan_output, Passthrough, PipelinePlan, PlanInput};
use signal_path::build_signal_path;

const MIN_FREE_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub enum Command {
    SetQueue { paths: Vec<PathBuf>, start: usize },
    Play(Option<u32>), Pause, Toggle, Stop, Next, Prev,
    Seek(u64), SetVolume(f32), SetMode(OutputMode), SetDevice(Option<String>), Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    State(PlayerState), SignalPath(SignalPath), Queue(QueueChanged), Error(EngineErrorEvent), DevicesChanged(Vec<DeviceInfo>), Ended,
}

#[derive(Clone, Debug)]
pub struct PlayerConfig { pub buffer_secs: f64, pub preroll_secs: f64 }
impl Default for PlayerConfig { fn default() -> Self { Self { buffer_secs: 2.0, preroll_secs: 10.0 } } }

pub struct PlayerHandle { tx: Sender<Command>, state: Arc<Mutex<PlayerState>>, join: Option<JoinHandle<()>> }

impl PlayerHandle {
    pub fn send(&self, c: Command) { let _ = self.tx.send(c); }
    pub fn state(&self) -> PlayerState { self.state.lock().unwrap().clone() }
    pub fn shutdown(mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(j) = self.join.take() { let _ = j.join(); }
    }
}

impl Drop for PlayerHandle {
    fn drop(&mut self) { let _ = self.tx.send(Command::Shutdown); }
}

pub fn spawn_player(backend: Arc<dyn OutputBackend>, cfg: PlayerConfig, events: Sender<PlayerEvent>) -> PlayerHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let state = Arc::new(Mutex::new(PlayerState { volume: 1.0, ..Default::default() }));
    let st = state.clone();
    let join = std::thread::Builder::new()
        .name("aurora-player".into())
        .spawn(move || Player::new(backend, cfg, events, st).run(rx))
        .expect("spawn player thread");
    PlayerHandle { tx, state, join: Some(join) }
}

struct Entry { id: u32, path: PathBuf, title: String }

struct Active {
    stream: Box<dyn OutputStream>,
    prod: rtrb::Producer<u8>,
    shared: Arc<StreamShared>,
    info: StreamInfo,
    written_bytes: u64,
    carry: Vec<u8>,
    carry_pos: usize,
}

impl Active {
    fn bpf(&self) -> u64 { self.info.format.bytes_per_frame() as u64 }
    fn written_frames(&self) -> u64 { self.written_bytes / self.bpf() }
    fn exclusive(&self) -> bool { self.info.share == ShareMode::Exclusive }
}

struct Feeding { idx: usize, dec: Box<dyn TrackDecoder>, chain: ProcessChain, decoded: u64, eof: bool }

struct Marker { start: u64, idx: usize, start_ms: u64, out_rate: u32, signal: SignalPath, duration_ms: Option<u64> }

struct Reopen { idx: usize, dec: Box<dyn TrackDecoder>, plan: PipelinePlan }

struct Player {
    backend: Arc<dyn OutputBackend>,
    cfg: PlayerConfig,
    events: Sender<PlayerEvent>,
    shared_state: Arc<Mutex<PlayerState>>,
    queue: Vec<Entry>,
    next_id: u32,
    cur: usize,
    status: PlaybackStatus,
    volume: f32,
    mode: OutputMode,
    device: Option<String>,
    active: Option<Active>,
    feeding: Option<Feeding>,
    prepared: Option<(usize, Box<dyn TrackDecoder>)>,
    markers: VecDeque<Marker>,
    reopen: Option<Reopen>,
    end_pending: bool,
    shown_idx: Option<usize>,
    resume_ms: Option<u64>,
    last_tick: Instant,
    last_underruns: (u64, Instant),
    last_devices: (Vec<DeviceInfo>, Instant),
    buffer_secs: f64,
    underrun_times: VecDeque<Instant>,
}

fn title_of(p: &std::path::Path) -> String { p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() }

/// Ring size after `recent` underruns within the last 10 s (spec §6: +1 s per burst, max 8 s).
pub fn grown_buffer(current: f64, recent: usize) -> f64 {
    if recent >= 3 { (current + 1.0).min(8.0) } else { current }
}

#[cfg(test)]
mod tests {
    use super::grown_buffer;
    #[test]
    fn buffer_grows_on_bursts_and_caps_at_eight() {
        assert_eq!(grown_buffer(2.0, 2), 2.0);
        assert_eq!(grown_buffer(2.0, 3), 3.0);
        assert_eq!(grown_buffer(7.5, 5), 8.0);
        assert_eq!(grown_buffer(8.0, 9), 8.0);
    }
}

impl Player {
    fn new(backend: Arc<dyn OutputBackend>, cfg: PlayerConfig, events: Sender<PlayerEvent>, shared_state: Arc<Mutex<PlayerState>>) -> Self {
        let devices = backend.devices().unwrap_or_default();
        Self {
            backend, events, shared_state, queue: vec![], next_id: 1, cur: 0, status: PlaybackStatus::Stopped,
            volume: 1.0, mode: OutputMode::Processed, device: None, active: None, feeding: None, prepared: None,
            markers: VecDeque::new(), reopen: None, end_pending: false, shown_idx: None, resume_ms: None,
            last_tick: Instant::now(), last_underruns: (0, Instant::now()), last_devices: (devices, Instant::now()),
            buffer_secs: cfg.buffer_secs, underrun_times: VecDeque::new(), cfg,
        }
    }

    fn run(mut self, rx: Receiver<Command>) {
        loop {
            let timeout = Duration::from_millis(if self.status == PlaybackStatus::Playing { 2 } else { 50 });
            match rx.recv_timeout(timeout) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self.status == PlaybackStatus::Playing { self.pump(); }
            self.update_position();
            self.check_device();
        }
        self.active = None;
    }

    // ---------- commands ----------

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::SetQueue { paths, start } => {
                self.stop_internal();
                self.queue = paths.into_iter().map(|path| { let id = self.next_id; self.next_id += 1; Entry { id, title: title_of(&path), path } }).collect();
                self.cur = start.min(self.queue.len().saturating_sub(1));
                self.emit_queue();
                self.publish(true);
            }
            Command::Play(Some(id)) => if let Some(i) = self.queue.iter().position(|e| e.id == id) { self.start_at(i, 0) },
            Command::Play(None) => match self.status {
                PlaybackStatus::Paused => self.resume(),
                PlaybackStatus::Stopped if !self.queue.is_empty() => self.start_at(self.cur, 0),
                _ => {}
            },
            Command::Pause => self.pause(),
            Command::Toggle => if self.status == PlaybackStatus::Playing { self.pause() } else { self.handle(Command::Play(None)) },
            Command::Stop => { self.stop_internal(); self.publish(true); }
            Command::Next => { if self.cur + 1 < self.queue.len() { self.start_at(self.cur + 1, 0) } else { self.stop_internal(); self.publish(true) } }
            Command::Prev => { let target = if self.position_ms() > 3000 { self.cur } else { self.cur.saturating_sub(1) }; if !self.queue.is_empty() { self.start_at(target, 0) } }
            Command::Seek(ms) => self.seek(ms),
            Command::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                if let Some(a) = &self.active { a.shared.set_gain(volume_gain(self.volume) as f32); }
                if let Some(m) = self.markers.front_mut() {
                    if let Some(vol) = m.signal.stages.iter_mut().find(|s| s.kind == StageKind::Volume && s.detail.ends_with('%')) {
                        vol.detail = format!("{}%", (self.volume * 100.0).round() as u32);
                        vol.active = self.volume < 1.0;
                    }
                    let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
                }
                self.publish(true);
            }
            Command::SetMode(m) => { self.mode = m; self.restart_if_active(); self.publish(true); }
            Command::SetDevice(d) => { self.device = d; self.restart_if_active(); self.publish(true); }
            Command::Shutdown => {}
        }
    }

    fn restart_if_active(&mut self) {
        if self.status == PlaybackStatus::Stopped { return; }
        let (idx, ms, paused) = (self.cur, self.position_ms(), self.status == PlaybackStatus::Paused);
        self.start_at(idx, ms);
        if paused { self.pause(); }
    }

    fn pause(&mut self) {
        if self.status != PlaybackStatus::Playing { return; }
        if let Some(a) = &self.active { a.shared.playing.store(false, Release); }
        self.status = PlaybackStatus::Paused;
        self.publish(true);
    }

    fn resume(&mut self) {
        if self.active.is_none() {
            let ms = self.resume_ms.take().unwrap_or(0);
            self.start_at(self.cur, ms);
            return;
        }
        if let Some(a) = &self.active { a.shared.playing.store(true, Release); }
        self.status = PlaybackStatus::Playing;
        self.publish(true);
    }

    fn stop_internal(&mut self) {
        self.active = None;
        self.feeding = None;
        self.prepared = None;
        self.reopen = None;
        self.markers.clear();
        self.end_pending = false;
        self.shown_idx = None;
        self.resume_ms = None;
        self.status = PlaybackStatus::Stopped;
    }

    // ---------- opening ----------

    fn open_decoder(&mut self, idx: usize) -> Result<Box<dyn TrackDecoder>, DecodeError> {
        if let Some((i, _)) = &self.prepared { if *i == idx { return Ok(self.prepared.take().unwrap().1); } }
        decode::open(&self.queue[idx].path)
    }

    fn device_transport(&self) -> Transport {
        let devices = &self.last_devices.0;
        let found = match &self.device { Some(id) => devices.iter().find(|d| &d.id == id), None => devices.iter().find(|d| d.is_default) };
        found.map_or(Transport::Other, |d| d.transport)
    }

    fn plan_for(&self, dec: &mut dyn TrackDecoder, current: Option<(OutputFormat, bool)>, force_shared: bool) -> PipelinePlan {
        let dev = self.device.as_deref();
        let shared = self.backend.caps(dev, false).unwrap_or_else(|_| DeviceCaps::fallback());
        let exclusive = (self.mode == OutputMode::BitPerfect && !force_shared).then(|| self.backend.caps(dev, true).ok()).flatten();
        let spec = dec.info().spec;
        let plan = plan_output(&PlanInput {
            source: &spec, dsd_rate: dec.info().dsd_rate, mode: if force_shared { OutputMode::Processed } else { self.mode },
            transport: self.device_transport(), shared_caps: &shared, exclusive_caps: exclusive.as_ref(), current,
        });
        if let Some(out) = plan.dsd { dec.set_dsd_output(out); }
        plan
    }

    fn open_stream(&mut self, plan: &PipelinePlan) -> Result<(), OutputError> {
        let fmt = plan.format;
        let (prod, src, shared) = ring_pair((fmt.sample_rate as f64 * self.buffer_secs) as usize, fmt);
        shared.apply_gain.store(plan.passthrough == Passthrough::None, Release);
        shared.set_gain(volume_gain(self.volume) as f32);
        let mut stream = self.backend.open(self.device.as_deref(), fmt, plan.exclusive, src)?;
        stream.start()?;
        let info = stream.info().clone();
        self.active = Some(Active { stream, prod, shared, info, written_bytes: 0, carry: vec![], carry_pos: 0 });
        Ok(())
    }

    /// Opens the decoder at `idx` (skipping broken entries), seeks to `ms`, opens a stream and starts feeding.
    fn start_at(&mut self, idx: usize, ms: u64) {
        let keep_prepared = self.prepared.take();
        self.stop_internal();
        self.prepared = keep_prepared;
        let (mut idx, mut ms) = (idx, ms);
        loop {
            if idx >= self.queue.len() { self.finish(); return; }
            let mut dec = match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); idx += 1; ms = 0; continue; } };
            if ms > 0 {
                let frame = ms * dec.info().spec.sample_rate as u64 / 1000;
                if dec.info().total_frames.is_some_and(|t| frame >= t) { idx += 1; ms = 0; continue; }
                if let Err(e) = dec.seek(frame) { self.track_error(idx, &e); idx += 1; ms = 0; continue; }
            }
            let mut plan = self.plan_for(dec.as_mut(), None, false);
            if let Err(e) = self.open_stream(&plan) {
                match e {
                    OutputError::ExclusiveDenied(reason) if plan.exclusive => {
                        plan = self.plan_for(dec.as_mut(), None, true);
                        plan.notes.push(format!("Exclusive access denied ({reason}) — using shared mode"));
                        if let Err(e) = self.open_stream(&plan) { self.output_error(&e); return; }
                    }
                    e => { self.output_error(&e); return; }
                }
            }
            self.begin_feeding(idx, dec, plan, ms, None);
            self.cur = idx;
            self.status = PlaybackStatus::Playing;
            if let Some(a) = &self.active { a.shared.playing.store(true, Release); }
            self.publish(true);
            return;
        }
    }

    fn begin_feeding(&mut self, idx: usize, dec: Box<dyn TrackDecoder>, plan: PipelinePlan, start_ms: u64, reuse: Option<crate::dsp::resample::StreamResampler>) {
        let a = self.active.as_ref().expect("stream open before feeding");
        let info = dec.info().clone();
        let signal = build_signal_path(&info, &plan, &a.info, self.volume, self.backend.bluetooth(&a.info), a.shared.underruns.load(Relaxed));
        self.markers.push_back(Marker {
            start: a.written_frames(), idx, start_ms, out_rate: plan.format.sample_rate, signal,
            duration_ms: info.total_frames.map(|f| f * 1000 / info.spec.sample_rate.max(1) as u64),
        });
        let chain = ProcessChain::new(plan, info.spec, reuse);
        self.feeding = Some(Feeding { idx, dec, chain, decoded: start_ms * info.spec.sample_rate as u64 / 1000, eof: false });
    }

    // ---------- feeding ----------

    fn pump(&mut self) {
        for _ in 0..64 {
            if !self.flush_carry() { return; }
            if self.reopen.is_some() { self.try_reopen(); return; }
            let Some(feed) = self.feeding.as_mut() else { self.check_drained(); return; };
            if feed.eof { self.advance(); continue; }
            if self.active.as_ref().map_or(0, |a| a.prod.slots()) < MIN_FREE_BYTES { return; }
            let feed = self.feeding.as_mut().unwrap();
            match feed.dec.next_block() {
                Ok(Some(block)) => {
                    feed.decoded += block.frames() as u64;
                    let mut bytes = Vec::with_capacity(block.frames() * 16);
                    feed.chain.process(&block, &mut bytes);
                    self.push_bytes(bytes);
                    self.maybe_prepare_next();
                }
                Ok(None) => feed.eof = true,
                Err(e) => { let idx = feed.idx; feed.eof = true; self.track_error(idx, &e); }
            }
        }
    }

    fn push_bytes(&mut self, bytes: Vec<u8>) {
        if let Some(a) = self.active.as_mut() {
            if a.carry_pos < a.carry.len() { a.carry.drain(..a.carry_pos); a.carry.extend(bytes); } else { a.carry = bytes; }
            a.carry_pos = 0;
        }
    }

    /// Writes pending bytes into the ring, whole frames only, so `written_frames()` is exact and a
    /// seek's `discard_to` never splits a frame. Returns true when nothing is left pending.
    fn flush_carry(&mut self) -> bool {
        let Some(a) = self.active.as_mut() else { return false };
        let remaining = a.carry.len() - a.carry_pos;
        if remaining == 0 { return true; }
        let bpf = a.bpf() as usize;
        let n = a.prod.slots().min(remaining) / bpf * bpf;
        if n > 0 {
            let mut chunk = a.prod.write_chunk(n).expect("slots checked");
            let (x, y) = chunk.as_mut_slices();
            let src = &a.carry[a.carry_pos..a.carry_pos + n];
            x.copy_from_slice(&src[..x.len()]);
            y.copy_from_slice(&src[x.len()..]);
            chunk.commit_all();
            a.carry_pos += n;
            a.written_bytes += n as u64;
        }
        a.carry_pos == a.carry.len()
    }

    fn maybe_prepare_next(&mut self) {
        let Some(feed) = &self.feeding else { return };
        let next = feed.idx + 1;
        if next >= self.queue.len() || self.prepared.as_ref().is_some_and(|(i, _)| *i == next) { return; }
        let info = feed.dec.info();
        let Some(total) = info.total_frames else { return };
        let remaining_secs = total.saturating_sub(feed.decoded) as f64 / info.spec.sample_rate.max(1) as f64;
        if remaining_secs <= self.cfg.preroll_secs {
            if let Ok(dec) = decode::open(&self.queue[next].path) { self.prepared = Some((next, dec)); }
        }
    }

    /// The feeding decoder hit EOF: continue gaplessly with the next track, schedule a reopen, or end.
    fn advance(&mut self) {
        let mut feed = self.feeding.take().expect("advance with feeding");
        let mut idx = feed.idx + 1;
        loop {
            if idx >= self.queue.len() {
                let mut tail = vec![];
                feed.chain.flush(&mut tail);
                self.push_bytes(tail);
                self.end_pending = true;
                if let Some(a) = &self.active { a.shared.draining.store(true, Release); }
                return;
            }
            let mut dec = match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); idx += 1; continue; } };
            let current = self.active.as_ref().map(|a| (a.info.format, a.exclusive()));
            let mut plan = self.plan_for(dec.as_mut(), current, false);
            let same = current.is_some_and(|(f, ex)| f == plan.format && ex == plan.exclusive);
            if same {
                let reuse = feed.chain.take_resampler();
                self.begin_feeding(idx, dec, plan, 0, reuse);
            } else {
                let mut tail = vec![];
                feed.chain.flush(&mut tail);
                self.push_bytes(tail);
                let rate_changed = current.is_some_and(|(f, _)| f.sample_rate != plan.format.sample_rate);
                plan.notes.push(if rate_changed { "Rate change: brief gap" } else { "Format change: brief gap" }.into());
                if let Some(a) = &self.active { a.shared.draining.store(true, Release); }
                self.reopen = Some(Reopen { idx, dec, plan });
            }
            return;
        }
    }

    fn drained(&self) -> bool {
        self.active.as_ref().is_none_or(|a| a.carry_pos == a.carry.len() && a.shared.consumed_frames.load(Acquire) >= a.written_frames())
    }

    fn try_reopen(&mut self) {
        if !self.drained() { return; }
        self.update_position();
        let Reopen { idx, dec, plan } = self.reopen.take().unwrap();
        self.active = None;
        self.markers.clear();
        if let Err(e) = self.open_stream(&plan) { self.output_error(&e); return; }
        self.begin_feeding(idx, dec, plan, 0, None);
        if let Some(a) = &self.active { a.shared.playing.store(true, Release); }
    }

    fn check_drained(&mut self) {
        if self.end_pending && self.drained() {
            self.update_position();
            self.finish();
        }
    }

    fn finish(&mut self) {
        self.stop_internal();
        self.cur = 0;
        let _ = self.events.send(PlayerEvent::Ended);
        self.publish(true);
    }

    // ---------- seek ----------

    fn seek(&mut self, ms: u64) {
        if self.status == PlaybackStatus::Stopped || self.active.is_none() { return; }
        let idx = self.markers.front().map_or(self.cur, |m| m.idx);
        {
            let a = self.active.as_mut().unwrap();
            a.carry.clear();
            a.carry_pos = 0;
            a.shared.discard_to.store(a.written_frames(), Release);
            a.shared.draining.store(false, Release);
        }
        self.reopen = None;
        self.end_pending = false;
        self.markers.clear();
        self.shown_idx = None;
        let mut dec = match self.feeding.take() {
            Some(f) if f.idx == idx => f.dec,
            _ => match self.open_decoder(idx) { Ok(d) => d, Err(e) => { self.track_error(idx, &e); return self.start_at(idx + 1, 0); } },
        };
        let frame = ms * dec.info().spec.sample_rate as u64 / 1000;
        if dec.info().total_frames.is_some_and(|t| frame >= t) { return self.start_at(idx + 1, 0); }
        if let Err(e) = dec.seek(frame) { self.track_error(idx, &e); return self.start_at(idx + 1, 0); }
        let current = self.active.as_ref().map(|a| (a.info.format, a.exclusive()));
        let plan = self.plan_for(dec.as_mut(), current, false);
        if current.is_some_and(|(f, ex)| f != plan.format || ex != plan.exclusive) { return self.start_at(idx, ms); }
        self.begin_feeding(idx, dec, plan, ms, None);
        self.cur = idx;
        self.publish(true);
    }

    // ---------- position, events ----------

    fn position_ms(&self) -> u64 {
        if self.status == PlaybackStatus::Stopped { return 0; }
        let (Some(a), Some(m)) = (&self.active, self.markers.front()) else { return self.resume_ms.unwrap_or(0) };
        let consumed = a.shared.consumed_frames.load(Acquire);
        m.start_ms + consumed.saturating_sub(m.start) * 1000 / m.out_rate.max(1) as u64
    }

    fn update_position(&mut self) {
        let Some(a) = &self.active else { return };
        let consumed = a.shared.consumed_frames.load(Acquire);
        let underruns = a.shared.underruns.load(Relaxed);
        while self.markers.len() > 1 && self.markers[1].start <= consumed { self.markers.pop_front(); }
        let mut changed = false;
        if let Some(m) = self.markers.front_mut() {
            if self.shown_idx != Some(m.idx) && m.start <= consumed.max(a.shared.discard_to.load(Acquire)) {
                self.shown_idx = Some(m.idx);
                self.cur = m.idx;
                m.signal.underruns = underruns;
                let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
                changed = true;
            } else if underruns != self.last_underruns.0 && self.last_underruns.1.elapsed() >= Duration::from_secs(1) {
                let fresh = underruns.saturating_sub(self.last_underruns.0).min(16);
                let now = Instant::now();
                self.underrun_times.extend(std::iter::repeat_n(now, fresh as usize));
                while self.underrun_times.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(10)) { self.underrun_times.pop_front(); }
                let grown = grown_buffer(self.buffer_secs, self.underrun_times.len());
                if grown > self.buffer_secs {
                    self.buffer_secs = grown;
                    self.underrun_times.clear();
                    m.signal.notes.push(format!("Buffer increased to {grown:.0} s after repeated underruns"));
                }
                m.signal.underruns = underruns;
                self.last_underruns = (underruns, now);
                let _ = self.events.send(PlayerEvent::SignalPath(m.signal.clone()));
            }
        }
        if changed || (self.status == PlaybackStatus::Playing && self.last_tick.elapsed() >= Duration::from_millis(250)) {
            self.publish(true);
        }
    }

    fn publish(&mut self, emit: bool) {
        self.last_tick = Instant::now();
        let entry = self.queue.get(self.cur);
        let duration_ms = self.markers.front().filter(|m| m.idx == self.cur).and_then(|m| m.duration_ms);
        let state = PlayerState {
            status: self.status,
            queue_index: entry.map(|_| self.cur as u32),
            track: entry.map(|e| TrackSummary { path: e.path.to_string_lossy().into_owned(), title: e.title.clone(), duration_ms }),
            position_ms: self.position_ms(),
            duration_ms,
            volume: self.volume,
            mode: self.mode,
            device_id: self.device.clone(),
        };
        *self.shared_state.lock().unwrap() = state.clone();
        if emit { let _ = self.events.send(PlayerEvent::State(state)); }
    }

    fn emit_queue(&self) {
        let items = self.queue.iter().map(|e| QueueItem { id: e.id, path: e.path.to_string_lossy().into_owned(), title: e.title.clone() }).collect();
        let _ = self.events.send(PlayerEvent::Queue(QueueChanged { items, current: (!self.queue.is_empty()).then_some(self.cur as u32) }));
    }

    fn track_error(&self, idx: usize, e: &DecodeError) {
        let code = e.code();
        let _ = self.events.send(PlayerEvent::Error(EngineErrorEvent {
            code: code.code(), name: code.name().into(), message: e.to_string(),
            path: self.queue.get(idx).map(|q| q.path.to_string_lossy().into_owned()),
        }));
    }

    fn output_error(&mut self, e: &OutputError) {
        let code = e.code();
        let _ = self.events.send(PlayerEvent::Error(EngineErrorEvent { code: code.code(), name: code.name().into(), message: e.to_string(), path: None }));
        self.stop_internal();
        self.publish(true);
    }

    // ---------- devices ----------

    fn check_device(&mut self) {
        let lost = self.active.as_ref().is_some_and(|a| a.shared.device_lost.load(Acquire));
        if lost {
            let ms = self.position_ms();
            let idx = self.cur;
            self.stop_internal();
            self.cur = idx;
            self.resume_ms = Some(ms);
            self.device = None;
            self.status = PlaybackStatus::Paused;
            let code = ErrorCode::DeviceUnavailable;
            let _ = self.events.send(PlayerEvent::Error(EngineErrorEvent { code: code.code(), name: code.name().into(), message: "Output device disconnected; switched to the system default".into(), path: None }));
            self.refresh_devices(true);
            self.publish(true);
            return;
        }
        if self.last_devices.1.elapsed() >= Duration::from_secs(2) { self.refresh_devices(false); }
    }

    fn refresh_devices(&mut self, force_emit: bool) {
        let list = self.backend.devices().unwrap_or_default();
        let changed = list != self.last_devices.0;
        self.last_devices = (list.clone(), Instant::now());
        if changed || force_emit { let _ = self.events.send(PlayerEvent::DevicesChanged(list)); }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine-core --manifest-path engine/Cargo.toml --test player -- --test-threads=4`
Expected: all 14 PASS.

**Debugging guidance (in order):**
- If `gapless_pair_is_sample_exact_in_processed_mode` is off by a few samples, check that `advance()` reuses the stream (`h.opened().len() == 1`) and that `ProcessChain` passes the f32 path without dithering.
- If seek tests see stale audio, confirm `discard_to` is set *before* any new bytes are written.
- If `wait_for` times out, print `p.state()`. Positions come from the front marker, so check that the marker list was rebuilt.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core
git commit -m "feat(player): gapless queue playback, seek, pause, device loss, exclusive fallback"
```

---
### Task 15: Shared-mode output (cpal), SystemBackend, and the `devices` / `play` CLI

**Files:**
- Create: `engine/crates/engine-core/src/output/cpal_out.rs`, `src/output/system.rs`
- Modify: `src/output/mod.rs` (add `pub mod cpal_out; pub mod system;` plus the cfg'd platform modules as empty files for now: `#[cfg(windows)] pub mod wasapi_excl;`, `#[cfg(target_os = "macos")] pub mod coreaudio_hog;`, `#[cfg(target_os = "linux")] pub mod alsa_hw;`), `src/output/format.rs` (add `bytes_to_f32`)
- Modify: `engine/crates/aurora-engine/src/main.rs`
- Test: unit test in `format.rs`, `engine/crates/aurora-engine/tests/cli.rs` (extend)

**Interfaces:**
- Consumes: `OutputBackend`, `OutputStream`, `RingSource`, `StreamInfo`, `DeviceCaps`, `COMMON_RATES` (Task 11); `probe::bluetooth_for` (Task 13); `spawn_player`, `Command`, `PlayerEvent` (Task 14).
- Produces:
```rust
pub fn format::bytes_to_f32(sample: DeviceSample, src: &[u8], dst: &mut [f32]);  // exact for ≤24-bit ints; realtime-safe
pub struct cpal_out::CpalBackend;  impl OutputBackend          // shared mode only; never offers I24 (cpal's I24 is 4 bytes)
pub(crate) fn cpal_out::host() -> cpal::Host;                    // PipeWire host on Linux when available
pub(crate) fn cpal_out::find_device(host: &cpal::Host, id: Option<&str>) -> Result<cpal::Device, OutputError>;
pub(crate) fn cpal_out::device_name(id: Option<&str>) -> Option<String>;
pub struct system::SystemBackend;  impl SystemBackend { pub fn new() -> Self }  impl OutputBackend
```
- **SystemBackend routing:** `exclusive == false` → CpalBackend. `exclusive == true` → `wasapi_excl` (Windows), `coreaudio_hog` (macOS) or `alsa_hw` (Linux). On Linux, a Bluetooth device always reports `caps(_, true) = Err`, so the planner's "Bluetooth: bit-perfect to OS" rule applies. `bluetooth()` delegates to `probe::bluetooth_for`.

- [ ] **Step 1: Write the failing tests**

Bottom of `output/format.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i24_round_trips_through_f32_exactly() {
        let vals = [-(1 << 23), -1, 0, 1, 0x12_3456, (1 << 23) - 1];
        let bytes: Vec<u8> = vals.iter().flat_map(|v: &i32| v.to_le_bytes()[..3].to_vec()).collect();
        let mut f = vec![0f32; vals.len()];
        bytes_to_f32(DeviceSample::I24, &bytes, &mut f);
        let back: Vec<i32> = f.iter().map(|&x| (x as f64 * 8_388_608.0) as i32).collect();
        assert_eq!(back, vals);
    }

    #[test]
    fn i16_and_f32() {
        let mut f = [0f32; 2];
        bytes_to_f32(DeviceSample::I16, &[0x00, 0x40, 0x00, 0xC0], &mut f);
        assert_eq!(f, [0.5, -0.5]);
        bytes_to_f32(DeviceSample::F32, &[0.25f32.to_le_bytes(), 1.0f32.to_le_bytes()].concat(), &mut f);
        assert_eq!(f, [0.25, 1.0]);
    }
}
```

Append to `engine/crates/aurora-engine/tests/cli.rs`:
```rust
#[test]
fn devices_prints_a_json_array() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).args(["devices", "--null"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["id"], "null");
}

#[test]
fn play_null_runs_to_end_and_prints_signal_path() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
        .args(["play", "--null", "--speed", "20", "--bit-perfect", &fixture("short_a.flac"), &fixture("short_b.flac")])
        .output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    let paths: Vec<serde_json::Value> = text.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).filter(|v| v["event"] == "signalPath").collect();
    assert!(!paths.is_empty());
    assert_eq!(paths[0]["data"]["bitPerfect"], true);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core format::tests` and `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --test cli`
Expected: compile error / unknown subcommand failures.

- [ ] **Step 3: Implement `bytes_to_f32`** (append to `format.rs` above the tests)

```rust
/// Device bytes → f32 (used by callbacks that must hand floats to the OS). Realtime-safe.
/// Exact for 16- and 24-bit integers: f32 has a 24-bit mantissa.
pub fn bytes_to_f32(sample: DeviceSample, src: &[u8], dst: &mut [f32]) {
    match sample {
        DeviceSample::I16 => for (o, c) in dst.iter_mut().zip(src.chunks_exact(2)) { *o = i16::from_le_bytes([c[0], c[1]]) as f32 / 32_768.0; },
        DeviceSample::I24 => for (o, c) in dst.iter_mut().zip(src.chunks_exact(3)) { *o = (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0; },
        DeviceSample::I32 => for (o, c) in dst.iter_mut().zip(src.chunks_exact(4)) { *o = (i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64 / 2_147_483_648.0) as f32; },
        DeviceSample::F32 => for (o, c) in dst.iter_mut().zip(src.chunks_exact(4)) { *o = f32::from_le_bytes([c[0], c[1], c[2], c[3]]); },
    }
}
```

- [ ] **Step 4: Implement `cpal_out.rs`**

```rust
use std::sync::atomic::Ordering::*;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, InterfaceType, SampleFormat, StreamConfig};
use protocol::{BluetoothInfo, DeviceInfo, ShareMode, Transport};

use super::format::{DeviceSample, OutputFormat};
use super::{DeviceCaps, OutputBackend, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

pub struct CpalBackend;

pub(crate) fn host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    if let Ok(h) = cpal::host_from_id(cpal::HostId::PipeWire) { return h; }
    cpal::default_host()
}

pub(crate) fn find_device(host: &cpal::Host, id: Option<&str>) -> Result<cpal::Device, OutputError> {
    match id {
        Some(id) => {
            let parsed: cpal::DeviceId = id.parse().map_err(|_| OutputError::DeviceUnavailable(id.into()))?;
            host.device_by_id(&parsed).ok_or_else(|| OutputError::DeviceUnavailable(id.into()))
        }
        None => host.default_output_device().ok_or_else(|| OutputError::DeviceUnavailable("no default output device".into())),
    }
}

pub(crate) fn device_name(id: Option<&str>) -> Option<String> {
    find_device(&host(), id).ok()?.description().ok().map(|d| d.name().to_string())
}

pub(crate) fn transport_of(t: InterfaceType) -> Transport {
    match t {
        InterfaceType::Bluetooth => Transport::Bluetooth,
        InterfaceType::Usb => Transport::Usb,
        InterfaceType::BuiltIn => Transport::Builtin,
        InterfaceType::Hdmi | InterfaceType::DisplayPort => Transport::Hdmi,
        _ => Transport::Other,
    }
}

fn sample_of(f: SampleFormat) -> Option<DeviceSample> {
    match f { SampleFormat::I16 => Some(DeviceSample::I16), SampleFormat::I32 => Some(DeviceSample::I32), SampleFormat::F32 => Some(DeviceSample::F32), _ => None }
}

fn describe(dev: &cpal::Device) -> (String, String, Transport) {
    let id = dev.id().map(|i| i.to_string()).unwrap_or_default();
    let (name, transport) = dev.description().map(|d| (d.name().to_string(), transport_of(d.interface_type()))).unwrap_or_else(|_| (id.clone(), Transport::Other));
    (id, name, transport)
}

fn shared_caps(dev: &cpal::Device) -> Result<DeviceCaps, OutputError> {
    let configs: Vec<_> = dev.supported_output_configs().map_err(|e| OutputError::DeviceUnavailable(e.to_string()))?.collect();
    let mut caps = DeviceCaps { rates: vec![], max_channels: 0, samples: vec![], exclusive: false };
    for c in &configs {
        caps.max_channels = caps.max_channels.max(c.channels());
        if let Some(s) = sample_of(c.sample_format()) { if !caps.samples.contains(&s) { caps.samples.push(s); } }
        for r in COMMON_RATES { if r >= c.min_sample_rate() && r <= c.max_sample_rate() && !caps.rates.contains(&r) { caps.rates.push(r); } }
    }
    caps.rates.sort_unstable();
    if caps.samples.is_empty() || caps.rates.is_empty() { return Err(OutputError::UnsupportedFormat("device reports no usable PCM configs".into())); }
    Ok(caps)
}

impl CpalBackend {
    pub(crate) fn list(&self, exclusive_capable: impl Fn(Transport) -> bool) -> Result<Vec<DeviceInfo>, OutputError> {
        let host = host();
        let default_id = host.default_output_device().and_then(|d| d.id().ok()).map(|i| i.to_string());
        let devices = host.output_devices().map_err(|e| OutputError::Other(e.to_string()))?;
        Ok(devices.map(|d| {
            let (id, name, transport) = describe(&d);
            let rates = shared_caps(&d).map(|c| c.rates).unwrap_or_default();
            DeviceInfo { is_default: Some(&id) == default_id.as_ref(), id, name, transport, sample_rates: rates, exclusive_capable: exclusive_capable(transport) }
        }).collect())
    }
}

pub struct CpalStream { stream: cpal::Stream, info: StreamInfo }

impl OutputStream for CpalStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { self.stream.play().map_err(|e| OutputError::Other(e.to_string())) }
}

/// Opens a shared-mode stream. `to_f32` makes the callback convert ring bytes in `ring_sample`
/// format to f32 (used by the macOS hog backend); otherwise ring bytes are copied raw.
pub(crate) fn open_stream(dev: &cpal::Device, fmt: OutputFormat, ring_sample: DeviceSample, mut source: RingSource, share: ShareMode) -> Result<CpalStream, OutputError> {
    let (id, name, transport) = describe(dev);
    let config = StreamConfig { channels: fmt.channels, sample_rate: fmt.sample_rate, buffer_size: BufferSize::Default };
    let shared = source.shared().clone();
    let on_err = move |e: cpal::Error| {
        if matches!(e.kind(), ErrorKind::DeviceNotAvailable | ErrorKind::DeviceChanged) { shared.device_lost.store(true, Release); }
    };
    let stream = if fmt.sample == DeviceSample::F32 && ring_sample != DeviceSample::F32 {
        let bpf = ring_sample.bytes() * fmt.channels as usize;
        let mut scratch = vec![0u8; 65_536 * bpf];
        dev.build_output_stream_raw(config, SampleFormat::F32, move |data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {
            let Some(out) = data.as_slice_mut::<f32>() else { return };
            let need = (out.len() * ring_sample.bytes()).min(scratch.len());
            source.fill(&mut scratch[..need]);
            bytes_to_f32_slice(ring_sample, &scratch[..need], out);
        }, on_err, None)
    } else {
        let sf = match fmt.sample {
            DeviceSample::I16 => SampleFormat::I16,
            DeviceSample::I32 => SampleFormat::I32,
            DeviceSample::F32 => SampleFormat::F32,
            DeviceSample::I24 => return Err(OutputError::UnsupportedFormat("packed 24-bit isn't available in shared mode".into())),
        };
        dev.build_output_stream_raw(config, sf, move |data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| { source.fill(data.bytes_mut()); }, on_err, None)
    }
    .map_err(|e| OutputError::UnsupportedFormat(e.to_string()))?;
    Ok(CpalStream { stream, info: StreamInfo { device_id: id, device_name: name, transport, share, format: fmt } })
}

fn bytes_to_f32_slice(s: DeviceSample, src: &[u8], dst: &mut [f32]) {
    let n = (src.len() / s.bytes()).min(dst.len());
    super::format::bytes_to_f32(s, &src[..n * s.bytes()], &mut dst[..n]);
    dst[n..].fill(0.0);
}

impl OutputBackend for CpalBackend {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> { self.list(|_| false) }
    fn caps(&self, device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError> {
        if exclusive { return Err(OutputError::Other("cpal backend is shared-mode only".into())); }
        shared_caps(&find_device(&host(), device)?)
    }
    fn open(&self, device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
        if exclusive { return Err(OutputError::Other("cpal backend is shared-mode only".into())); }
        Ok(Box::new(open_stream(&find_device(&host(), device)?, format, format.sample, source, ShareMode::Shared)?))
    }
    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> { crate::probe::bluetooth_for(stream) }
}
```

- [ ] **Step 5: Implement `system.rs`**

```rust
use protocol::{BluetoothInfo, DeviceInfo, Transport};

use super::cpal_out::CpalBackend;
use super::format::OutputFormat;
use super::{DeviceCaps, OutputBackend, OutputError, OutputStream, RingSource, StreamInfo};

pub struct SystemBackend { shared: CpalBackend }

impl SystemBackend {
    pub fn new() -> Self { Self { shared: CpalBackend } }
}

impl Default for SystemBackend { fn default() -> Self { Self::new() } }

// One implementation per OS keeps every build free of unreachable code.
#[cfg(windows)]
fn exclusive_caps(_: &SystemBackend, d: Option<&str>) -> Result<DeviceCaps, OutputError> { super::wasapi_excl::caps(d) }
#[cfg(windows)]
fn exclusive_open(d: Option<&str>, f: OutputFormat, s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { super::wasapi_excl::open(d, f, s) }

#[cfg(target_os = "macos")]
fn exclusive_caps(_: &SystemBackend, d: Option<&str>) -> Result<DeviceCaps, OutputError> { super::coreaudio_hog::caps(d) }
#[cfg(target_os = "macos")]
fn exclusive_open(d: Option<&str>, f: OutputFormat, s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { super::coreaudio_hog::open(d, f, s) }

#[cfg(target_os = "linux")]
fn exclusive_caps(b: &SystemBackend, d: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let transport = b.devices().ok().and_then(|ds| ds.into_iter().find(|x| match d { Some(id) => x.id == id, None => x.is_default })).map_or(Transport::Other, |x| x.transport);
    if transport == Transport::Bluetooth { return Err(OutputError::Other("Bluetooth goes through PipeWire".into())); }
    super::alsa_hw::caps(d)
}
#[cfg(target_os = "linux")]
fn exclusive_open(d: Option<&str>, f: OutputFormat, s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { super::alsa_hw::open(d, f, s) }

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn exclusive_caps(_: &SystemBackend, _: Option<&str>) -> Result<DeviceCaps, OutputError> { Err(OutputError::Other("exclusive mode not supported on this OS".into())) }
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn exclusive_open(_: Option<&str>, _: OutputFormat, _: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { Err(OutputError::Other("exclusive mode not supported on this OS".into())) }

impl OutputBackend for SystemBackend {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        self.shared.list(|t| if cfg!(target_os = "linux") { t != Transport::Bluetooth } else { cfg!(any(windows, target_os = "macos")) })
    }

    fn caps(&self, device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError> {
        if exclusive { exclusive_caps(self, device) } else { self.shared.caps(device, false) }
    }

    fn open(&self, device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
        if exclusive { exclusive_open(device, format, source) } else { self.shared.open(device, format, false, source) }
    }

    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> { crate::probe::bluetooth_for(stream) }
}
```
Until Tasks 16–18 land, give each platform module stub functions so the crate compiles:
```rust
pub fn caps(_device: Option<&str>) -> Result<DeviceCaps, OutputError> { Err(OutputError::Other("not implemented yet".into())) }
pub fn open(_device: Option<&str>, _f: OutputFormat, _s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { Err(OutputError::Other("not implemented yet".into())) }
```
(with the matching `use super::{...}` lines).

- [ ] **Step 6: Extend the CLI** (`aurora-engine/src/main.rs`): add `devices` and `play`

```rust
use std::sync::Arc;
use engine_core::output::null::{NullBackend, NullMode};
use engine_core::output::{system::SystemBackend, OutputBackend};
use engine_core::player::{spawn_player, Command, PlayerConfig, PlayerEvent};

// add to enum Cmd:
    /// List output devices as JSON.
    Devices { #[arg(long)] null: bool },
    /// Play files from the command line, printing events as JSON lines.
    Play {
        files: Vec<PathBuf>,
        #[arg(long)] bit_perfect: bool,
        #[arg(long)] device: Option<String>,
        /// Use the null output instead of a real device.
        #[arg(long)] null: bool,
        /// Null-output speed multiplier.
        #[arg(long, default_value_t = 1.0)] speed: f64,
    },

fn backend(null: bool, speed: f64) -> Arc<dyn OutputBackend> {
    if null { Arc::new(NullBackend::new(NullMode::Realtime { speed }).0) } else { Arc::new(SystemBackend::new()) }
}

fn devices(null: bool) -> i32 {
    match backend(null, 1.0).devices() {
        Ok(list) => { println!("{}", serde_json::to_string(&list).unwrap()); 0 }
        Err(e) => { eprintln!("{e}"); 1 }
    }
}

fn play(files: Vec<PathBuf>, bit_perfect: bool, device: Option<String>, null: bool, speed: f64) -> i32 {
    let (tx, rx) = crossbeam_channel::unbounded();
    let player = spawn_player(backend(null, speed), PlayerConfig::default(), tx);
    if bit_perfect { player.send(Command::SetMode(protocol::OutputMode::BitPerfect)); }
    if device.is_some() { player.send(Command::SetDevice(device)); }
    player.send(Command::SetQueue { paths: files, start: 0 });
    player.send(Command::Play(None));
    for ev in rx.iter() {
        let line = match &ev {
            PlayerEvent::SignalPath(s) => serde_json::json!({ "event": "signalPath", "data": s }),
            PlayerEvent::Error(e) => serde_json::json!({ "event": "error", "data": e }),
            PlayerEvent::Ended => { println!("{}", serde_json::json!({ "event": "ended" })); break; }
            _ => continue,
        };
        println!("{line}");
    }
    player.shutdown();
    0
}
// in main's match:
        Some(Cmd::Devices { null }) => devices(null),
        Some(Cmd::Play { files, bit_perfect, device, null, speed }) => play(files, bit_perfect, device, null, speed),
```

- [ ] **Step 7: Run the tests, then listen to a real file**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core format::tests` and `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --test cli`
Expected: all PASS.
Manual check: `cargo run -q --manifest-path engine/Cargo.toml -p aurora-engine -- devices`, then `cargo run -q --release --manifest-path engine/Cargo.toml -p aurora-engine -- play fixtures/out/mp3_320.mp3`. You should hear the two-tone signal from your default device, and a `signalPath` line should show `"mode":"shared"`.

- [ ] **Step 8: Commit**

```bash
git add engine/crates
git commit -m "feat(output): cpal shared backend, SystemBackend routing, devices/play CLI"
```

---

### Task 16: Windows exclusive mode (WASAPI)

**Files:**
- Modify: `engine/crates/engine-core/src/output/wasapi_excl.rs` (replace the stub)
- Modify: `engine/crates/engine-core/Cargo.toml` — add under `[target.'cfg(target_os = "windows")'.dependencies]`: `windows = { version = "0.62", features = ["Win32_Media_Audio", "Win32_Foundation"] }`
- Test: `engine/crates/engine-core/tests/hw_exclusive.rs` (`#[ignore]` hardware test, shared by Tasks 16–18)

**Interfaces:**
- Consumes: `DeviceCaps`, `OutputFormat`, `DeviceSample`, `RingSource`, `StreamInfo`, `COMMON_RATES`; `cpal_out::device_name` (Task 15).
- Produces: `pub fn caps(device: Option<&str>) -> Result<DeviceCaps, OutputError>` and `pub fn open(device: Option<&str>, f: OutputFormat, src: RingSource) -> Result<Box<dyn OutputStream>, OutputError>`.
- **Mapping:** a cpal device id looks like `"wasapi:<endpoint-id>"`, so strip the prefix and call `DeviceEnumerator::get_device`. If that fails, fall back to `get_device_with_name(cpal_out::device_name(id))`. `None` → the default render device.
- **Formats:** I16 → store/valid 16/16. I24 → 24/24 packed. I32 → 32/32, falling back to 32/24 (WASAPI 24-in-32 is MSB-aligned, so our left-justified data is correct). F32 → 32/32 float.
- `AUDCLNT_E_DEVICE_IN_USE` → `ExclusiveDenied`. A render thread owns all COM objects (they're thread-affine).

- [ ] **Step 1: Write the ignored hardware test**

`tests/hw_exclusive.rs`:
```rust
//! Run manually with a real output device: `cargo test -p engine-core --test hw_exclusive -- --ignored --nocapture`
use std::sync::atomic::Ordering::*;
use std::time::Duration;

use engine_core::output::format::OutputFormat;
use engine_core::output::ring::ring_pair;
use engine_core::output::{system::SystemBackend, OutputBackend};

#[test]
#[ignore = "needs a real audio device"]
fn exclusive_caps_and_one_second_of_tone() {
    let b = SystemBackend::new();
    let caps = b.caps(None, true).expect("exclusive caps");
    println!("exclusive caps: {caps:?}");
    assert!(caps.exclusive && !caps.rates.is_empty());
    let sample = caps.best_int_for(16).expect("an integer format");
    let rate = if caps.supports_rate(48_000) { 48_000 } else { caps.rates[0] };
    let fmt = OutputFormat { sample_rate: rate, channels: 2, sample };
    let (mut prod, src, shared) = ring_pair(rate as usize, fmt);
    let mut bytes = Vec::new();
    for n in 0..rate as usize {
        let v = ((2.0 * std::f64::consts::PI * 440.0 * n as f64 / rate as f64).sin() * 0.1 * i32::MAX as f64) as i32;
        for _ in 0..2 { engine_core::dsp::convert::write_int_passthrough(&[v], sample, &mut bytes); }
    }
    let mut c = prod.write_chunk(bytes.len().min(prod.slots())).unwrap();
    let (a, rest) = c.as_mut_slices();
    let (al, rl) = (a.len(), rest.len());
    a.copy_from_slice(&bytes[..al]);
    rest.copy_from_slice(&bytes[al..al + rl]);
    c.commit_all();
    let mut s = b.open(None, fmt, true, src).expect("open exclusive");
    s.start().unwrap();
    shared.playing.store(true, Release);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(shared.consumed_frames.load(Acquire) >= rate as u64 * 9 / 10);
    println!("stream: {:?}", s.info());
}
```

- [ ] **Step 2: Run it before implementing**

Run (Windows): `cargo test --manifest-path engine/Cargo.toml -p engine-core --test hw_exclusive -- --ignored --nocapture`
Expected: FAIL at `expect("exclusive caps")` with "not implemented yet".

- [ ] **Step 3: Implement `wasapi_excl.rs`**

```rust
//! WASAPI exclusive-mode output, event-driven. All COM objects live on one render thread.
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use protocol::{ShareMode, Transport};
use wasapi::{calculate_period_100ns, initialize_mta, DeviceEnumerator, Direction, SampleType, StreamMode, WasapiError, WaveFormat};
use windows::Win32::Media::Audio::{AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED, AUDCLNT_E_UNSUPPORTED_FORMAT};

use super::format::{DeviceSample, OutputFormat};
use super::{cpal_out, DeviceCaps, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

fn map_err(e: WasapiError) -> OutputError {
    if let WasapiError::Windows(w) = &e {
        let c = w.code();
        if c == AUDCLNT_E_DEVICE_IN_USE { return OutputError::ExclusiveDenied("device in use by another application".into()); }
        if c == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED { return OutputError::ExclusiveDenied("exclusive mode disabled in Windows sound settings".into()); }
        if c == AUDCLNT_E_UNSUPPORTED_FORMAT { return OutputError::UnsupportedFormat("format rejected by device".into()); }
    }
    OutputError::DeviceUnavailable(e.to_string())
}

fn device(id: Option<&str>) -> Result<wasapi::Device, OutputError> {
    let _ = initialize_mta();
    let en = DeviceEnumerator::new().map_err(map_err)?;
    match id {
        None => en.get_default_device(&Direction::Render).map_err(map_err),
        Some(id) => en.get_device(id.strip_prefix("wasapi:").unwrap_or(id)).or_else(|_| {
            let name = cpal_out::device_name(Some(id)).ok_or_else(|| WasapiError::DeviceNotFound(id.into()))?;
            en.get_device_with_name(&name)
        }).map_err(map_err),
    }
}

/// Candidate wave formats for a device sample type, most preferred first.
fn wave_formats(s: DeviceSample, rate: u32, ch: u16) -> Vec<WaveFormat> {
    let (r, c) = (rate as usize, ch as usize);
    match s {
        DeviceSample::I16 => vec![WaveFormat::new(16, 16, &SampleType::Int, r, c, None)],
        DeviceSample::I24 => vec![WaveFormat::new(24, 24, &SampleType::Int, r, c, None)],
        DeviceSample::I32 => vec![WaveFormat::new(32, 32, &SampleType::Int, r, c, None), WaveFormat::new(32, 24, &SampleType::Int, r, c, None)],
        DeviceSample::F32 => vec![WaveFormat::new(32, 32, &SampleType::Float, r, c, None)],
    }
}

fn same(a: &WaveFormat, b: &WaveFormat) -> bool {
    a.get_samplespersec() == b.get_samplespersec() && a.get_bitspersample() == b.get_bitspersample() && a.get_validbitspersample() == b.get_validbitspersample() && a.get_nchannels() == b.get_nchannels()
}

pub fn caps(id: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let dev = device(id)?;
    let client = dev.get_iaudioclient().map_err(map_err)?;
    let channels = dev.get_device_format().map(|f| f.get_nchannels()).unwrap_or(2).max(2);
    let mut caps = DeviceCaps { rates: vec![], max_channels: channels, samples: vec![], exclusive: false };
    for rate in COMMON_RATES {
        for s in [DeviceSample::I16, DeviceSample::I24, DeviceSample::I32, DeviceSample::F32] {
            let ok = wave_formats(s, rate, 2).iter().any(|wf| client.is_supported_exclusive_with_quirks(wf).is_ok_and(|got| same(&got, wf)));
            if ok {
                if !caps.rates.contains(&rate) { caps.rates.push(rate); }
                if !caps.samples.contains(&s) { caps.samples.push(s); }
            }
        }
    }
    caps.exclusive = !caps.rates.is_empty();
    if !caps.exclusive { return Err(OutputError::Other("device accepts no exclusive formats".into())); }
    Ok(caps)
}

pub struct WasapiStream { info: StreamInfo, alive: Arc<AtomicBool>, join: Option<JoinHandle<()>> }

impl OutputStream for WasapiStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { Ok(()) } // starts as soon as it's initialized
}

impl Drop for WasapiStream {
    fn drop(&mut self) {
        self.alive.store(false, Release);
        if let Some(j) = self.join.take() { let _ = j.join(); }
    }
}

pub fn open(id: Option<&str>, fmt: OutputFormat, mut source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
    let id_owned = id.map(str::to_owned);
    let alive = Arc::new(AtomicBool::new(true));
    let (init_tx, init_rx) = mpsc::channel::<Result<(String, String), OutputError>>();
    let alive2 = alive.clone();
    let join = std::thread::Builder::new().name("aurora-wasapi".into()).spawn(move || {
        let setup = (|| -> Result<_, OutputError> {
            let dev = device(id_owned.as_deref())?;
            let name = dev.get_friendlyname().unwrap_or_default();
            let dev_id = dev.get_id().unwrap_or_default();
            let mut last_err = OutputError::UnsupportedFormat("no matching wave format".into());
            for wf in wave_formats(fmt.sample, fmt.sample_rate, fmt.channels) {
                let mut client = dev.get_iaudioclient().map_err(map_err)?;
                let (def, _min) = client.get_device_period().map_err(map_err)?;
                let period = client.calculate_aligned_period_near(def, Some(128), &wf).map_err(map_err)?;
                let mut result = client.initialize_client(&wf, &Direction::Render, &StreamMode::EventsExclusive { period_hns: period });
                if let Err(WasapiError::Windows(w)) = &result {
                    if w.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                        let frames = client.get_buffer_size().map_err(map_err)?;
                        let aligned = calculate_period_100ns(frames as i64, fmt.sample_rate as i64);
                        client = dev.get_iaudioclient().map_err(map_err)?;
                        result = client.initialize_client(&wf, &Direction::Render, &StreamMode::EventsExclusive { period_hns: aligned });
                    }
                }
                match result {
                    Ok(()) => return Ok((client, wf.get_blockalign() as usize, name, dev_id)),
                    Err(e) => last_err = map_err(e),
                }
            }
            Err(last_err)
        })();
        let (client, bpf, name, dev_id) = match setup {
            Ok(v) => v,
            Err(e) => { let _ = init_tx.send(Err(e)); return; }
        };
        let run = (|| -> Result<(), WasapiError> {
            let event = client.set_get_eventhandle()?;
            let render = client.get_audiorenderclient()?;
            let max_frames = client.get_buffer_size()? as usize;
            let mut buf = vec![0u8; max_frames * bpf];
            let frames = client.get_available_space_in_frames()? as usize;
            source.fill(&mut buf[..frames * bpf]);
            render.write_to_device(frames, &buf[..frames * bpf], None)?;
            client.start_stream()?;
            let _ = init_tx.send(Ok((name, dev_id)));
            while alive2.load(Acquire) {
                if event.wait_for_event(1000).is_err() { source.shared().device_lost.store(true, Release); break; }
                let frames = client.get_available_space_in_frames()? as usize;
                if frames == 0 { continue; }
                source.fill(&mut buf[..frames * bpf]);
                render.write_to_device(frames, &buf[..frames * bpf], None)?;
            }
            client.stop_stream()?;
            Ok(())
        })();
        if run.is_err() { source.shared().device_lost.store(true, Release); }
    }).map_err(|e| OutputError::Other(e.to_string()))?;

    match init_rx.recv() {
        Ok(Ok((name, dev_id))) => {
            let transport = if dev_id.contains("BTHENUM") || dev_id.contains("BTHLEDevice") || name.to_ascii_lowercase().contains("bluetooth") { Transport::Bluetooth } else { Transport::Other };
            Ok(Box::new(WasapiStream {
                info: StreamInfo { device_id: id.map(str::to_owned).unwrap_or(dev_id), device_name: name, transport, share: ShareMode::Exclusive, format: fmt },
                alive,
                join: Some(join),
            }))
        }
        Ok(Err(e)) => { let _ = join.join(); Err(e) }
        Err(_) => { let _ = join.join(); Err(OutputError::Other("render thread exited during setup".into())) }
    }
}
```
**Transport:** the cpal device list (Task 15) already detects Bluetooth via `BTHENUM`. The endpoint-id check above is a best-effort mirror of that, so the exclusive stream reports the same transport. If you can, prefer looking up the transport from `SystemBackend::devices()` by id.

- [ ] **Step 4: Run the hardware test on Windows**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core --test hw_exclusive -- --ignored --nocapture`
Expected: PASS. You should hear a 1-second 440 Hz tone, and the printed caps should list several rates. If another app holds the device exclusively, expect `ExclusiveDenied`.
Then try a real bit-perfect run: `cargo run -q --release --manifest-path engine/Cargo.toml -p aurora-engine -- play --bit-perfect fixtures/out/flac24_96k.flac`. The `signalPath` should show `"mode":"exclusive"`, `"bitPerfect":true`, `"bitPerfectScope":"device"`.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/engine-core
git commit -m "feat(output): WASAPI exclusive-mode backend"
```

---

### Task 17: macOS exclusive mode (CoreAudio hog mode)

**Files:**
- Modify: `engine/crates/engine-core/src/output/coreaudio_hog.rs` (replace the stub)

**Interfaces:**
- Consumes: `cpal_out::{host, find_device, open_stream, CpalStream}`, `bytes_to_f32` (Task 15).
- Produces: `pub fn caps(device: Option<&str>) -> Result<DeviceCaps, OutputError>` and `pub fn open(device: Option<&str>, f: OutputFormat, src: RingSource) -> Result<Box<dyn OutputStream>, OutputError>`.
- **Approach:** hog the device, set its nominal sample rate, set its physical format to the matching integer width, then open a cpal F32 stream on it. The callback converts the ring's integer bytes to f32 *exactly* (≤24-bit), and CoreAudio converts back to integers without loss because nothing else is mixing.
  - `caps` advertises I16, I24 and F32 only. I32 isn't offered because f32 can't carry 32-bit integers.
  - Transport comes from `get_device_transport_type`; `kAudioDeviceTransportTypeBluetooth = 'blue' = 0x626C7565`.
  - Hog mode is released in `Drop`.

- [ ] **Step 1: Implement**

```rust
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{
    find_matching_physical_format, get_available_sample_rates, get_default_device_id, get_device_id_from_name,
    get_device_transport_type, get_hogging_pid, get_supported_physical_stream_formats, set_device_physical_stream_format,
    set_device_sample_rate, toggle_hog_mode,
};
use coreaudio::audio_unit::{SampleFormat as CaSample, StreamFormat};
use cpal::traits::DeviceTrait;
use protocol::{ShareMode, Transport};

use super::cpal_out::{self, CpalStream};
use super::format::{DeviceSample, OutputFormat};
use super::{DeviceCaps, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

const TRANSPORT_BLUETOOTH: u32 = u32::from_be_bytes(*b"blue");
const TRANSPORT_BLUETOOTH_LE: u32 = u32::from_be_bytes(*b"blea");
const TRANSPORT_USB: u32 = u32::from_be_bytes(*b"usb ");
const TRANSPORT_BUILTIN: u32 = u32::from_be_bytes(*b"bltn");

fn ca_err(e: impl std::fmt::Debug) -> OutputError { OutputError::DeviceUnavailable(format!("{e:?}")) }

fn resolve(id: Option<&str>) -> Result<(u32, String), OutputError> {
    let name = cpal_out::device_name(id).ok_or_else(|| OutputError::DeviceUnavailable(id.unwrap_or("default").into()))?;
    let dev = match id { None => get_default_device_id(false), Some(_) => get_device_id_from_name(&name, false) };
    dev.map(|d| (d, name)).ok_or_else(|| OutputError::DeviceUnavailable("CoreAudio device not found".into()))
}

fn transport(dev: u32) -> Transport {
    match get_device_transport_type(dev).unwrap_or(0) {
        TRANSPORT_BLUETOOTH | TRANSPORT_BLUETOOTH_LE => Transport::Bluetooth,
        TRANSPORT_USB => Transport::Usb,
        TRANSPORT_BUILTIN => Transport::Builtin,
        _ => Transport::Other,
    }
}

pub fn caps(id: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let (dev, _) = resolve(id)?;
    let ranges = get_available_sample_rates(dev).map_err(ca_err)?;
    let rates = COMMON_RATES.into_iter().filter(|&r| ranges.iter().any(|v| (r as f64) >= v.mMinimum && (r as f64) <= v.mMaximum)).collect::<Vec<_>>();
    let mut samples = vec![DeviceSample::F32];
    let mut max_channels = 2u16;
    for f in get_supported_physical_stream_formats(dev).map_err(ca_err)? {
        max_channels = max_channels.max(f.mFormat.mChannelsPerFrame as u16);
        let s = match f.mFormat.mBitsPerChannel { 16 => Some(DeviceSample::I16), 24 => Some(DeviceSample::I24), _ => None };
        if let Some(s) = s { if !samples.contains(&s) { samples.push(s); } }
    }
    if rates.is_empty() { return Err(OutputError::Other("no usable sample rates".into())); }
    Ok(DeviceCaps { rates, max_channels, samples, exclusive: true })
}

pub struct HogStream { inner: CpalStream, info: StreamInfo, dev: u32 }

impl OutputStream for HogStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { self.inner.start() }
}

impl Drop for HogStream {
    fn drop(&mut self) {
        if get_hogging_pid(self.dev).ok() == Some(std::process::id() as i32) { let _ = toggle_hog_mode(self.dev); }
    }
}

pub fn open(id: Option<&str>, fmt: OutputFormat, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
    let (dev, name) = resolve(id)?;
    let me = std::process::id() as i32;
    match get_hogging_pid(dev) {
        Ok(pid) if pid != -1 && pid != me => return Err(OutputError::ExclusiveDenied(format!("device hogged by process {pid}"))),
        _ => {}
    }
    if toggle_hog_mode(dev).map_err(ca_err)? != me { return Err(OutputError::ExclusiveDenied("hog mode refused".into())); }
    let release = || { let _ = toggle_hog_mode(dev); };
    if let Err(e) = set_device_sample_rate(dev, fmt.sample_rate as f64) { release(); return Err(ca_err(e)); }
    let (ca_sample, flags) = match fmt.sample {
        DeviceSample::I16 => (CaSample::I16, LinearPcmFlags::IS_SIGNED_INTEGER | LinearPcmFlags::IS_PACKED),
        DeviceSample::I24 => (CaSample::I24, LinearPcmFlags::IS_SIGNED_INTEGER | LinearPcmFlags::IS_PACKED),
        DeviceSample::F32 | DeviceSample::I32 => (CaSample::F32, LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED),
    };
    let wanted = StreamFormat { sample_rate: fmt.sample_rate as f64, sample_format: ca_sample, flags, channels: fmt.channels as u32 };
    if let Some(asbd) = find_matching_physical_format(dev, wanted) { let _ = set_device_physical_stream_format(dev, asbd); }
    let cpal_dev = match cpal_out::find_device(&cpal_out::host(), id) { Ok(d) => d, Err(e) => { release(); return Err(e); } };
    let float_fmt = OutputFormat { sample: DeviceSample::F32, ..fmt };
    let inner = match cpal_out::open_stream(&cpal_dev, float_fmt, fmt.sample, source, ShareMode::Exclusive) { Ok(s) => s, Err(e) => { release(); return Err(e); } };
    let device_id = cpal_dev.id().map(|i| i.to_string()).unwrap_or_default();
    Ok(Box::new(HogStream { inner, info: StreamInfo { device_id, device_name: name, transport: transport(dev), share: ShareMode::Exclusive, format: fmt }, dev }))
}
```

- [ ] **Step 2: Run the hardware test on a Mac**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core --test hw_exclusive -- --ignored --nocapture`
Expected: PASS. You hear a 440 Hz tone, and Audio MIDI Setup shows the device switch to the test's sample rate while it plays. Then run `aurora-engine play --bit-perfect fixtures/out/flac24_96k.flac` and check that the signal path shows `"mode":"exclusive"` and `"format":"s24"`.
If you don't have a Mac, run `cargo check --manifest-path engine/Cargo.toml -p engine-core` in CI's macOS job (Task 23).

- [ ] **Step 3: Commit**

```bash
git add engine/crates/engine-core/src/output/coreaudio_hog.rs
git commit -m "feat(output): macOS hog-mode exclusive backend with exact f32 transport"
```

---

### Task 18: Linux exclusive mode (ALSA `hw:`)

**Files:**
- Modify: `engine/crates/engine-core/src/output/alsa_hw.rs` (replace the stub)

**Interfaces:**
- Consumes: `cpal_out::device_name` (Task 15).
- Produces: `pub fn caps(device: Option<&str>) -> Result<DeviceCaps, OutputError>`, `pub fn open(device: Option<&str>, f: OutputFormat, src: RingSource) -> Result<Box<dyn OutputStream>, OutputError>`, and `pub fn card_for(name: &str, cards: &[(i32, String, String)]) -> Option<i32>` (pure, unit-tested).
- **Card selection:** match the cpal/PipeWire device name against each ALSA card's short or long name, by substring in either direction (case-insensitive). If nothing matches, return `Err`, which makes the planner fall back with the note "Exclusive mode unavailable". `EBUSY` (PipeWire holding the card) → `ExclusiveDenied`, which triggers the shared fallback with a "device busy" note.
- **Formats:** I16 → `S16_LE`, I24 → `S24_3LE`, I32 → `S32_LE`. No rate resampling (`set_rate_resample(false)`).

- [ ] **Step 1: Write the failing unit test** (bottom of `alsa_hw.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_cards_by_name() {
        let cards = vec![(0, "PCH".into(), "HDA Intel PCH".into()), (2, "D50s".into(), "Topping D50s at usb-0000:00:14.0-2".into())];
        assert_eq!(card_for("Topping D50s Analog Stereo", &cards), Some(2));
        assert_eq!(card_for("Built-in Audio (HDA Intel PCH)", &cards), Some(0));
        assert_eq!(card_for("Something else", &cards), None);
    }
}
```
Run (Linux): `cargo test --manifest-path engine/Cargo.toml -p engine-core alsa_hw` → compile error.

- [ ] **Step 2: Implement**

```rust
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use protocol::{ShareMode, Transport};

use super::format::{DeviceSample, OutputFormat};
use super::{cpal_out, DeviceCaps, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

pub fn card_for(name: &str, cards: &[(i32, String, String)]) -> Option<i32> {
    let n = name.to_ascii_lowercase();
    cards.iter().find(|(_, short, long)| {
        let (s, l) = (short.to_ascii_lowercase(), long.to_ascii_lowercase());
        let l_head = l.split(" at ").next().unwrap_or(&l).to_string();
        n.contains(&s) || n.contains(&l_head) || l_head.contains(&n)
    }).map(|(i, _, _)| *i)
}

fn cards() -> Vec<(i32, String, String)> {
    alsa::card::Iter::new().filter_map(Result::ok).map(|c| (c.get_index(), c.get_name().unwrap_or_default(), c.get_longname().unwrap_or_default())).collect()
}

fn hw_name(id: Option<&str>) -> Result<(String, String), OutputError> {
    let name = cpal_out::device_name(id).ok_or_else(|| OutputError::DeviceUnavailable("device not found".into()))?;
    let card = card_for(&name, &cards()).ok_or_else(|| OutputError::Other(format!("no ALSA card matches \"{name}\"")))?;
    Ok((format!("hw:{card},0"), name))
}

fn open_pcm(hw: &str) -> Result<PCM, OutputError> {
    PCM::new(hw, Direction::Playback, false).map_err(|e| {
        if e.errno() == libc_ebusy() { OutputError::ExclusiveDenied("device busy (held by PipeWire or another app)".into()) } else { OutputError::DeviceUnavailable(e.to_string()) }
    })
}

const fn libc_ebusy() -> i32 { 16 }

fn alsa_format(s: DeviceSample) -> Option<Format> {
    match s { DeviceSample::I16 => Some(Format::s16()), DeviceSample::I24 => Some(Format::s24_3()), DeviceSample::I32 => Some(Format::s32()), DeviceSample::F32 => None }
}

pub fn caps(id: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let (hw, _) = hw_name(id)?;
    let pcm = open_pcm(&hw)?;
    let hwp = HwParams::any(&pcm).map_err(|e| OutputError::Other(e.to_string()))?;
    let _ = hwp.set_rate_resample(false);
    let rates = COMMON_RATES.into_iter().filter(|&r| hwp.test_rate(r).is_ok()).collect::<Vec<_>>();
    let samples = [DeviceSample::I16, DeviceSample::I24, DeviceSample::I32].into_iter().filter(|&s| alsa_format(s).is_some_and(|f| hwp.test_format(f).is_ok())).collect::<Vec<_>>();
    let max_channels = hwp.get_channels_max().unwrap_or(2).min(16) as u16;
    if rates.is_empty() || samples.is_empty() { return Err(OutputError::Other("no usable hw formats".into())); }
    Ok(DeviceCaps { rates, max_channels, samples, exclusive: true })
}

pub struct AlsaStream { info: StreamInfo, alive: Arc<AtomicBool>, join: Option<JoinHandle<()>> }

impl OutputStream for AlsaStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { Ok(()) }
}

impl Drop for AlsaStream {
    fn drop(&mut self) { self.alive.store(false, Release); if let Some(j) = self.join.take() { let _ = j.join(); } }
}

pub fn open(id: Option<&str>, fmt: OutputFormat, mut source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
    let (hw, name) = hw_name(id)?;
    let format = alsa_format(fmt.sample).ok_or_else(|| OutputError::UnsupportedFormat("float isn't used for ALSA hw".into()))?;
    let alive = Arc::new(AtomicBool::new(true));
    let alive2 = alive.clone();
    let (tx, rx) = mpsc::channel::<Result<(), OutputError>>();
    let bpf = fmt.bytes_per_frame();
    let join = std::thread::Builder::new().name("aurora-alsa".into()).spawn(move || {
        let setup = (|| -> Result<(PCM, usize), OutputError> {
            let pcm = open_pcm(&hw)?;
            {
                let hwp = HwParams::any(&pcm).map_err(|e| OutputError::Other(e.to_string()))?;
                let err = |e: alsa::Error| OutputError::UnsupportedFormat(e.to_string());
                hwp.set_rate_resample(false).map_err(err)?;
                hwp.set_access(Access::RWInterleaved).map_err(err)?;
                hwp.set_format(format).map_err(err)?;
                hwp.set_channels(fmt.channels as u32).map_err(err)?;
                hwp.set_rate(fmt.sample_rate, ValueOr::Nearest).map_err(err)?;
                hwp.set_buffer_time_near(200_000, ValueOr::Nearest).map_err(err)?;
                pcm.hw_params(&hwp).map_err(err)?;
            }
            let cur = pcm.hw_params_current().map_err(|e| OutputError::Other(e.to_string()))?;
            if cur.get_rate().ok() != Some(fmt.sample_rate) { return Err(OutputError::UnsupportedFormat("device refused exact rate".into())); }
            let period = cur.get_period_size().map_err(|e| OutputError::Other(e.to_string()))? as usize;
            Ok((pcm, period))
        })();
        let (pcm, period) = match setup { Ok(v) => v, Err(e) => { let _ = tx.send(Err(e)); return; } };
        let _ = tx.send(Ok(()));
        let io = pcm.io_bytes();
        let mut buf = vec![0u8; period * bpf];
        while alive2.load(Acquire) {
            source.fill(&mut buf);
            if let Err(e) = io.writei(&buf) {
                if pcm.try_recover(e, true).is_err() { source.shared().device_lost.store(true, Release); break; }
            }
        }
        let _ = pcm.drain();
    }).map_err(|e| OutputError::Other(e.to_string()))?;
    match rx.recv() {
        Ok(Ok(())) => Ok(Box::new(AlsaStream {
            info: StreamInfo { device_id: id.unwrap_or("default").into(), device_name: name, transport: Transport::Other, share: ShareMode::Exclusive, format: fmt },
            alive,
            join: Some(join),
        })),
        Ok(Err(e)) => { let _ = join.join(); Err(e) }
        Err(_) => { let _ = join.join(); Err(OutputError::Other("ALSA thread exited during setup".into())) }
    }
}
```
`writei` blocks for at most one period, so `alive` is re-checked about every period. `try_recover` handles underruns (`EPIPE`) by re-preparing the device.

- [ ] **Step 3: Run the tests on Linux**

Run: `cargo test --manifest-path engine/Cargo.toml -p engine-core alsa_hw`, then on a machine with a USB DAC: `cargo test --manifest-path engine/Cargo.toml -p engine-core --test hw_exclusive -- --ignored --nocapture`
Expected: the unit test PASSes. The hardware test plays a tone, or fails with `ExclusiveDenied("device busy…")` if PipeWire holds the card. To free it, run `pactl suspend-sink <sink> 1` or stop playback elsewhere.

- [ ] **Step 4: Commit**

```bash
git add engine/crates/engine-core/src/output/alsa_hw.rs
git commit -m "feat(output): ALSA hw: exclusive backend for Linux"
```

---

### Task 19: JSON-RPC server (`aurora-engine rpc`)

**Files:**
- Create: `engine/crates/aurora-engine/src/rpc/mod.rs`, `src/rpc/writer.rs`, `src/rpc/dispatch.rs`
- Modify: `engine/crates/aurora-engine/src/main.rs` (default command = rpc)
- Test: `engine/crates/aurora-engine/tests/rpc.rs`

**Interfaces:**
- Consumes: `spawn_player`, `PlayerHandle`, `Command`, `PlayerEvent` (Task 14); `SystemBackend`, `NullBackend` (Tasks 11, 15); protocol `rpc::*` param types (Task 1).
- Produces the wire protocol the Electron app depends on:
  - **Methods:** `player.play {queueItemId?}`, `player.pause`, `player.toggle`, `player.stop`, `player.next`, `player.prev`, `player.seek {ms}`, `player.setVolume {volume}`, `player.setMode {mode}`, `player.setDevice {id}`, `player.state` → `PlayerState`, `queue.setPaths {paths, startIndex}`, `queue.get` → `QueueChanged`, `devices.list` → `DeviceInfo[]`, `engine.shutdown`. Commands return `null`.
  - **Notifications:** `engine.ready` (`EngineReady`), `player.state`, `player.signalPath`, `queue.changed`, `devices.changed`, `engine.error`.
  - **Errors:** parse error `-32700` (id null), unknown method `-32601`, bad params `-32602`, `startIndex` out of range → `INVALID_ARGUMENT -32009`.
  - **Environment:** `AURORA_OUTPUT=null` selects `NullBackend` Realtime with `AURORA_NULL_SPEED` (default 1). Stdin EOF → clean exit.

- [ ] **Step 1: Write the failing integration tests**

`tests/rpc.rs`:
```rust
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn fixture(name: &str) -> String { format!("{}/../../../fixtures/out/{name}", env!("CARGO_MANIFEST_DIR")) }

/// `seen` is a window that tests may clear; `all` keeps every message for final assertions.
struct Engine { child: Child, stdin: ChildStdin, rx: Receiver<String>, next_id: u64, seen: Vec<Value>, all: Vec<Value> }

impl Engine {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
            .env("AURORA_OUTPUT", "null").env("AURORA_NULL_SPEED", "10")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || for line in BufReader::new(stdout).lines().map_while(Result::ok) { let _ = tx.send(line); });
        Engine { child, stdin, rx, next_id: 1, seen: vec![], all: vec![] }
    }

    fn next(&mut self, timeout: Duration) -> Value {
        let line = self.rx.recv_timeout(timeout).expect("engine went quiet");
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
        self.seen.push(v.clone());
        self.all.push(v.clone());
        v
    }

    fn wait_for(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        if let Some(v) = self.seen.iter().find(|v| pred(v)) { return v.clone(); }
        loop {
            assert!(start.elapsed() < Duration::from_secs(15), "timed out; seen: {:#?}", self.seen);
            let v = self.next(Duration::from_secs(15));
            if pred(&v) { return v; }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
        self.stdin.flush().unwrap();
        self.wait_for(|v| v["id"] == id)
    }

    fn notified(&mut self, method: &'static str, pred: impl Fn(&Value) -> bool) -> Value {
        self.wait_for(move |v| v["method"] == method && pred(&v["params"]))["params"].clone()
    }
}

impl Drop for Engine { fn drop(&mut self) { let _ = self.child.kill(); } }

#[test]
fn handshake_comes_first() {
    let mut e = Engine::spawn();
    let first = e.next(Duration::from_secs(10));
    assert_eq!(first["method"], "engine.ready");
    assert_eq!(first["params"]["protocolVersion"], 1);
    assert!(first["params"]["features"].as_array().unwrap().iter().any(|f| f == "dsd"));
}

#[test]
fn plays_a_queue_and_reports_signal_path() {
    let mut e = Engine::spawn();
    assert_eq!(e.call("queue.setPaths", json!({ "paths": [fixture("short_a.flac")], "startIndex": 0 }))["result"], Value::Null);
    e.call("player.play", json!({}));
    let sp = e.notified("player.signalPath", |_| true);
    assert_eq!(sp["source"]["codec"], "flac");
    e.notified("player.state", |p| p["status"] == "playing");
    e.seen.clear(); // the "stopped" state from setPaths must not satisfy the next wait
    e.notified("player.state", |p| p["status"] == "stopped");
}

#[test]
fn plays_unicode_path() {
    let dir = std::env::temp_dir().join(format!("aurora-ü-{}", std::process::id())).join("Beyoncé");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("日本語 ♫.flac");
    std::fs::copy(fixture("short_a.flac"), &path).unwrap();
    let mut e = Engine::spawn();
    e.call("queue.setPaths", json!({ "paths": [path.to_string_lossy()], "startIndex": 0 }));
    e.call("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert!(st["track"]["path"].as_str().unwrap().contains("日本語 ♫.flac"));
    e.seen.clear();
    e.notified("player.state", |p| p["status"] == "stopped");
    assert!(!e.all.iter().any(|v| v["method"] == "engine.error"), "{:#?}", e.all);
}

#[test]
fn rpc_errors() {
    let mut e = Engine::spawn();
    assert_eq!(e.call("nope.nothing", json!({}))["error"]["code"], -32601);
    assert_eq!(e.call("player.seek", json!({ "ms": "soon" }))["error"]["code"], -32602);
    assert_eq!(e.call("queue.setPaths", json!({ "paths": [], "startIndex": 3 }))["error"]["code"], -32009);
    writeln!(e.stdin, "{{not json").unwrap();
    e.stdin.flush().unwrap();
    let v = e.wait_for(|v| v["error"]["code"] == -32700);
    assert_eq!(v["id"], Value::Null);
}

#[test]
fn devices_and_state() {
    let mut e = Engine::spawn();
    assert_eq!(e.call("devices.list", json!({}))["result"][0]["id"], "null");
    let st = e.call("player.state", json!({}))["result"].clone();
    assert_eq!(st["status"], "stopped");
    assert_eq!(st["mode"], "processed");
    e.call("player.setMode", json!({ "mode": "bitPerfect" }));
    assert_eq!(e.call("player.state", json!({}))["result"]["mode"], "bitPerfect");
}

#[test]
fn exits_when_stdin_closes() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aurora-engine"))
        .env("AURORA_OUTPUT", "null")
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().unwrap();
    drop(child.stdin.take());
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() { break; }
        assert!(start.elapsed() < Duration::from_secs(3), "engine didn't exit on stdin EOF");
        std::thread::sleep(Duration::from_millis(20));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path engine/Cargo.toml -p aurora-engine --test rpc`
Expected: FAIL (`rpc mode arrives in a later task`).

- [ ] **Step 3: Implement**

`src/rpc/writer.rs`:
```rust
use std::io::Write;
use std::sync::Mutex;

use serde_json::Value;

/// Serializes whole lines to stdout. The only code allowed to write to stdout in rpc mode.
pub struct Writer { lock: Mutex<()> }

impl Writer {
    pub fn new() -> Self { Self { lock: Mutex::new(()) } }
    pub fn send(&self, v: &Value) {
        let _g = self.lock.lock().unwrap();
        let mut out = std::io::stdout().lock();
        let _ = serde_json::to_writer(&mut out, v);
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
    pub fn notify(&self, method: &str, params: impl serde::Serialize) {
        self.send(&serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }
}
```

`src/rpc/dispatch.rs`:
```rust
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
        "queue.get" => Ok(Outcome::Reply(serde_json::to_value(&*ctx.queue.lock().unwrap()).unwrap())),
        "devices.list" => ctx.backend.devices().map(|d| Outcome::Reply(serde_json::to_value(d).unwrap())).map_err(|e| err(e.code().code(), e.to_string())),
        "engine.shutdown" => Ok(Outcome::Shutdown),
        other => Err(err(-32601, format!("method not found: {other}"))),
    }
}
```

`src/rpc/mod.rs`:
```rust
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
                PlayerEvent::Queue(q) => { *queue2.lock().unwrap() = q.clone(); out2.notify("queue.changed", q) }
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
```

In `main.rs`, add `mod rpc;`, a `Rpc` variant to `Cmd`, and route both `Some(Cmd::Rpc)` and `None` to `rpc::run()`.

Also install a panic hook at the top of `main()`, right after `init_logging()`, so that a panic on *any* thread (including an output callback) is logged with a backtrace and the process exits cleanly. Electron's supervisor then restarts the engine (spec §6):
```rust
std::panic::set_hook(Box::new(|info| {
    let bt = std::backtrace::Backtrace::force_capture();
    tracing::error!(panic = %info, backtrace = %bt, "engine panic");
    eprintln!("aurora-engine panic: {info}\n{bt}");
    std::process::exit(70);
}));
```
Add a test to `tests/cli.rs` covering the hidden subcommand `aurora-engine __panic` (used only by tests), which calls `panic!("test panic")`:
```rust
#[test]
fn panics_exit_cleanly_with_code_70() {
    let out = Command::new(env!("CARGO_BIN_EXE_aurora-engine")).arg("__panic").output().unwrap();
    assert_eq!(out.status.code(), Some(70));
    assert!(String::from_utf8_lossy(&out.stderr).contains("test panic"));
}
```
with `#[command(hide = true, name = "__panic")] Panic,` in `Cmd` and `Some(Cmd::Panic) => panic!("test panic"),` in `main`.

- [ ] **Step 4: Run all engine tests**

Run: `cargo test --manifest-path engine/Cargo.toml --workspace`
Expected: everything PASSes, except the `#[ignore]` hardware tests, which are skipped.
Also run: `cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings`. Fix any warnings it reports.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/aurora-engine
git commit -m "feat(engine): JSON-RPC 2.0 stdio server with handshake and event notifications"
```

---
### Task 20: Electron scaffold and the main-process engine client + supervisor

**Files:**
- Create: `app/package.json`, `app/electron.vite.config.ts`, `app/tsconfig.json`, `app/vitest.config.ts`
- Create: `app/src/main/engine/lineCodec.ts`, `app/src/main/engine/EngineClient.ts`, `app/src/main/engine/supervisor.ts`, `app/src/main/engine/enginePath.ts`
- Test: `app/test/lineCodec.test.ts`, `app/test/EngineClient.test.ts`, `app/test/supervisor.test.ts`, `app/test/enginePath.test.ts`

**Interfaces:**
- Consumes: the wire protocol (Task 19) and the generated types in `shared/protocol/*.ts` (Task 1), including `PROTOCOL_VERSION` from `shared/protocol/ProtocolVersion.ts`.
- Produces:
```ts
// lineCodec.ts
export class LineDecoder { push(chunk: string): string[] }
// EngineClient.ts
export interface ChildLike { stdin: NodeJS.WritableStream; stdout: NodeJS.ReadableStream; stderr: NodeJS.ReadableStream; kill(): boolean; on(e: 'exit', cb: (code: number | null) => void): unknown }
export class RpcError extends Error { code: number; data?: unknown }
export class EngineClient extends EventEmitter {   // events: 'notification'(method, params), 'exit'(code), 'log'(line)
  constructor(child: ChildLike, opts: { protocolVersion: number; timeoutMs?: number })
  ready(): Promise<EngineReady>          // rejects with Error & { code: 'PROTOCOL_MISMATCH' } on version mismatch
  call<T = unknown>(method: string, params?: unknown): Promise<T>
  dispose(): void
}
// supervisor.ts
export type RestartDecision = { action: 'restart'; delayMs: number } | { action: 'giveUp' }
export function restartDecision(crashTimes: number[], now: number): RestartDecision
export class EngineSupervisor extends EventEmitter {   // events: 'status'(EngineStatus), 'notification'(method, params), 'log'(line)
  constructor(spawn: () => ChildLike, opts: { protocolVersion: number; now?: () => number; schedule?: (fn: () => void, ms: number) => void })
  start(): void; restart(): void; stop(): Promise<void>
  call<T = unknown>(method: string, params?: unknown): Promise<T>
  get status(): EngineStatus
}
export type EngineStatus = { state: 'starting' | 'ready' | 'restarting' | 'failed' | 'mismatch'; message?: string }
// enginePath.ts
export function enginePath(o: { isPackaged: boolean; resourcesPath: string; appPath: string; platform: string; arch: string; env: Record<string, string | undefined> }): string
```
- **Restart policy (spec §5.2):** wait 0.5 s after the first crash, then 1 s, 2 s, … capped at 8 s. Give up with status `failed` after 3 crashes within 60 s. A protocol mismatch → `mismatch`, and the supervisor never restarts. Session restore after a restart is M2 (`session.json`); in M1 the renderer re-fetches state when status returns to `ready`.

- [ ] **Step 1: Scaffold the app package**

`app/package.json`:
```json
{
  "name": "aurora",
  "version": "0.1.0",
  "private": true,
  "description": "Aurora: a native hi-res music player",
  "license": "MIT",
  "main": "out/main/index.js",
  "scripts": {
    "dev": "electron-vite dev",
    "build": "electron-vite build",
    "test": "vitest run",
    "typecheck": "tsc --noEmit",
    "e2e": "electron-vite build && playwright test"
  },
  "dependencies": {
    "@fontsource/inter": "5.3.0",
    "@fontsource/jetbrains-mono": "5.3.0",
    "lucide-react": "1.55.0",
    "react": "19.3.0",
    "react-dom": "19.3.0",
    "zustand": "5.0.15"
  },
  "devDependencies": {
    "@playwright/test": "1.64.0",
    "@testing-library/react": "16.3.3",
    "@types/node": "^24.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "5.2.0",
    "electron": "44.7.0",
    "electron-vite": "5.0.0",
    "jsdom": "30.1.2",
    "typescript": "5.9.3",
    "vite": "7.3.7",
    "vitest": "5.0.3"
  }
}
```
Run: `npm install --prefix app`. If any pinned version is rejected, run `npm view <pkg> versions` and pick the newest compatible patch. **electron-vite 5 requires Vite ≤ 7, so stay on Vite 7.**

`app/electron.vite.config.ts`:
```ts
import { defineConfig } from 'electron-vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

const alias = { '@shared': resolve('src/shared'), '@protocol': resolve('../shared/protocol') }

export default defineConfig({
  main: { resolve: { alias }, build: { outDir: 'out/main' } },
  preload: { resolve: { alias }, build: { outDir: 'out/preload' } },
  renderer: {
    root: 'src/renderer',
    resolve: { alias },
    plugins: [react()],
    build: { outDir: 'out/renderer', rollupOptions: { input: resolve('src/renderer/index.html') } },
  },
})
```

`app/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "isolatedModules": true,
    "types": ["node", "vite/client"],
    "baseUrl": ".",
    "paths": { "@shared/*": ["src/shared/*"], "@protocol/*": ["../shared/protocol/*"] }
  },
  "include": ["src", "test", "e2e", "../shared/protocol", "*.config.ts"]
}
```

`app/vitest.config.ts`:
```ts
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@shared': fileURLToPath(new URL('./src/shared', import.meta.url)),
      '@protocol': fileURLToPath(new URL('../shared/protocol', import.meta.url)),
    },
  },
  test: { include: ['test/**/*.test.{ts,tsx}'], environment: 'node' },
})
```

- [ ] **Step 2: Write the failing tests**

`app/test/lineCodec.test.ts`:
```ts
import { describe, expect, it } from 'vitest'
import { LineDecoder } from '../src/main/engine/lineCodec'

describe('LineDecoder', () => {
  it('splits complete lines and buffers partial ones', () => {
    const d = new LineDecoder()
    expect(d.push('{"a":1}\n{"b"')).toEqual(['{"a":1}'])
    expect(d.push(':2}\r\n\n')).toEqual(['{"b":2}'])
    expect(d.push('')).toEqual([])
  })
})
```

`app/test/EngineClient.test.ts`:
```ts
import { PassThrough } from 'node:stream'
import { EventEmitter } from 'node:events'
import { describe, expect, it } from 'vitest'
import { EngineClient, RpcError, type ChildLike } from '../src/main/engine/EngineClient'

function fakeChild() {
  const ev = new EventEmitter()
  const child = Object.assign(ev, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: () => { ev.emit('exit', null); return true } })
  const sent: any[] = []
  child.stdin.on('data', (b: Buffer) => b.toString().split('\n').filter(Boolean).forEach((l) => sent.push(JSON.parse(l))))
  const emit = (o: unknown) => child.stdout.write(JSON.stringify(o) + '\n')
  return { child: child as unknown as ChildLike & EventEmitter, sent, emit }
}

const ready = (v = 1) => ({ jsonrpc: '2.0', method: 'engine.ready', params: { version: '0.1.0', protocolVersion: v, features: [] } })

describe('EngineClient', () => {
  it('resolves ready and correlates responses by id', async () => {
    const { child, sent, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    emit(ready())
    await expect(c.ready()).resolves.toMatchObject({ protocolVersion: 1 })
    const p = c.call('player.state')
    await new Promise((r) => setImmediate(r))
    expect(sent[0]).toMatchObject({ jsonrpc: '2.0', method: 'player.state', id: 1 })
    emit({ jsonrpc: '2.0', id: 1, result: { status: 'stopped' } })
    await expect(p).resolves.toEqual({ status: 'stopped' })
  })

  it('maps error responses to RpcError', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    emit(ready())
    const p = c.call('nope')
    emit({ jsonrpc: '2.0', id: 1, error: { code: -32601, message: 'method not found' } })
    await expect(p).rejects.toBeInstanceOf(RpcError)
    await expect(p).rejects.toMatchObject({ code: -32601 })
  })

  it('emits notifications', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const got = new Promise((r) => c.on('notification', (m, p) => r([m, p])))
    emit({ jsonrpc: '2.0', method: 'player.state', params: { status: 'playing' } })
    await expect(got).resolves.toEqual(['player.state', { status: 'playing' }])
  })

  it('rejects ready on protocol mismatch', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    emit(ready(2))
    await expect(c.ready()).rejects.toMatchObject({ code: 'PROTOCOL_MISMATCH' })
  })

  it('rejects pending calls when the engine exits, and times out', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1, timeoutMs: 20 })
    emit(ready())
    await expect(c.call('slow')).rejects.toThrow(/timed out/)
    const p = c.call('player.state')
    child.emit('exit', 101)
    await expect(p).rejects.toThrow(/exited/)
  })

  it('ignores non-JSON stdout lines and reports them as logs', async () => {
    const { child } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const log = new Promise((r) => c.on('log', r))
    child.stdout.write('garbage\n')
    await expect(log).resolves.toMatch(/garbage/)
  })
})
```

`app/test/supervisor.test.ts`:
```ts
import { EventEmitter } from 'node:events'
import { PassThrough } from 'node:stream'
import { describe, expect, it } from 'vitest'
import { EngineSupervisor, restartDecision } from '../src/main/engine/supervisor'

describe('restartDecision', () => {
  it('backs off exponentially and gives up after 3 crashes in 60 s', () => {
    expect(restartDecision([1000], 1000)).toEqual({ action: 'restart', delayMs: 500 })
    expect(restartDecision([1000, 2000], 2000)).toEqual({ action: 'restart', delayMs: 1000 })
    expect(restartDecision([1000, 2000, 3000], 3000)).toEqual({ action: 'giveUp' })
    expect(restartDecision([0, 70_000, 71_000], 71_000)).toEqual({ action: 'restart', delayMs: 1000 })
  })
})

function spawner() {
  const children: any[] = []
  const spawn = () => {
    const ev = new EventEmitter()
    const child = Object.assign(ev, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: () => true })
    children.push(child)
    return child as any
  }
  const ready = (i: number, v = 1) => children[i].stdout.write(JSON.stringify({ jsonrpc: '2.0', method: 'engine.ready', params: { version: 'x', protocolVersion: v, features: [] } }) + '\n')
  return { spawn, children, ready }
}

describe('EngineSupervisor', () => {
  it('restarts after a crash and gives up after three', async () => {
    let now = 0
    const timers: Array<() => void> = []
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1, now: () => now, schedule: (fn) => timers.push(fn) })
    const states: string[] = []
    sup.on('status', (st) => states.push(st.state))
    sup.start()
    s.ready(0)
    await new Promise((r) => setImmediate(r))
    expect(sup.status.state).toBe('ready')
    for (let i = 0; i < 3; i++) {
      now += 1000
      s.children[i].emit('exit', 1)
      if (i < 2) { timers.shift()!(); s.ready(i + 1); await new Promise((r) => setImmediate(r)) }
    }
    expect(states).toEqual(['starting', 'ready', 'restarting', 'starting', 'ready', 'restarting', 'starting', 'ready', 'failed'])
    expect(s.children).toHaveLength(3)
  })

  it('does not restart on protocol mismatch', async () => {
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1, schedule: () => { throw new Error('must not schedule') } })
    sup.start()
    s.ready(0, 9)
    await new Promise((r) => setImmediate(r))
    expect(sup.status.state).toBe('mismatch')
  })

  it('rejects calls while not ready', async () => {
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1 })
    sup.start()
    await expect(sup.call('player.state')).rejects.toThrow(/not ready/)
  })
})
```

`app/test/enginePath.test.ts`:
```ts
import { describe, expect, it } from 'vitest'
import { join } from 'node:path'
import { enginePath } from '../src/main/engine/enginePath'

describe('enginePath', () => {
  const base = { resourcesPath: '/res', appPath: '/repo/app', arch: 'x64', env: {} }
  it('uses the cargo target dir in dev', () => {
    expect(enginePath({ ...base, isPackaged: false, platform: 'linux' })).toBe(join('/repo/app', '..', 'engine', 'target', 'debug', 'aurora-engine'))
  })
  it('uses resources/bin/<platform>-<arch> when packaged, with .exe on Windows', () => {
    expect(enginePath({ ...base, isPackaged: true, platform: 'win32' })).toBe(join('/res', 'bin', 'win32-x64', 'aurora-engine.exe'))
  })
  it('honours AURORA_ENGINE_PATH', () => {
    expect(enginePath({ ...base, isPackaged: true, platform: 'darwin', env: { AURORA_ENGINE_PATH: '/x/engine' } })).toBe('/x/engine')
  })
})
```

- [ ] **Step 3: Run to verify they fail**

Run: `npm --prefix app test`
Expected: FAIL (modules not found).

- [ ] **Step 4: Implement**

`app/src/main/engine/lineCodec.ts`:
```ts
/** Splits a UTF-8 text stream into newline-delimited lines (CRLF tolerated, blank lines dropped). */
export class LineDecoder {
  private buf = ''
  push(chunk: string): string[] {
    this.buf += chunk
    const parts = this.buf.split('\n')
    this.buf = parts.pop() ?? ''
    return parts.map((l) => l.replace(/\r$/, '')).filter((l) => l.length > 0)
  }
}
```

`app/src/main/engine/EngineClient.ts`:
```ts
import { EventEmitter } from 'node:events'
import type { EngineReady } from '@protocol/EngineReady'
import { LineDecoder } from './lineCodec'

export interface ChildLike {
  stdin: NodeJS.WritableStream
  stdout: NodeJS.ReadableStream
  stderr: NodeJS.ReadableStream
  kill(): boolean
  on(e: 'exit', cb: (code: number | null) => void): unknown
}

export class RpcError extends Error {
  constructor(public code: number, message: string, public data?: unknown) { super(message); this.name = 'RpcError' }
}

type Pending = { resolve: (v: any) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }

export class EngineClient extends EventEmitter {
  private pending = new Map<number, Pending>()
  private nextId = 1
  private exited = false
  private readyPromise: Promise<EngineReady>
  private settleReady!: { resolve: (r: EngineReady) => void; reject: (e: Error) => void }

  constructor(private child: ChildLike, private opts: { protocolVersion: number; timeoutMs?: number }) {
    super()
    this.readyPromise = new Promise((resolve, reject) => { this.settleReady = { resolve, reject } })
    this.readyPromise.catch(() => {}) // observed via ready()
    const out = new LineDecoder()
    const err = new LineDecoder()
    child.stdout.setEncoding?.('utf8')
    child.stderr.setEncoding?.('utf8')
    child.stdout.on('data', (c: string | Buffer) => out.push(c.toString()).forEach((l) => this.onLine(l)))
    child.stderr.on('data', (c: string | Buffer) => err.push(c.toString()).forEach((l) => this.emit('log', l)))
    child.on('exit', (code) => {
      this.exited = true
      const e = new Error(`engine exited (code ${code})`)
      this.settleReady.reject(e)
      for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(e) }
      this.pending.clear()
      this.emit('exit', code)
    })
  }

  private onLine(line: string) {
    let msg: any
    try { msg = JSON.parse(line) } catch { this.emit('log', `non-JSON stdout line: ${line}`); return }
    if (msg.id !== undefined && msg.id !== null && ('result' in msg || 'error' in msg)) {
      const p = this.pending.get(msg.id)
      if (!p) return
      this.pending.delete(msg.id)
      clearTimeout(p.timer)
      if (msg.error) p.reject(new RpcError(msg.error.code, msg.error.message, msg.error.data))
      else p.resolve(msg.result)
      return
    }
    if (msg.method === 'engine.ready') {
      const r = msg.params as EngineReady
      if (r.protocolVersion !== this.opts.protocolVersion) {
        this.settleReady.reject(Object.assign(new Error(`protocol ${r.protocolVersion} != ${this.opts.protocolVersion}`), { code: 'PROTOCOL_MISMATCH' }))
      } else this.settleReady.resolve(r)
      return
    }
    if (typeof msg.method === 'string') this.emit('notification', msg.method, msg.params)
  }

  ready(): Promise<EngineReady> { return this.readyPromise }

  call<T = unknown>(method: string, params: unknown = {}): Promise<T> {
    if (this.exited) return Promise.reject(new Error('engine exited'))
    const id = this.nextId++
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`${method} timed out`)) }, this.opts.timeoutMs ?? 10_000)
      this.pending.set(id, { resolve, reject, timer })
      this.child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n')
    })
  }

  dispose() { this.exited = true; this.child.kill() }
}
```

`app/src/main/engine/supervisor.ts`:
```ts
import { EventEmitter } from 'node:events'
import { EngineClient, type ChildLike } from './EngineClient'

export type EngineStatus = { state: 'starting' | 'ready' | 'restarting' | 'failed' | 'mismatch'; message?: string }
export type RestartDecision = { action: 'restart'; delayMs: number } | { action: 'giveUp' }

export function restartDecision(crashTimes: number[], now: number): RestartDecision {
  const recent = crashTimes.filter((t) => now - t <= 60_000)
  if (recent.length >= 3) return { action: 'giveUp' }
  return { action: 'restart', delayMs: Math.min(8_000, 500 * 2 ** Math.max(0, recent.length - 1)) }
}

export class EngineSupervisor extends EventEmitter {
  private client: EngineClient | null = null
  private isReady = false
  private crashes: number[] = []
  private stopping = false
  private current: EngineStatus = { state: 'starting' }
  private now: () => number
  private schedule: (fn: () => void, ms: number) => void

  constructor(private spawn: () => ChildLike, private opts: { protocolVersion: number; now?: () => number; schedule?: (fn: () => void, ms: number) => void }) {
    super()
    this.now = opts.now ?? Date.now
    this.schedule = opts.schedule ?? ((fn, ms) => { setTimeout(fn, ms) })
  }

  get status(): EngineStatus { return this.current }

  private setStatus(s: EngineStatus) { this.current = s; this.emit('status', s) }

  start() {
    this.stopping = false
    this.isReady = false
    this.setStatus({ state: 'starting' })
    let child: ChildLike
    try { child = this.spawn() } catch (e) { this.setStatus({ state: 'failed', message: `Couldn't start the audio engine: ${(e as Error).message}` }); return }
    const client = new EngineClient(child, { protocolVersion: this.opts.protocolVersion })
    this.client = client
    client.on('notification', (m, p) => this.emit('notification', m, p))
    client.on('log', (l) => this.emit('log', l))
    client.on('exit', () => this.onExit(client))
    client.ready().then(
      () => { if (this.client === client) { this.isReady = true; this.setStatus({ state: 'ready' }) } },
      (e) => {
        if (e?.code === 'PROTOCOL_MISMATCH' && this.client === client) {
          this.stopping = true
          this.setStatus({ state: 'mismatch', message: 'Engine version mismatch: reinstall Aurora.' })
          client.dispose()
        }
      },
    )
  }

  private onExit(client: EngineClient) {
    if (this.client !== client) return
    this.isReady = false
    if (this.stopping) return
    this.crashes.push(this.now())
    const d = restartDecision(this.crashes, this.now())
    if (d.action === 'giveUp') { this.setStatus({ state: 'failed', message: 'The audio engine crashed 3 times in a minute.' }); return }
    this.setStatus({ state: 'restarting' })
    this.schedule(() => { if (!this.stopping) this.start() }, d.delayMs)
  }

  restart() {
    this.crashes = []
    this.stopping = true
    this.client?.dispose()
    this.start()
  }

  async stop() {
    this.stopping = true
    const c = this.client
    if (!c) return
    await Promise.race([c.call('engine.shutdown').catch(() => {}), new Promise((r) => setTimeout(r, 1000))])
    c.dispose()
  }

  call<T = unknown>(method: string, params?: unknown): Promise<T> {
    if (!this.client || !this.isReady) return Promise.reject(new Error('engine not ready'))
    return this.client.call<T>(method, params)
  }
}
```

`app/src/main/engine/enginePath.ts`:
```ts
import { join } from 'node:path'

export function enginePath(o: { isPackaged: boolean; resourcesPath: string; appPath: string; platform: string; arch: string; env: Record<string, string | undefined> }): string {
  if (o.env.AURORA_ENGINE_PATH) return o.env.AURORA_ENGINE_PATH
  const exe = o.platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'
  if (o.isPackaged) return join(o.resourcesPath, 'bin', `${o.platform}-${o.arch}`, exe)
  return join(o.appPath, '..', 'engine', 'target', o.env.AURORA_ENGINE_PROFILE ?? 'debug', exe)
}
```

- [ ] **Step 5: Run the tests**

Run: `npm --prefix app test`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add app/package.json app/package-lock.json app/electron.vite.config.ts app/tsconfig.json app/vitest.config.ts app/src/main app/test
git commit -m "feat(app): electron scaffold, JSON-RPC engine client and crash supervisor"
```

---

### Task 21: Main window, preload API and IPC bridge

**Files:**
- Create: `app/src/shared/api.ts`, `app/src/main/index.ts`, `app/src/main/logs.ts`, `app/src/preload/index.ts`, `app/src/renderer/env.d.ts`
- Test: `app/test/api.test.ts`

**Interfaces:**
- Consumes: `EngineSupervisor`, `enginePath` (Task 20); generated protocol types.
- Produces:
```ts
// shared/api.ts
export type Methods = { 'player.play': { params: PlayParams; result: null }; …; 'player.state': { params: {}; result: PlayerState }; 'queue.setPaths': { params: QueueSetPathsParams; result: null }; 'queue.get': { params: {}; result: QueueChanged }; 'devices.list': { params: {}; result: DeviceInfo[] } }
export type Events = { 'player.state': PlayerState; 'player.signalPath': SignalPath; 'queue.changed': QueueChanged; 'devices.changed': DeviceInfo[]; 'engine.error': EngineErrorEvent; 'engine.status': EngineStatus }
export const ALLOWED_METHODS: readonly (keyof Methods)[]
export function isAllowedMethod(m: unknown): m is keyof Methods
export const AUDIO_EXTENSIONS: readonly string[]
export interface AuroraApi {
  call<M extends keyof Methods>(method: M, params: Methods[M]['params']): Promise<Methods[M]['result']>
  on<E extends keyof Events>(event: E, cb: (payload: Events[E]) => void): () => void
  openFiles(): Promise<string[]>
  pathsForFiles(files: File[]): string[]
  status(): Promise<EngineStatus>
  restartEngine(): Promise<void>
  openLogs(): Promise<void>
  platform: string
}
```
- **IPC channels:** `aurora:call`, `aurora:openFiles`, `aurora:status`, `aurora:restartEngine`, `aurora:openLogs` (invoke), and `aurora:event` (main → renderer, args `(event, payload)`). Main rejects any method not in `ALLOWED_METHODS`. `AURORA_TEST_OPEN_FILES` (a `path.delimiter`-separated list) bypasses the file dialog, for e2e tests.

- [ ] **Step 1: Write the failing test**

`app/test/api.test.ts`:
```ts
import { describe, expect, it } from 'vitest'
import { ALLOWED_METHODS, AUDIO_EXTENSIONS, isAllowedMethod } from '../src/shared/api'

describe('api allowlist', () => {
  it('allows exactly the M1 engine methods', () => {
    expect([...ALLOWED_METHODS].sort()).toEqual([
      'devices.list', 'player.next', 'player.pause', 'player.play', 'player.prev', 'player.seek', 'player.setDevice',
      'player.setMode', 'player.setVolume', 'player.state', 'player.stop', 'player.toggle', 'queue.get', 'queue.setPaths',
    ])
    expect(isAllowedMethod('engine.shutdown')).toBe(false)
    expect(isAllowedMethod({})).toBe(false)
  })
  it('lists every supported audio extension', () => {
    for (const e of ['mp3', 'm4a', 'aac', 'flac', 'wav', 'aiff', 'ogg', 'opus', 'dsf', 'dff']) expect(AUDIO_EXTENSIONS).toContain(e)
  })
})
```
Run: `npm --prefix app test -- api` → FAIL.

- [ ] **Step 2: Implement `shared/api.ts`**

```ts
import type { DeviceInfo } from '@protocol/DeviceInfo'
import type { DeviceParams } from '@protocol/DeviceParams'
import type { EngineErrorEvent } from '@protocol/EngineErrorEvent'
import type { ModeParams } from '@protocol/ModeParams'
import type { PlayParams } from '@protocol/PlayParams'
import type { PlayerState } from '@protocol/PlayerState'
import type { QueueChanged } from '@protocol/QueueChanged'
import type { QueueSetPathsParams } from '@protocol/QueueSetPathsParams'
import type { SeekParams } from '@protocol/SeekParams'
import type { SignalPath } from '@protocol/SignalPath'
import type { VolumeParams } from '@protocol/VolumeParams'

export type EngineStatus = { state: 'starting' | 'ready' | 'restarting' | 'failed' | 'mismatch'; message?: string }
type None = Record<string, never>

export type Methods = {
  'player.play': { params: PlayParams; result: null }
  'player.pause': { params: None; result: null }
  'player.toggle': { params: None; result: null }
  'player.stop': { params: None; result: null }
  'player.next': { params: None; result: null }
  'player.prev': { params: None; result: null }
  'player.seek': { params: SeekParams; result: null }
  'player.setVolume': { params: VolumeParams; result: null }
  'player.setMode': { params: ModeParams; result: null }
  'player.setDevice': { params: DeviceParams; result: null }
  'player.state': { params: None; result: PlayerState }
  'queue.setPaths': { params: QueueSetPathsParams; result: null }
  'queue.get': { params: None; result: QueueChanged }
  'devices.list': { params: None; result: DeviceInfo[] }
}

export type Events = {
  'player.state': PlayerState
  'player.signalPath': SignalPath
  'queue.changed': QueueChanged
  'devices.changed': DeviceInfo[]
  'engine.error': EngineErrorEvent
  'engine.status': EngineStatus
}

export const ALLOWED_METHODS = [
  'player.play', 'player.pause', 'player.toggle', 'player.stop', 'player.next', 'player.prev', 'player.seek',
  'player.setVolume', 'player.setMode', 'player.setDevice', 'player.state', 'queue.setPaths', 'queue.get', 'devices.list',
] as const satisfies readonly (keyof Methods)[]

export function isAllowedMethod(m: unknown): m is keyof Methods {
  return typeof m === 'string' && (ALLOWED_METHODS as readonly string[]).includes(m)
}

export const AUDIO_EXTENSIONS = ['mp3', 'm4a', 'm4b', 'mp4', 'aac', 'alac', 'flac', 'wav', 'aif', 'aiff', 'aifc', 'ogg', 'oga', 'opus', 'dsf', 'dff'] as const

export interface AuroraApi {
  call<M extends keyof Methods>(method: M, params: Methods[M]['params']): Promise<Methods[M]['result']>
  on<E extends keyof Events>(event: E, cb: (payload: Events[E]) => void): () => void
  openFiles(): Promise<string[]>
  pathsForFiles(files: File[]): string[]
  status(): Promise<EngineStatus>
  restartEngine(): Promise<void>
  openLogs(): Promise<void>
  platform: string
}
```
Run the test again → PASS. Make `supervisor.ts` import `EngineStatus` from `@shared/api` instead of redeclaring it.

- [ ] **Step 3: Implement `logs.ts`, `main/index.ts`, `preload/index.ts`, `env.d.ts`**

`app/src/main/logs.ts`:
```ts
import { app } from 'electron'
import { appendFile, mkdirSync } from 'node:fs'
import { join } from 'node:path'

export const logDir = () => join(app.getPath('userData'), 'logs')

export function logger(file: string) {
  const dir = logDir()
  mkdirSync(dir, { recursive: true })
  const path = join(dir, file)
  return (line: string) => appendFile(path, `${new Date().toISOString()} ${line}\n`, () => {})
}
```

`app/src/main/index.ts`:
```ts
import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron'
import { spawn } from 'node:child_process'
import { delimiter, join } from 'node:path'
import { PROTOCOL_VERSION } from '@protocol/ProtocolVersion'
import { AUDIO_EXTENSIONS, isAllowedMethod } from '@shared/api'
import { enginePath } from './engine/enginePath'
import { EngineSupervisor } from './engine/supervisor'
import { logDir, logger } from './logs'

let win: BrowserWindow | null = null

const binary = () => enginePath({ isPackaged: app.isPackaged, resourcesPath: process.resourcesPath, appPath: app.getAppPath(), platform: process.platform, arch: process.arch, env: process.env })

const supervisor = new EngineSupervisor(() => spawn(binary(), ['rpc'], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }), { protocolVersion: PROTOCOL_VERSION })

function send(event: string, payload: unknown) {
  if (win && !win.isDestroyed()) win.webContents.send('aurora:event', event, payload)
}

function createWindow() {
  const mac = process.platform === 'darwin'
  win = new BrowserWindow({
    width: 1280, height: 800, minWidth: 940, minHeight: 600, show: false,
    backgroundColor: '#17151A',
    titleBarStyle: mac ? 'hiddenInset' : 'hidden',
    titleBarOverlay: mac ? undefined : { color: '#100E12', symbolColor: '#EFE9E1', height: 36 },
    webPreferences: { preload: join(__dirname, '../preload/index.js'), contextIsolation: true, nodeIntegration: false, sandbox: true },
  })
  win.once('ready-to-show', () => win?.show())
  if (process.env.ELECTRON_RENDERER_URL) win.loadURL(process.env.ELECTRON_RENDERER_URL)
  else win.loadFile(join(__dirname, '../renderer/index.html'))
}

app.whenReady().then(() => {
  const engineLog = logger('engine.log')
  supervisor.on('log', engineLog)
  supervisor.on('notification', (m: string, p: unknown) => send(m, p))
  supervisor.on('status', (s) => { send('engine.status', s); engineLog(`[status] ${JSON.stringify(s)}`) })

  ipcMain.handle('aurora:call', (_e, method: unknown, params: unknown) => {
    if (!isAllowedMethod(method)) throw new Error(`method not allowed: ${String(method)}`)
    return supervisor.call(method, params ?? {})
  })
  ipcMain.handle('aurora:openFiles', async () => {
    const test = process.env.AURORA_TEST_OPEN_FILES
    if (test) return test.split(delimiter).filter(Boolean)
    const r = await dialog.showOpenDialog(win!, { properties: ['openFile', 'multiSelections'], filters: [{ name: 'Audio', extensions: [...AUDIO_EXTENSIONS] }] })
    return r.canceled ? [] : r.filePaths
  })
  ipcMain.handle('aurora:status', () => supervisor.status)
  ipcMain.handle('aurora:restartEngine', () => supervisor.restart())
  ipcMain.handle('aurora:openLogs', async () => { await shell.openPath(logDir()) })

  supervisor.start()
  createWindow()
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow() })
})

app.on('before-quit', () => { void supervisor.stop() })
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
```

`app/src/preload/index.ts`:
```ts
import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron'
import type { AuroraApi } from '../shared/api'

const api: AuroraApi = {
  call: (method, params) => ipcRenderer.invoke('aurora:call', method, params),
  on: (event, cb) => {
    const handler = (_e: IpcRendererEvent, name: string, payload: unknown) => { if (name === event) cb(payload as never) }
    ipcRenderer.on('aurora:event', handler)
    return () => { ipcRenderer.removeListener('aurora:event', handler) }
  },
  openFiles: () => ipcRenderer.invoke('aurora:openFiles'),
  pathsForFiles: (files) => files.map((f) => webUtils.getPathForFile(f)).filter(Boolean),
  status: () => ipcRenderer.invoke('aurora:status'),
  restartEngine: () => ipcRenderer.invoke('aurora:restartEngine'),
  openLogs: () => ipcRenderer.invoke('aurora:openLogs'),
  platform: process.platform,
}

contextBridge.exposeInMainWorld('aurora', api)
```

`app/src/renderer/env.d.ts`:
```ts
import type { AuroraApi } from '../shared/api'
declare global { interface Window { aurora: AuroraApi } }
export {}
```

- [ ] **Step 4: Typecheck**

Run: `npm --prefix app run typecheck`
Expected: no errors. (The renderer entry is added in Task 22. If `tsc` complains that `src/renderer` has no inputs, that resolves once Task 22 lands; until then, check that the main and preload files type-check cleanly.)

- [ ] **Step 5: Commit**

```bash
git add app/src/shared app/src/main app/src/preload app/src/renderer/env.d.ts app/test/api.test.ts
git commit -m "feat(app): secure preload API, IPC allowlist and main window"
```

---

### Task 22: Renderer: Studio design system, player store and M1 screens

**Files:**
- Create: `app/src/renderer/index.html`, `main.tsx`, `App.tsx`, `App.module.css`
- Create: `app/src/renderer/styles/tokens.css`, `styles/global.css`
- Create: `app/src/renderer/lib/format.ts`, `app/src/renderer/stores/player.ts`
- Create: `app/src/renderer/components/{IconRail,PlayerBar,SeekBar,Transport,FormatBadges,SignalPathPanel,DevicePicker,Home,EngineBanner}.tsx` + matching `.module.css`
- Test: `app/test/format.test.ts`, `app/test/store.test.ts`, `app/test/SignalPathPanel.test.tsx`, `app/test/PlayerBar.test.tsx`

**Interfaces:**
- Consumes: `AuroraApi`, `Methods`, `Events`, `EngineStatus` (Task 21).
- Produces:
```ts
// lib/format.ts
export function formatTime(ms: number): string                 // 0 → "0:00", 3_723_000 → "1:02:03"
export function khz(rate: number): string                      // 44100 → "44.1", 96000 → "96"
export function codecLabel(src: SourceSummary): string          // "FLAC", "HE-AAC", "WAV"…
export function formatBadge(src: SourceSummary): string         // "FLAC 24/96", "MP3 320k", "DSD64"
export function bitPerfectLabel(sp: SignalPath): string | null  // "Bit-perfect" | "Bit-perfect to OS" | null
export function bluetoothLabel(bt: BluetoothInfo): string       // "LDAC · 990 kbps" | "AAC" | "Codec not reported by the OS"
// stores/player.ts
export const usePlayer: UseBoundStore<StoreApi<PlayerStore>>
export interface PlayerStore { api; state: PlayerState; stamp: number; signal: SignalPath | null; queue: QueueChanged; devices: DeviceInfo[]; engine: EngineStatus; error: EngineErrorEvent | null; panel: 'queue' | 'signal';
  connect(api: AuroraApi): () => void; openFiles(): Promise<void>; playPaths(paths: string[]): Promise<void>; playItem(id: number): Promise<void>;
  toggle(): Promise<void>; next(): Promise<void>; prev(): Promise<void>; seek(ms: number): Promise<void>; setVolume(v: number): Promise<void>;
  setMode(m: OutputMode): Promise<void>; setDevice(id: string | null): Promise<void>; setPanel(p: 'queue' | 'signal'): void; dismissError(): void }
export function livePosition(s: Pick<PlayerStore, 'state' | 'stamp'>, now: number): number
```
- **UI contract (tested or used by e2e):**
  - Play/pause button `aria-label` is "Play" or "Pause". Mode control: `role="switch"` with `aria-label="Bit-perfect"`. Volume slider `aria-label="Volume"`, disabled in bit-perfect mode with title "Bit-perfect: use your system or DAC volume".
  - `data-testid`s: `now-title`, `format-badge`, `bitperfect-badge`, `signal-output`.
  - The signal-path node for an inactive stage carries `data-active="false"` and is drawn dashed.
  - The icon-rail button that opens the panel has `aria-label="Signal path"`.
  - "Open files" button. Opening files sets the queue *and* starts playback.
  - Text-to-speech-friendly labels: the visible badges use CSS `text-transform: uppercase`, while the DOM text stays e.g. "Bit-perfect to OS".

- [ ] **Step 1: Write the failing tests**

`app/test/format.test.ts`:
```ts
import { describe, expect, it } from 'vitest'
import { bitPerfectLabel, bluetoothLabel, formatBadge, formatTime, khz } from '../src/renderer/lib/format'

const src = (o: object) => ({ container: 'flac', codec: 'flac', bitrateKbps: 2304, sampleRate: 96000, bitDepth: 24, channels: 2, dsdRate: null, ...o }) as any

describe('format', () => {
  it('times', () => {
    expect(formatTime(0)).toBe('0:00')
    expect(formatTime(61_999)).toBe('1:01')
    expect(formatTime(3_723_000)).toBe('1:02:03')
  })
  it('rates', () => { expect(khz(44100)).toBe('44.1'); expect(khz(96000)).toBe('96'); expect(khz(352800)).toBe('352.8') })
  it('badges', () => {
    expect(formatBadge(src({}))).toBe('FLAC 24/96')
    expect(formatBadge(src({ codec: 'mp3', container: 'mp3', bitDepth: null, sampleRate: 44100, bitrateKbps: 320 }))).toBe('MP3 320k')
    expect(formatBadge(src({ codec: 'he-aac', container: 'm4a', bitDepth: 16, sampleRate: 48000, bitrateKbps: 64 }))).toBe('HE-AAC 64k')
    expect(formatBadge(src({ codec: 'pcm', container: 'wav', bitDepth: 16, sampleRate: 44100 }))).toBe('WAV 16/44.1')
    expect(formatBadge(src({ codec: 'dsd', container: 'dsf', dsdRate: 2822400, sampleRate: 88200, bitDepth: null }))).toBe('DSD64')
  })
  it('bit-perfect labels follow the Bluetooth honesty rule', () => {
    expect(bitPerfectLabel({ bitPerfect: true, bitPerfectScope: 'device' } as any)).toBe('Bit-perfect')
    expect(bitPerfectLabel({ bitPerfect: true, bitPerfectScope: 'os' } as any)).toBe('Bit-perfect to OS')
    expect(bitPerfectLabel({ bitPerfect: false, bitPerfectScope: null } as any)).toBeNull()
  })
  it('bluetooth labels never guess', () => {
    expect(bluetoothLabel({ codec: 'ldac', quality: 'hq', bitrateKbps: 990 })).toBe('LDAC · 990 kbps')
    expect(bluetoothLabel({ codec: 'ldac', quality: 'auto', bitrateKbps: null })).toBe('LDAC · auto')
    expect(bluetoothLabel({ codec: 'aac', quality: null, bitrateKbps: null })).toBe('AAC')
    expect(bluetoothLabel({ codec: null, quality: null, bitrateKbps: null })).toBe('Codec not reported by the OS')
  })
})
```

`app/test/store.test.ts`:
```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { livePosition, usePlayer } from '../src/renderer/stores/player'

function fakeApi() {
  const handlers = new Map<string, (p: any) => void>()
  const api = {
    call: vi.fn(async (m: string) => (m === 'player.state' ? { status: 'stopped', queueIndex: null, track: null, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null } : m === 'devices.list' ? [] : m === 'queue.get' ? { items: [], current: null } : null)),
    on: vi.fn((e: string, cb: any) => { handlers.set(e, cb); return () => handlers.delete(e) }),
    openFiles: vi.fn(async () => ['/m/a.flac', '/m/b.flac']),
    pathsForFiles: vi.fn(() => []),
    status: vi.fn(async () => ({ state: 'ready' })),
    restartEngine: vi.fn(), openLogs: vi.fn(), platform: 'linux',
  }
  return { api, emit: (e: string, p: any) => handlers.get(e)?.(p) }
}

describe('player store', () => {
  beforeEach(() => usePlayer.setState(usePlayer.getInitialState()))

  it('applies engine events', async () => {
    const { api, emit } = fakeApi()
    usePlayer.getState().connect(api as any)
    emit('player.state', { status: 'playing', positionMs: 1500, volume: 0.5, mode: 'processed' })
    expect(usePlayer.getState().state.status).toBe('playing')
    emit('player.signalPath', { bitPerfect: true })
    expect(usePlayer.getState().signal).toEqual({ bitPerfect: true })
    emit('engine.error', { code: -32003, name: 'UNSUPPORTED_FORMAT', message: 'x', path: '/a' })
    expect(usePlayer.getState().error?.name).toBe('UNSUPPORTED_FORMAT')
  })

  it('openFiles sets the queue and plays', async () => {
    const { api } = fakeApi()
    usePlayer.getState().connect(api as any)
    await usePlayer.getState().openFiles()
    expect(api.call).toHaveBeenCalledWith('queue.setPaths', { paths: ['/m/a.flac', '/m/b.flac'], startIndex: 0 })
    expect(api.call).toHaveBeenCalledWith('player.play', {})
  })

  it('interpolates position only while playing', () => {
    expect(livePosition({ state: { status: 'playing', positionMs: 1000 } as any, stamp: 100 }, 350)).toBe(1250)
    expect(livePosition({ state: { status: 'paused', positionMs: 1000 } as any, stamp: 100 }, 350)).toBe(1000)
    expect(livePosition({ state: { status: 'playing', positionMs: 1000, durationMs: 1100 } as any, stamp: 0 }, 5000)).toBe(1100)
  })
})
```

`app/test/SignalPathPanel.test.tsx`:
```tsx
// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { SignalPathPanel } from '../src/renderer/components/SignalPathPanel'

const base = {
  source: { container: 'm4a', codec: 'aac-lc', bitrateKbps: 320, sampleRate: 44100, bitDepth: null, channels: 2, dsdRate: null },
  decoder: { name: 'symphonia', gapless: { delay: 2112, padding: 1024 } },
  stages: [
    { kind: 'channels', active: false, detail: 'unchanged' },
    { kind: 'resampler', active: false, detail: 'rate matched' },
    { kind: 'volume', active: true, detail: '82%' },
  ],
  output: { deviceId: 'd', name: 'WH-1000XM5', transport: 'bluetooth', mode: 'shared', sampleRate: 44100, format: 'f32' },
  bluetooth: { codec: null, quality: null, bitrateKbps: null },
  bitPerfect: false, bitPerfectScope: null, underruns: 0, notes: ['Exclusive mode unavailable — using processed mode'],
} as any

afterEach(cleanup)

describe('SignalPathPanel', () => {
  it('draws the chain with inactive stages marked and honest Bluetooth text', () => {
    render(<SignalPathPanel signal={base} />)
    expect(screen.getByText('M4A · AAC')).toBeTruthy()
    expect(screen.getByText('rate matched').closest('[data-active]')?.getAttribute('data-active')).toBe('false')
    expect(screen.getByText('Codec not reported by the OS')).toBeTruthy()
    expect(screen.getByTestId('signal-output').textContent).toContain('WH-1000XM5')
    expect(screen.getByText('Exclusive mode unavailable — using processed mode')).toBeTruthy()
  })

  it('shows LDAC and the OS-scoped badge', () => {
    render(<SignalPathPanel signal={{ ...base, bitPerfect: true, bitPerfectScope: 'os', bluetooth: { codec: 'ldac', quality: 'hq', bitrateKbps: 990 } }} />)
    expect(screen.getByText('LDAC · 990 kbps')).toBeTruthy()
    expect(screen.getByText('Bit-perfect to OS')).toBeTruthy()
  })

  it('renders an empty state with no signal', () => {
    render(<SignalPathPanel signal={null} />)
    expect(screen.getByText(/play something/i)).toBeTruthy()
  })
})
```

`app/test/PlayerBar.test.tsx`:
```tsx
// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PlayerBar } from '../src/renderer/components/PlayerBar'
import { usePlayer } from '../src/renderer/stores/player'

afterEach(cleanup)

describe('PlayerBar', () => {
  beforeEach(() => usePlayer.setState(usePlayer.getInitialState()))

  it('toggles playback through the store', () => {
    const toggle = vi.fn(async () => {})
    usePlayer.setState({ toggle, state: { ...usePlayer.getState().state, status: 'paused', track: { path: '/a.flac', title: 'a', durationMs: 1000 } } })
    render(<PlayerBar />)
    fireEvent.click(screen.getByRole('button', { name: 'Play' }))
    expect(toggle).toHaveBeenCalled()
    expect(screen.getByTestId('now-title').textContent).toBe('a')
  })

  it('disables software volume in bit-perfect mode', () => {
    usePlayer.setState({ state: { ...usePlayer.getState().state, mode: 'bitPerfect' } })
    render(<PlayerBar />)
    const vol = screen.getByRole('slider', { name: 'Volume' }) as HTMLInputElement
    expect(vol.disabled).toBe(true)
    expect(vol.title).toBe('Bit-perfect: use your system or DAC volume')
    expect(screen.getByRole('switch', { name: 'Bit-perfect' }).getAttribute('aria-checked')).toBe('true')
  })
})
```

- [ ] **Step 2: Run to verify they fail**

Run: `npm --prefix app test`
Expected: the new tests FAIL (modules missing).

- [ ] **Step 3: Implement `lib/format.ts` and `stores/player.ts`**

`app/src/renderer/lib/format.ts`:
```ts
import type { BluetoothInfo } from '@protocol/BluetoothInfo'
import type { SignalPath } from '@protocol/SignalPath'
import type { SourceSummary } from '@protocol/SourceSummary'

export function formatTime(ms: number): string {
  const t = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60
  const ss = String(s).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`
}

export function khz(rate: number): string {
  const k = rate / 1000
  return Number.isInteger(k) ? String(k) : k.toFixed(1)
}

const LABELS: Record<string, string> = { mp3: 'MP3', 'aac-lc': 'AAC', 'he-aac': 'HE-AAC', 'he-aac-v2': 'HE-AACv2', alac: 'ALAC', flac: 'FLAC', vorbis: 'Vorbis', opus: 'Opus', dsd: 'DSD' }
const LOSSLESS = new Set(['flac', 'alac', 'pcm'])

export function codecLabel(src: SourceSummary): string {
  return src.codec === 'pcm' ? src.container.toUpperCase() : (LABELS[src.codec] ?? src.codec.toUpperCase())
}

export function formatBadge(src: SourceSummary): string {
  if (src.codec === 'dsd' && src.dsdRate) return `DSD${Math.round(src.dsdRate / 44100)}`
  if (LOSSLESS.has(src.codec)) return `${codecLabel(src)} ${src.bitDepth ?? 16}/${khz(src.sampleRate)}`
  return src.bitrateKbps ? `${codecLabel(src)} ${src.bitrateKbps}k` : `${codecLabel(src)} ${khz(src.sampleRate)}`
}

export function bitPerfectLabel(sp: Pick<SignalPath, 'bitPerfect' | 'bitPerfectScope'>): string | null {
  if (!sp.bitPerfect) return null
  return sp.bitPerfectScope === 'os' ? 'Bit-perfect to OS' : 'Bit-perfect'
}

export function bluetoothLabel(bt: BluetoothInfo): string {
  if (!bt.codec) return 'Codec not reported by the OS'
  const name = bt.codec.toUpperCase()
  if (bt.bitrateKbps) return `${name} · ${bt.bitrateKbps} kbps`
  if (bt.quality) return `${name} · ${bt.quality}`
  return name
}
```

`app/src/renderer/stores/player.ts`:
```ts
import { create } from 'zustand'
import type { DeviceInfo } from '@protocol/DeviceInfo'
import type { EngineErrorEvent } from '@protocol/EngineErrorEvent'
import type { OutputMode } from '@protocol/OutputMode'
import type { PlayerState } from '@protocol/PlayerState'
import type { QueueChanged } from '@protocol/QueueChanged'
import type { SignalPath } from '@protocol/SignalPath'
import type { AuroraApi, EngineStatus } from '@shared/api'

export interface PlayerStore {
  api: AuroraApi | null
  state: PlayerState
  stamp: number
  signal: SignalPath | null
  queue: QueueChanged
  devices: DeviceInfo[]
  engine: EngineStatus
  error: EngineErrorEvent | null
  panel: 'queue' | 'signal'
  connect(api: AuroraApi): () => void
  openFiles(): Promise<void>
  playPaths(paths: string[]): Promise<void>
  playItem(id: number): Promise<void>
  toggle(): Promise<void>
  next(): Promise<void>
  prev(): Promise<void>
  seek(ms: number): Promise<void>
  setVolume(v: number): Promise<void>
  setMode(m: OutputMode): Promise<void>
  setDevice(id: string | null): Promise<void>
  setPanel(p: 'queue' | 'signal'): void
  dismissError(): void
}

const initialState: PlayerState = { status: 'stopped', queueIndex: null, track: null, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null }
const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now())

export function livePosition(s: Pick<PlayerStore, 'state' | 'stamp'>, at: number): number {
  const base = s.state.positionMs
  if (s.state.status !== 'playing') return base
  const p = base + Math.max(0, at - s.stamp)
  return s.state.durationMs != null ? Math.min(p, s.state.durationMs) : p
}

export const usePlayer = create<PlayerStore>()((set, get) => {
  const call: AuroraApi['call'] = (method, params) => {
    const api = get().api
    return api ? api.call(method, params) : Promise.reject(new Error('not connected'))
  }
  const safe = async (p: Promise<unknown>) => { try { await p } catch (e) { console.warn(e) } }

  return {
    api: null,
    state: initialState,
    stamp: 0,
    signal: null,
    queue: { items: [], current: null },
    devices: [],
    engine: { state: 'starting' },
    error: null,
    panel: 'queue',

    connect(api) {
      set({ api })
      const refresh = async () => {
        try {
          const [state, queue, devices] = await Promise.all([api.call('player.state', {}), api.call('queue.get', {}), api.call('devices.list', {})])
          set({ state, stamp: now(), queue, devices })
        } catch { /* the engine isn't ready yet; engine.status will trigger another refresh */ }
      }
      const offs = [
        api.on('player.state', (state) => set({ state, stamp: now() })),
        api.on('player.signalPath', (signal) => set({ signal })),
        api.on('queue.changed', (queue) => set({ queue })),
        api.on('devices.changed', (devices) => set({ devices })),
        api.on('engine.error', (error) => set({ error })),
        api.on('engine.status', (engine) => { set({ engine }); if (engine.state === 'ready') void refresh() }),
      ]
      void api.status().then((engine) => { set({ engine }); if (engine.state === 'ready') void refresh() }).catch(() => {})
      return () => offs.forEach((off) => off())
    },

    async openFiles() {
      const api = get().api
      if (!api) return
      const paths = await api.openFiles()
      if (paths.length) await get().playPaths(paths)
    },
    async playPaths(paths) {
      await safe(call('queue.setPaths', { paths, startIndex: 0 }))
      await safe(call('player.play', {}))
    },
    playItem: (id) => safe(call('player.play', { queueItemId: id })),
    toggle: () => safe(call('player.toggle', {})),
    next: () => safe(call('player.next', {})),
    prev: () => safe(call('player.prev', {})),
    async seek(ms) {
      set((s) => ({ state: { ...s.state, positionMs: ms }, stamp: now() }))
      await safe(call('player.seek', { ms: Math.round(ms) }))
    },
    async setVolume(volume) {
      set((s) => ({ state: { ...s.state, volume } }))
      await safe(call('player.setVolume', { volume }))
    },
    setMode: (mode) => safe(call('player.setMode', { mode })),
    setDevice: (id) => safe(call('player.setDevice', { id })),
    setPanel: (panel) => set({ panel }),
    dismissError: () => set({ error: null }),
  }
})
```

- [ ] **Step 4: Implement the styles**

`styles/tokens.css`:
```css
:root {
  --bg: #17151a;
  --surface: #100e12;
  --raised: #1b181f;
  --raised-2: #231f28;
  --border: #2a2530;
  --text: #efe9e1;
  --text-muted: rgb(239 233 225 / 0.55);
  --text-faint: rgb(239 233 225 / 0.35);
  --accent: #ffb547;
  --accent-2: #ff7a3d;
  --ok: #6fd39a;
  --danger: #ff6b6b;
  --radius-sm: 4px;
  --radius: 6px;
  --rail: 64px;
  --bar: 74px;
  --titlebar: 36px;
  --font-ui: 'Inter', system-ui, sans-serif;
  --font-mono: 'JetBrains Mono', ui-monospace, monospace;
  --ease: cubic-bezier(0.2, 0.7, 0.2, 1);
  --dur: 180ms;
  color-scheme: dark;
}
```

`styles/global.css`:
```css
* { box-sizing: border-box; }
html, body, #root { height: 100%; margin: 0; }
body { background: var(--bg); color: var(--text); font: 13px/1.45 var(--font-ui); -webkit-font-smoothing: antialiased; user-select: none; overflow: hidden; }
button { font: inherit; color: inherit; background: none; border: 0; cursor: pointer; }
button:disabled { cursor: default; opacity: 0.35; }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: var(--radius-sm); }
.mono { font-family: var(--font-mono); font-feature-settings: 'zero'; letter-spacing: 0; }
.muted { color: var(--text-muted); }
.drag { -webkit-app-region: drag; }
.no-drag { -webkit-app-region: no-drag; }
::-webkit-scrollbar { width: 10px; height: 10px; }
::-webkit-scrollbar-thumb { background: var(--raised-2); border-radius: 5px; border: 2px solid var(--bg); }
@media (prefers-reduced-motion: reduce) { *, *::before, *::after { transition: none !important; animation: none !important; } }
```

- [ ] **Step 5: Implement the components**

`components/FormatBadges.tsx` + `FormatBadges.module.css`:
```tsx
import type { SignalPath } from '@protocol/SignalPath'
import { bitPerfectLabel, bluetoothLabel, formatBadge } from '../lib/format'
import s from './FormatBadges.module.css'

export function FormatBadges({ signal }: { signal: SignalPath | null }) {
  if (!signal) return null
  const bp = bitPerfectLabel(signal)
  return (
    <div className={s.row}>
      {bp && <span className={`${s.badge} ${s.ok}`} data-testid="bitperfect-badge">{bp}</span>}
      <span className={s.badge} data-testid="format-badge">{formatBadge(signal.source)}</span>
      {signal.bluetooth?.codec && <span className={s.badge}>{bluetoothLabel(signal.bluetooth)}</span>}
    </div>
  )
}
```
```css
.row { display: flex; gap: 6px; align-items: center; }
.badge { font: 700 9.5px/1 var(--font-mono); letter-spacing: 0.06em; text-transform: uppercase; padding: 4px 6px; border-radius: var(--radius-sm); border: 1px solid var(--accent); color: var(--accent); white-space: nowrap; }
.ok { border-color: var(--ok); color: var(--ok); }
```

`components/SignalPathPanel.tsx` + `.module.css`:
```tsx
import type { SignalPath } from '@protocol/SignalPath'
import { bitPerfectLabel, bluetoothLabel, codecLabel, khz } from '../lib/format'
import s from './SignalPathPanel.module.css'

const STAGE_TITLE: Record<string, string> = { dsd: 'DSD', channels: 'Channels', resampler: 'Resampler', volume: 'Volume', dither: 'Dither' }

function Node({ title, active = true, children, testId }: { title: string; active?: boolean; children: React.ReactNode; testId?: string }) {
  return (
    <div className={s.node} data-active={String(active)} data-testid={testId}>
      <h5>{title}</h5>
      {children}
    </div>
  )
}

export function SignalPathPanel({ signal }: { signal: SignalPath | null }) {
  if (!signal) return <section className={s.panel}><p className="muted">Play something to see its signal path.</p></section>
  const { source: src, output: out } = signal
  const bp = bitPerfectLabel(signal)
  return (
    <section className={s.panel} aria-label="Signal path details">
      <header className={s.head}>
        <h2>Signal path</h2>
        <span className={`${s.badge} ${bp ? s.ok : ''}`}>{bp ?? 'Processed'}</span>
        {signal.underruns > 0 && <span className={`${s.badge} ${s.warn}`}>{signal.underruns} underruns</span>}
      </header>
      <div className={`${s.chain} mono`}>
        <Node title="SOURCE">
          <b>{`${src.container.toUpperCase()} · ${codecLabel(src)}`}</b>
          <span>{src.dsdRate ? `${(src.dsdRate / 1e6).toFixed(4)} MHz · 1-bit` : `${khz(src.sampleRate)} kHz${src.bitDepth ? ` · ${src.bitDepth}-bit` : ''}`}</span>
          <span>{src.channels} ch{src.bitrateKbps ? ` · ${src.bitrateKbps} kbps` : ''}</span>
        </Node>
        <span className={s.arrow} aria-hidden>→</span>
        <Node title="DECODER">
          <b>{signal.decoder.name}</b>
          <span>{signal.decoder.gapless ? `gapless −${signal.decoder.gapless.delay} / −${signal.decoder.gapless.padding}` : 'gapless: n/a'}</span>
        </Node>
        {signal.stages.map((st) => (
          <span key={st.kind} className={s.pair}>
            <span className={s.arrow} aria-hidden>→</span>
            <Node title={STAGE_TITLE[st.kind] ?? st.kind.toUpperCase()} active={st.active}><span>{st.detail}</span></Node>
          </span>
        ))}
        <span className={s.arrow} aria-hidden>→</span>
        <Node title="OUTPUT" testId="signal-output">
          <b>{out.name}</b>
          <span>{out.mode} · {khz(out.sampleRate)} kHz</span>
          <span>{out.format} · {out.transport}</span>
        </Node>
        {signal.bluetooth && (
          <>
            <span className={s.arrow} aria-hidden>→</span>
            <Node title="BLUETOOTH">
              <b className={signal.bluetooth.codec ? s.accent : undefined}>{bluetoothLabel(signal.bluetooth)}</b>
              {signal.bluetooth.codec && <span>negotiated by the OS</span>}
            </Node>
          </>
        )}
      </div>
      {signal.notes.length > 0 && <ul className={s.notes}>{signal.notes.map((n) => <li key={n}>{n}</li>)}</ul>}
    </section>
  )
}
```
```css
.panel { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); padding: 18px 20px; }
.head { display: flex; align-items: center; gap: 10px; margin-bottom: 14px; }
.head h2 { font-size: 14px; font-weight: 700; margin: 0; }
.badge { font: 700 9.5px/1 var(--font-mono); letter-spacing: 0.06em; text-transform: uppercase; padding: 4px 6px; border-radius: var(--radius-sm); border: 1px solid var(--accent); color: var(--accent); }
.ok { border-color: var(--ok); color: var(--ok); }
.warn { border-color: var(--danger); color: var(--danger); }
.chain { display: flex; align-items: stretch; gap: 0; overflow-x: auto; padding-bottom: 6px; font-size: 11px; }
.pair { display: contents; }
.node { flex: 1 0 128px; display: flex; flex-direction: column; gap: 3px; background: var(--raised); border: 1px solid var(--border); border-radius: var(--radius); padding: 10px; transition: opacity var(--dur) var(--ease); }
.node[data-active='false'] { opacity: 0.4; border-style: dashed; }
.node h5 { margin: 0 0 4px; font-size: 9.5px; letter-spacing: 0.1em; color: var(--text-muted); font-weight: 600; }
.node b { font-size: 12.5px; font-weight: 700; color: var(--text); }
.node span { color: var(--text-muted); }
.accent { color: var(--accent) !important; }
.arrow { display: grid; place-items: center; width: 22px; flex: 0 0 22px; color: var(--accent); }
.notes { margin: 12px 0 0; padding-left: 16px; color: var(--text-muted); font-size: 12px; }
```

`components/SeekBar.tsx` + `.module.css`:
```tsx
import { useEffect, useState } from 'react'
import { formatTime } from '../lib/format'
import { livePosition, usePlayer } from '../stores/player'
import s from './SeekBar.module.css'

export function SeekBar() {
  const state = usePlayer((p) => p.state)
  const stamp = usePlayer((p) => p.stamp)
  const seek = usePlayer((p) => p.seek)
  const [, tick] = useState(0)
  const [drag, setDrag] = useState<number | null>(null)
  useEffect(() => {
    if (state.status !== 'playing') return
    let raf = 0
    const loop = () => { tick((n) => n + 1); raf = requestAnimationFrame(loop) }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [state.status])
  const duration = state.durationMs ?? 0
  const pos = drag ?? livePosition({ state, stamp }, performance.now())
  const pct = duration > 0 ? Math.min(100, (pos / duration) * 100) : 0
  return (
    <div className={`${s.row} mono`}>
      <span className={s.time}>{formatTime(pos)}</span>
      <input
        className={s.range} type="range" min={0} max={duration || 1} step={100} value={Math.min(pos, duration || 1)}
        aria-label="Seek" disabled={!duration} style={{ ['--pct' as string]: `${pct}%` }}
        onChange={(e) => setDrag(Number(e.target.value))}
        onPointerUp={() => { if (drag != null) { void seek(drag); setDrag(null) } }}
        onKeyUp={() => { if (drag != null) { void seek(drag); setDrag(null) } }}
      />
      <span className={s.time}>{formatTime(duration)}</span>
    </div>
  )
}
```
```css
.row { display: flex; align-items: center; gap: 10px; width: 100%; font-size: 10.5px; }
.time { color: var(--text-muted); min-width: 40px; text-align: center; }
.range { flex: 1; appearance: none; height: 14px; background: transparent; cursor: pointer; }
.range::-webkit-slider-runnable-track { height: 3px; background: linear-gradient(to right, var(--accent) var(--pct), #2e2934 var(--pct)); border-radius: 0; }
.range::-webkit-slider-thumb { appearance: none; width: 10px; height: 10px; margin-top: -3.5px; background: var(--text); border-radius: 2px; opacity: 0; transition: opacity var(--dur) var(--ease); }
.range:hover::-webkit-slider-thumb, .range:focus-visible::-webkit-slider-thumb { opacity: 1; }
```

`components/Transport.tsx` + `.module.css`:
```tsx
import { Pause, Play, Repeat, Shuffle, SkipBack, SkipForward } from 'lucide-react'
import { usePlayer } from '../stores/player'
import s from './Transport.module.css'

// Actions are read from the store at click time so tests can swap them with setState.
const actions = () => usePlayer.getState()

export function Transport() {
  const playing = usePlayer((p) => p.state.status === 'playing')
  return (
    <div className={s.row}>
      <button aria-label="Shuffle" title="Shuffle arrives with the library (M2)" disabled><Shuffle size={16} /></button>
      <button aria-label="Previous" onClick={() => void actions().prev()}><SkipBack size={18} /></button>
      <button className={s.play} aria-label={playing ? 'Pause' : 'Play'} onClick={() => void actions().toggle()}>
        {playing ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}
      </button>
      <button aria-label="Next" onClick={() => void actions().next()}><SkipForward size={18} /></button>
      <button aria-label="Repeat" title="Repeat arrives with the queue (M2)" disabled><Repeat size={16} /></button>
    </div>
  )
}
```
```css
.row { display: flex; align-items: center; justify-content: center; gap: 18px; color: var(--text-muted); }
.row button:not(.play):hover:not(:disabled) { color: var(--text); }
.play { width: 34px; height: 34px; border-radius: var(--radius); background: var(--accent); color: #17151a !important; display: grid; place-items: center; transition: transform var(--dur) var(--ease), background var(--dur) var(--ease); }
.play:hover { background: #ffc46b; transform: scale(1.04); }
```

`components/DevicePicker.tsx`:
```tsx
import { Bluetooth, Headphones, Monitor, Speaker, Usb } from 'lucide-react'
import { usePlayer } from '../stores/player'

const ICON = { bluetooth: Bluetooth, usb: Usb, hdmi: Monitor, builtin: Speaker, other: Headphones } as const

export function DevicePicker() {
  const devices = usePlayer((p) => p.devices)
  const deviceId = usePlayer((p) => p.state.deviceId)
  const current = devices.find((d) => (deviceId ? d.id === deviceId : d.isDefault))
  const Icon = ICON[current?.transport ?? 'other']
  return (
    <label style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-muted)' }} title={current?.name ?? 'Output device'}>
      <Icon size={16} aria-hidden />
      <select
        aria-label="Output device" value={deviceId ?? ''} onChange={(e) => void usePlayer.getState().setDevice(e.target.value || null)}
        style={{ maxWidth: 150, background: 'var(--raised)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 4, padding: '3px 4px', font: '11px var(--font-ui)' }}
      >
        <option value="">System default</option>
        {devices.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
      </select>
    </label>
  )
}
```

`components/PlayerBar.tsx` + `.module.css`:
```tsx
import { Disc3, ListMusic, Volume2, VolumeX } from 'lucide-react'
import { usePlayer } from '../stores/player'
import { DevicePicker } from './DevicePicker'
import { FormatBadges } from './FormatBadges'
import { SeekBar } from './SeekBar'
import { Transport } from './Transport'
import s from './PlayerBar.module.css'

export function PlayerBar() {
  const state = usePlayer((p) => p.state)
  const signal = usePlayer((p) => p.signal)
  const bitPerfect = state.mode === 'bitPerfect'
  const { setVolume, setMode, setPanel } = usePlayer.getState()
  return (
    <footer className={s.bar}>
      <div className={s.now}>
        <div className={s.art} aria-hidden><Disc3 size={22} /></div>
        <div className={s.meta}>
          <div className={s.title} data-testid="now-title">{state.track?.title ?? 'Nothing playing'}</div>
          <div className="muted">{state.track ? state.track.path.split(/[\\/]/).slice(-2, -1)[0] : 'Open files to start'}</div>
        </div>
      </div>
      <div className={s.center}>
        <Transport />
        <SeekBar />
      </div>
      <div className={s.right}>
        <FormatBadges signal={signal} />
        <button className={s.icon} aria-label="Queue" onClick={() => setPanel('queue')}><ListMusic size={16} /></button>
        <DevicePicker />
        <button
          role="switch" aria-checked={bitPerfect} aria-label="Bit-perfect" className={s.switch}
          onClick={() => void setMode(bitPerfect ? 'processed' : 'bitPerfect')}
          title={bitPerfect ? 'Bit-perfect: exclusive, untouched output' : 'Processed: shared output with volume'}
        >
          <span className="mono">BP</span>
        </button>
        <span className={s.vol}>
          {state.volume === 0 ? <VolumeX size={16} aria-hidden /> : <Volume2 size={16} aria-hidden />}
          <input
            type="range" min={0} max={1} step={0.01} aria-label="Volume" value={bitPerfect ? 1 : state.volume}
            disabled={bitPerfect} title={bitPerfect ? 'Bit-perfect: use your system or DAC volume' : `${Math.round(state.volume * 100)}%`}
            onChange={(e) => void setVolume(Number(e.target.value))}
          />
        </span>
      </div>
    </footer>
  )
}
```
```css
.bar { display: grid; grid-template-columns: minmax(220px, 1fr) minmax(320px, 1.4fr) minmax(320px, 1fr); align-items: center; gap: 16px; padding: 0 16px; background: var(--surface); border-top: 1px solid var(--border); height: var(--bar); }
.now { display: flex; align-items: center; gap: 12px; min-width: 0; }
.art { width: 46px; height: 46px; border-radius: var(--radius-sm); display: grid; place-items: center; color: #17151a; background: linear-gradient(135deg, var(--accent), var(--accent-2)); flex: none; }
.meta { min-width: 0; }
.title { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.center { display: flex; flex-direction: column; align-items: center; gap: 6px; }
.right { display: flex; align-items: center; justify-content: flex-end; gap: 12px; }
.icon { color: var(--text-muted); }
.icon:hover { color: var(--text); }
.switch { font-size: 9.5px; font-weight: 700; padding: 4px 6px; border-radius: var(--radius-sm); border: 1px solid var(--border); color: var(--text-muted); }
.switch[aria-checked='true'] { border-color: var(--ok); color: var(--ok); }
.vol { display: flex; align-items: center; gap: 6px; color: var(--text-muted); }
.vol input { width: 90px; accent-color: var(--accent); }
```

`components/IconRail.tsx`:
```tsx
import { Activity, House, Library, Search, Settings } from 'lucide-react'
import { usePlayer } from '../stores/player'

export function IconRail() {
  const panel = usePlayer((p) => p.panel)
  const btn = (active: boolean): React.CSSProperties => ({ width: 40, height: 40, display: 'grid', placeItems: 'center', borderRadius: 6, color: active ? 'var(--accent)' : 'var(--text-muted)', background: active ? 'var(--raised)' : 'transparent' })
  return (
    <nav aria-label="Main" style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10, paddingTop: 14, background: 'var(--surface)', borderRight: '1px solid var(--border)' }}>
      <button aria-label="Home" style={btn(panel === 'queue')} onClick={() => usePlayer.getState().setPanel('queue')}><House size={18} /></button>
      <button aria-label="Library" title="Library arrives in M2" disabled style={btn(false)}><Library size={18} /></button>
      <button aria-label="Search" title="Search arrives in M2" disabled style={btn(false)}><Search size={18} /></button>
      <button aria-label="Signal path" style={btn(panel === 'signal')} onClick={() => usePlayer.getState().setPanel('signal')}><Activity size={18} /></button>
      <span style={{ flex: 1 }} />
      <button aria-label="Settings" title="Settings arrive in M2" disabled style={{ ...btn(false), marginBottom: 14 }}><Settings size={18} /></button>
    </nav>
  )
}
```

`components/Home.tsx` + `.module.css`:
```tsx
import { FolderOpen } from 'lucide-react'
import { usePlayer } from '../stores/player'
import s from './Home.module.css'

export function Home() {
  const queue = usePlayer((p) => p.queue)
  const current = usePlayer((p) => p.state.queueIndex)
  const { openFiles, playItem, playPaths, api } = usePlayer.getState()
  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const paths = api?.pathsForFiles([...e.dataTransfer.files]) ?? []
    if (paths.length) void playPaths(paths)
  }
  return (
    <div className={s.home} onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <header className={s.head}>
        <div>
          <div className="mono muted">QUEUE · {queue.items.length} {queue.items.length === 1 ? 'TRACK' : 'TRACKS'}</div>
          <h1>{queue.items.length ? 'Now playing' : 'Good to hear you'}</h1>
        </div>
        <button className={s.open} onClick={() => void openFiles()}><FolderOpen size={16} /> Open files</button>
      </header>
      {queue.items.length === 0 ? (
        <div className={s.empty}>
          <p>Drop audio files here, or open them to start listening.</p>
          <p className="mono muted">FLAC · ALAC · WAV · AIFF · MP3 · AAC · HE-AAC · Vorbis · Opus · DSF · DFF</p>
        </div>
      ) : (
        <ol className={s.list}>
          {queue.items.map((it, i) => (
            <li key={it.id} className={i === current ? s.current : undefined}>
              <button onDoubleClick={() => void playItem(it.id)} onKeyDown={(e) => { if (e.key === 'Enter') void playItem(it.id) }}>
                <span className="mono muted">{String(i + 1).padStart(2, '0')}</span>
                <span className={s.name}>{it.title}</span>
                <span className="mono muted">{it.path.split(/[\\/]/).slice(-2, -1)[0]}</span>
              </button>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
```
```css
.home { min-height: 100%; }
.head { display: flex; align-items: flex-end; justify-content: space-between; margin-bottom: 18px; }
.head h1 { font-size: 26px; font-weight: 800; margin: 4px 0 0; letter-spacing: -0.01em; }
.head :global(.mono) { font-size: 10px; letter-spacing: 0.1em; }
.open { display: inline-flex; align-items: center; gap: 8px; padding: 8px 14px; border: 1px solid var(--accent); color: var(--accent); border-radius: var(--radius); font-weight: 600; transition: background var(--dur) var(--ease); }
.open:hover { background: rgb(255 181 71 / 0.1); }
.empty { border: 1px dashed var(--border); border-radius: var(--radius); padding: 48px; text-align: center; }
.empty .mono { font-size: 10px; letter-spacing: 0.08em; }
.list { list-style: none; margin: 0; padding: 0; }
.list button { display: grid; grid-template-columns: 36px 1fr auto; gap: 12px; width: 100%; text-align: left; padding: 8px 6px; border-bottom: 1px solid #26222a; }
.list button:hover { background: var(--raised); }
.current button, .current .name { color: var(--accent); }
.name { font-weight: 500; }
```

`components/EngineBanner.tsx`:
```tsx
import { usePlayer } from '../stores/player'

export function EngineBanner() {
  const engine = usePlayer((p) => p.engine)
  const error = usePlayer((p) => p.error)
  const api = usePlayer((p) => p.api)
  const bar: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 12, padding: '8px 14px', margin: '0 0 16px', borderRadius: 6, border: '1px solid var(--danger)', color: 'var(--text)', background: 'rgb(255 107 107 / 0.08)' }
  if (engine.state === 'failed' || engine.state === 'mismatch') {
    return (
      <div role="alert" style={bar}>
        <span style={{ flex: 1 }}>{engine.message}</span>
        {engine.state === 'failed' && <button onClick={() => void api?.restartEngine()} style={{ color: 'var(--accent)' }}>Restart engine</button>}
        <button onClick={() => void api?.openLogs()} style={{ color: 'var(--text-muted)' }}>Open logs</button>
      </div>
    )
  }
  if (error) {
    return (
      <div role="status" style={{ ...bar, borderColor: 'var(--border)', background: 'var(--raised)' }}>
        <span className="mono" style={{ color: 'var(--accent)', fontSize: 10 }}>{error.name}</span>
        <span style={{ flex: 1 }}>{error.message}{error.path ? ` · ${error.path.split(/[\\/]/).pop()}` : ''}</span>
        <button onClick={() => usePlayer.getState().dismissError()} aria-label="Dismiss" style={{ color: 'var(--text-muted)' }}>✕</button>
      </div>
    )
  }
  return null
}
```

`App.tsx` + `App.module.css`:
```tsx
import { useEffect } from 'react'
import type { AuroraApi } from '@shared/api'
import { EngineBanner } from './components/EngineBanner'
import { Home } from './components/Home'
import { IconRail } from './components/IconRail'
import { PlayerBar } from './components/PlayerBar'
import { SignalPathPanel } from './components/SignalPathPanel'
import { usePlayer } from './stores/player'
import s from './App.module.css'

export function App({ api }: { api: AuroraApi }) {
  const panel = usePlayer((p) => p.panel)
  const signal = usePlayer((p) => p.signal)
  useEffect(() => usePlayer.getState().connect(api), [api])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === 'INPUT' || (e.target as HTMLElement)?.tagName === 'SELECT') return
      const p = usePlayer.getState()
      if (e.code === 'Space') { e.preventDefault(); void p.toggle() }
      else if (e.key === 'ArrowRight' && e.shiftKey) void p.next()
      else if (e.key === 'ArrowLeft' && e.shiftKey) void p.prev()
      else if (e.key === 'ArrowRight') void p.seek(p.state.positionMs + 5000)
      else if (e.key === 'ArrowLeft') void p.seek(Math.max(0, p.state.positionMs - 5000))
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  return (
    <div className={s.app} data-platform={api.platform}>
      <div className={`${s.titlebar} drag`}><span className="mono">aurora</span></div>
      <IconRail />
      <main className={s.main}>
        <EngineBanner />
        {panel === 'signal' ? <SignalPathPanel signal={signal} /> : <Home />}
      </main>
      <PlayerBar />
    </div>
  )
}
```
```css
.app { display: grid; grid-template-columns: var(--rail) 1fr; grid-template-rows: var(--titlebar) 1fr var(--bar); height: 100vh; }
.titlebar { grid-column: 1 / -1; display: flex; align-items: center; padding: 0 16px; background: var(--surface); border-bottom: 1px solid var(--border); color: var(--text-faint); font-size: 11px; letter-spacing: 0.12em; }
.app[data-platform='darwin'] .titlebar { padding-left: 84px; }
.main { overflow: auto; padding: 24px 28px; }
.app > footer { grid-column: 1 / -1; }
```

`main.tsx`:
```tsx
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import '@fontsource/inter/700.css'
import '@fontsource/inter/800.css'
import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/700.css'
import './styles/tokens.css'
import './styles/global.css'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App'

createRoot(document.getElementById('root')!).render(<StrictMode><App api={window.aurora} /></StrictMode>)
```

`index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data:" />
    <title>Aurora</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="./main.tsx"></script>
  </body>
</html>
```
If `electron-vite dev` reports CSP violations from the React-refresh preamble, add `'unsafe-inline'` to `script-src` **in dev only**: generate the meta tag from `import.meta.env.DEV` via a Vite `transformIndexHtml` hook. Production keeps `script-src 'self'`.

- [ ] **Step 6: Run the tests and typecheck**

Run: `npm --prefix app test` and `npm --prefix app run typecheck`
Expected: all PASS, no type errors.

- [ ] **Step 7: See it running**

Run: `cargo build --manifest-path engine/Cargo.toml -p aurora-engine` then `npm --prefix app run dev`.
Expected: the Studio window opens (charcoal, amber accent, icon rail, bottom player bar). Then:
1. Click **Open files** and pick `fixtures/out/flac24_96k.flac`. It plays, the title shows `flac24_96k` and the badge shows `FLAC 24/96`.
2. Click the **Signal path** rail icon to see the chain.
3. Toggle **BP** (bit-perfect). The volume slider greys out, and on a capable device the badge reads "Bit-perfect".

Compare the window against `.superpowers/brainstorm/*/content/studio-screens.html` for palette and typography.

- [ ] **Step 8: Commit**

```bash
git add app/src/renderer app/test
git commit -m "feat(app): Studio design system, player store, player bar and signal-path panel"
```

---

### Task 23: End-to-end smoke test and root scripts

**Files:**
- Create: `app/playwright.config.ts`, `app/e2e/smoke.spec.ts`, `package.json` (repo root)
- Modify: `README.md` (create) — dev quick start

**Interfaces:**
- Consumes: everything above. The e2e test drives the real app against the real engine with the `null` output (`AURORA_OUTPUT=null`), bypassing the file dialog with `AURORA_TEST_OPEN_FILES`.

- [ ] **Step 1: Write the smoke test**

`app/playwright.config.ts`:
```ts
import { defineConfig } from '@playwright/test'
export default defineConfig({ testDir: 'e2e', timeout: 90_000, retries: process.env.CI ? 1 : 0, reporter: [['list']] })
```

`app/e2e/smoke.spec.ts`:
```ts
import { _electron as electron, expect, test } from '@playwright/test'
import { resolve } from 'node:path'

const root = resolve(__dirname, '../..')
const exe = process.platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'

test('open → play → signal path → bit-perfect', async () => {
  const app = await electron.launch({
    args: [resolve(__dirname, '../out/main/index.js')],
    env: {
      ...process.env,
      AURORA_ENGINE_PATH: resolve(root, 'engine/target/debug', exe),
      AURORA_OUTPUT: 'null',
      AURORA_NULL_SPEED: '0.5',
      AURORA_TEST_OPEN_FILES: resolve(root, 'fixtures/out/flac24_96k.flac'),
    },
  })
  const win = await app.firstWindow()
  await win.getByRole('button', { name: /open files/i }).click()
  await expect(win.getByTestId('now-title')).toHaveText('flac24_96k')
  await expect(win.getByTestId('format-badge')).toHaveText('FLAC 24/96')
  await expect(win.getByRole('button', { name: 'Pause' })).toBeVisible()

  await win.getByRole('button', { name: 'Signal path' }).click()
  await expect(win.getByTestId('signal-output')).toContainText('Null Output')

  await win.getByRole('button', { name: 'Pause' }).click()
  await expect(win.getByRole('button', { name: 'Play' })).toBeVisible()

  await win.getByRole('switch', { name: 'Bit-perfect' }).click()
  await expect(win.getByRole('slider', { name: 'Volume' })).toBeDisabled()
  await win.getByRole('button', { name: 'Play' }).click()
  await expect(win.getByTestId('bitperfect-badge')).toHaveText('Bit-perfect')
  await app.close()
})
```

- [ ] **Step 2: Root scripts**

`package.json` (repo root):
```json
{
  "private": true,
  "scripts": {
    "fixtures": "bash fixtures/generate.sh",
    "engine:build": "cargo build --manifest-path engine/Cargo.toml -p aurora-engine",
    "engine:test": "cargo test --manifest-path engine/Cargo.toml --workspace",
    "dev": "npm run engine:build && npm --prefix app run dev",
    "test": "npm run engine:test && npm --prefix app test",
    "e2e": "npm run engine:build && npm --prefix app run e2e"
  }
}
```

`README.md`: project one-liner, then the prerequisites (Rust stable, CMake, Node 24, ffmpeg for fixtures), then the commands `npm run fixtures`, `npm run dev`, `npm test` and `npm run e2e`, then the "Bluetooth codec display" honesty note from spec §3.7, then the license with a pointer to `THIRD_PARTY_NOTICES.md`.

- [ ] **Step 3: Run it**

Run: `npx --prefix app playwright install chromium` (one time; Electron itself doesn't need it, but the test runner may), then `npm run e2e`.
Expected: 1 test PASSES.
If the `bitperfect-badge` assertion times out, check `engine.log` under the app's userData `logs/` folder for the plan notes. The null device advertises exclusive caps, so the 24/96 FLAC must be bit-perfect.

- [ ] **Step 4: Commit**

```bash
git add package.json README.md app/playwright.config.ts app/e2e
git commit -m "test(e2e): Electron smoke test against the real engine with null output"
```

---

### Task 24: CI on Windows, macOS and Linux

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: every test suite above. This is M1's "done" gate (spec §9): golden, gapless and bit-perfect tests pass on all OSes.

- [ ] **Step 1: Write the workflow**

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, windows-2025, macos-15]
    runs-on: ${{ matrix.os }}
    defaults: { run: { shell: bash } }
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: engine }
      - name: System packages (Linux)
        if: runner.os == 'Linux'
        run: sudo apt-get update && sudo apt-get install -y cmake clang pkg-config libasound2-dev libpipewire-0.3-dev libspa-0.2-dev ffmpeg xvfb
      - name: System packages (macOS)
        if: runner.os == 'macOS'
        run: brew install cmake ffmpeg
      - name: System packages (Windows)
        if: runner.os == 'Windows'
        run: choco install -y ffmpeg cmake --installargs 'ADD_CMAKE_TO_PATH=System'
      - uses: actions/cache@v4
        with:
          path: fixtures/out
          key: fixtures-${{ runner.os }}-${{ hashFiles('fixtures/generate.sh', 'engine/crates/fixtures-gen/src/**', 'engine/crates/engine-core/src/decode/dsd/modulator.rs') }}
      - name: Fixtures
        run: '[ -f fixtures/out/dsd64.dff ] || bash fixtures/generate.sh'
      - name: Engine tests
        run: cargo test --manifest-path engine/Cargo.toml --workspace
      - name: Engine tests without fdk-aac
        run: cargo test --manifest-path engine/Cargo.toml -p engine-core --no-default-features --test open
      - name: Clippy
        if: runner.os == 'Linux'
        run: cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings
      - name: Generated protocol types are committed
        run: git diff --exit-code -- shared/protocol
      - uses: actions/setup-node@v5
        with: { node-version: 24, cache: npm, cache-dependency-path: app/package-lock.json }
      - run: npm ci --prefix app
      - run: npm --prefix app run typecheck
      - run: npm --prefix app test
      - name: Build engine for e2e
        run: cargo build --manifest-path engine/Cargo.toml -p aurora-engine
      - name: E2E (Linux)
        if: runner.os == 'Linux'
        run: xvfb-run -a npm --prefix app run e2e
      - name: E2E (Windows/macOS)
        if: runner.os != 'Linux'
        run: npm --prefix app run e2e
```

- [ ] **Step 2: Validate locally, then push**

Run: `npx --yes @action-validator/cli .github/workflows/ci.yml` (or paste the workflow into the GitHub Actions editor).
Expected: no schema errors.
Pushing is outward-facing, so ask the user before creating a GitHub repo or pushing. Once they approve, push the branch and confirm all three jobs go green. If a platform-specific test fails, fix the code; only add `#[cfg]` skips for genuinely hardware-dependent cases.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: test engine and app on Windows, macOS and Linux"
```

---

## Milestone 1 exit checklist (maps to spec §9 M1 "Done when")

- [ ] `cargo test --workspace` passes on all three OSes in CI: golden lossless (bit-exact), lossy SNR, gapless pairs, DSD, HE-AAC, player bit-perfect bytes, RPC.
- [ ] `--no-default-features` build reports `CODEC_NOT_IN_BUILD` for HE-AAC.
- [ ] Manual: on Windows (WASAPI) and macOS (hog mode), `aurora-engine play --bit-perfect fixtures/out/flac24_96k.flac` shows `bitPerfect: true, bitPerfectScope: "device"`.
- [ ] Manual: on Linux with an LDAC headset, the signal path shows `LDAC · 990 kbps` (or the negotiated quality), and on Windows/macOS Bluetooth shows "Codec not reported by the OS".
- [ ] The Electron app opens any supported file, plays it, and shows an accurate signal path. The e2e test is green.
