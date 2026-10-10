# Aurora — Native Music Player: Design Spec

- **Date:** 2026-10-10
- **Status:** Draft for review
- **Working name:** Aurora (placeholder; rename is cheap until Milestone 4)

---

## 1. Intent

### What the user asked for
- A native desktop music player for **Windows, macOS and Linux**.
- **Rust backend**, **Electron frontend**.
- Main selling point: **AAC, LDAC and high-bitrate MP3** support.
- "All the functionality needed from a music player."
- A beautiful UI **inspired by Spotify**, but not similar enough to invite trademark/trade-dress claims.

### Decisions made during brainstorming
| Topic | Decision |
|---|---|
| Meaning of "LDAC support" | LDAC is a Bluetooth transmission codec, not a file format. Aurora delivers **unaltered hi-res audio to the OS** (exclusive/bit-perfect where possible) so LDAC can run at its 990 kbps tier, and **displays the active Bluetooth codec** where the OS exposes it. |
| Audience | **Open-source release** (MIT), CI-built installers for all three OSes. |
| Formats at launch | MP3 (CBR/VBR up to 320 kbps), AAC-LC, **HE-AAC / HE-AACv2**, ALAC, FLAC, WAV, AIFF, Ogg Vorbis, Opus, **DSD (DSF/DFF)**. |
| v1 extras | Audiophile signal-path panel, EQ + crossfade, synced lyrics, album-art-derived colours, spectrum visualizer, mini-player. |
| Phasing | One architecture spec (this doc), then plan + build **milestone by milestone, engine first**. |
| Rust ↔ Electron | **Sidecar process**: JSON-RPC 2.0 over stdio + binary frames for visualizer data. |
| Decoders | **Symphonia** core + **libopus** + **fdk-aac** (optional cargo feature) for HE-AAC + **custom Rust DSD** module. No FFmpeg. |
| Visual direction | **"Studio"**: warm charcoal, amber accent, slim icon rail, monospace technical readouts. |

### Assumptions (correct these if wrong)
- Aurora plays a **local file library**. No streaming services, no cloud sync, no online accounts in v1.
- No network features in v1 (no online lyrics/metadata fetching, no scrobbling). Lyrics come from embedded tags and sidecar `.lrc` files only.
- Dark theme only in v1; light theme is a follow-up.
- English UI only in v1; strings are kept in a single module so i18n can be added later.

### Success criteria
1. Every listed format plays correctly, with **sample-accurate gapless** playback for MP3, AAC and ALAC files that carry encoder delay/padding info.
2. In bit-perfect mode on a wired DAC, the bytes reaching the output device are **identical to the decoded source PCM** (verified by an automated test against a recording null device).
3. On Linux with a Bluetooth LDAC headset, the signal-path panel shows **"LDAC · <quality>"**. On Windows/macOS it truthfully shows that the codec isn't reported.
4. The library handles **100k tracks**: first scan under 2 min on an SSD, paging/search queries under 50 ms.
5. The UI reads as its own product: Studio palette, typography, icon rail and shape language, following the trade-dress guardrails in §5.5. A bottom player bar is a generic music-player convention and is allowed.
6. Installers are produced by CI for Windows (x64, arm64), macOS (x64, arm64) and Linux (x64, arm64).

---

## 2. Architecture overview

```
┌──────────────────────── Electron app ─────────────────────────┐
│  Renderer (React + TypeScript + Vite)                         │
│   UI, stores (Zustand), virtualized lists, visualizer canvas  │
│        ▲  typed IPC via preload/contextBridge (no node in UI) │
│  Main process (TypeScript)                                    │
│   • EngineSupervisor: spawns/restarts sidecar, routes JSON-RPC│
│   • windows (main + mini-player), tray, menus, UI settings    │
└────────────────┬──────────────────────────────────────────────┘
                 │ stdin/stdout: newline-delimited JSON-RPC 2.0
                 │ + length-prefixed binary frames (spectrum)
┌────────────────▼─────────────── Rust sidecar `aurora-engine` ─┐
│  rpc      – protocol, dispatch, event stream                  │
│  player   – state machine, queue cursor, gapless pre-decode   │
│  decode   – Symphonia | libopus | fdk-aac | dsd (ours)        │
│  dsp      – ReplayGain, resampler (rubato), EQ, crossfade,    │
│             spectrum analyzer                                 │
│  output   – cpal (shared) + WASAPI exclusive / CoreAudio hog /│
│             ALSA hw: direct for bit-perfect                   │
│  probe    – device capabilities + Bluetooth codec detection   │
│  library  – scanner (notify), tags (lofty), SQLite+FTS5,      │
│             artwork cache + palette extraction                │
│  osmedia  – SMTC / MPRIS / MPNowPlaying via `souvlaki`        │
└───────────────────────────────────────────────────────────────┘
```

**Responsibility split**
- **Rust owns everything audio and data:** playback, decoding, DSP, output, the library database, metadata, artwork, lyrics parsing and OS media-session integration. OS media integration lives in Rust because Chromium's Media Session only activates when Chromium itself is producing audio. On Windows the main process passes the main window's native handle (HWND) to the engine for SMTC.
- **Electron owns presentation:** windows, menus, tray, UI preferences (a JSON file in the OS app-data dir), and the renderer UI.
- The renderer sees only a typed API exposed by the preload script (`window.aurora.*`). `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`.

**Storage locations:** the OS-standard app data dir (`%APPDATA%/Aurora`, `~/Library/Application Support/Aurora`, `$XDG_DATA_HOME/aurora`):
- `library.db`: SQLite (owned by the engine)
- `artwork/`: resized artwork cache (owned by the engine)
- `session.json`: queue/position snapshot (owned by the engine)
- `settings.json`: UI prefs (owned by Electron)
- `logs/`: engine stderr + main-process logs, rotated

---

## 3. Rust engine

### 3.1 Workspace layout
```
engine/
  Cargo.toml                (workspace)
  crates/
    engine-core/            decode, dsp, output, probe, player
    library/                scanner, tags, db, artwork, lyrics
    protocol/               request/response/event types (serde + ts-rs)
    aurora-engine/          binary: rpc server, wiring, osmedia, CLI
```
Each crate is testable on its own. `aurora-engine` also offers a CLI for development and testing:
`aurora-engine play <file>`, `aurora-engine probe <file>`, `aurora-engine devices`, `aurora-engine rpc` (the default when stdin isn't a TTY).

### 3.2 Threads and data flow
```
RPC thread ──commands──▶ Player (state machine, owns queue cursor)
                            │ opens/closes
Decode thread:  Source → Decoder → DSP chain → ring buffer (rtrb, lock-free SPSC)
                                                       │
Output thread (realtime: no alloc, no locks, no I/O) ──┴─▶ device
                                                       └─▶ tap ring → Analyzer thread (FFT) → spectrum frames @ 30 fps
```
- Commands reach the decode thread via a bounded crossbeam channel. State changes flow back to the RPC thread as events.
- Default decode-ahead buffer is 2 s. It grows by 1 s at a time, up to 8 s, if underruns repeat.

### 3.3 Decoding
| Format | Container demux | Decoder |
|---|---|---|
| MP3 | Symphonia | Symphonia |
| AAC-LC | Symphonia (MP4/M4A, ADTS) | Symphonia |
| HE-AAC, HE-AACv2 | Symphonia (MP4/M4A, ADTS) | **fdk-aac** (`fdk-aac` cargo feature, on by default) |
| ALAC | Symphonia | Symphonia |
| FLAC, WAV, AIFF | Symphonia | Symphonia |
| Ogg Vorbis | Symphonia | Symphonia |
| Opus | Symphonia Ogg demux | **libopus** |
| DSD (DSF, DFF) | **custom** (`engine-core::decode::dsd`) | **custom** |

- AAC profile detection reads the AudioSpecificConfig (object type 5 = SBR, 29 = PS) and routes HE-AAC to fdk-aac. fdk-aac outputs at the full SBR rate.
- If a build lacks fdk-aac, HE-AAC files fail with the readable error `CODEC_NOT_IN_BUILD` ("HE-AAC decoder not included in this build"). A legacy fallback that decodes only the AAC-LC core at half rate is explicitly out of scope.
- **DSD:** the DSF/DFF parsers produce 1-bit streams (DSD64–DSD256). Two output paths:
  - **PCM path (default):** multi-stage FIR decimation to 24-bit PCM at 88.2 kHz (DSD64) or 176.4 kHz (DSD128+), with −6 dB gain compensation.
  - **DoP path:** when the output is in exclusive mode and the device reports support for the matching DoP PCM rate, pack DSD into 24-bit PCM frames with DoP markers. DoP is never used for Bluetooth or shared outputs.

### 3.4 Gapless playback
- About 10 s before the current track ends, the player opens and primes the next track's decoder into the same ring buffer.
- Encoder delay/padding is trimmed from:
  - MP3: LAME/Xing header
  - AAC/ALAC: `iTunSMPB`, plus the MP4 edit list (`elst`) when present
  - Opus: pre-skip
  - Vorbis/FLAC: native
- **Gapless works only when output parameters match.** In processed mode the device format is fixed, so this always holds. In bit-perfect mode, consecutive tracks with a different sample rate or bit depth force a device reopen, which causes an unavoidable short gap. The UI shows "Rate change: brief gap" in the signal path.

### 3.5 Output modes
**Processed (default)**
- Internal format f64.
- Chain: ReplayGain → preamp → 10-band EQ → crossfade mixer → volume.
- Resample with `rubato` (sinc, high quality) only if the device can't run at the source rate. Shared-mode output tries to match the source rate first where the OS allows it.
- Convert to the device sample format with TPDF dither when reducing bit depth.

**Bit-perfect**
- The device switches to the source sample rate and bit depth. Samples pass through as integers with no conversion.
- ReplayGain, EQ, crossfade and software volume are **disabled** (the UI greys them out and points to system/device volume).
- Per OS:
  - Windows: WASAPI exclusive mode (via the `wasapi` crate)
  - macOS: CoreAudio hog mode + nominal sample-rate switch (via `coreaudio-rs`)
  - Linux, wired: ALSA `hw:` direct
  - Linux, Bluetooth: PipeWire with the node rate set to the source rate (bit-perfect to the OS stack)
- **Fallbacks:** if exclusive access is denied (device busy), fall back to shared mode and report `bitPerfect: false, reason: "device busy"` in the signal path.
- **Bluetooth honesty:** for Bluetooth outputs the badge reads **"Bit-perfect to OS"**, never plain "Bit-perfect", because LDAC/AAC/SBC are lossy.

### 3.6 DSP features
- **ReplayGain:** track / album / off. Uses tag values (`REPLAYGAIN_*`, `R128_*`, iTunes `iTunNORM` is ignored). Peak-protected (gain is limited so the peak stays ≤ −0.1 dBFS).
- **EQ:** 10-band graphic EQ (31 Hz–16 kHz, ±12 dB) built from RBJ peaking biquads, plus a preamp and automatic headroom. Built-in presets (Flat, Warm, Bass, Vocal, Bright, Loudness) and user presets. Parametric EQ is out of scope for v1.
- **Crossfade:** 0–12 s (0 = off) with an equal-power curve. **Smart skip:** never crossfade between consecutive tracks of the same album played in order, so gapless albums stay gapless.
- **Spectrum analyzer:** 2048-point FFT with a Hann window, 64 log-spaced bins, at 30 fps, sent as binary frames (64 × u8). Computed only while a visualizer is visible (the UI sends `viz.subscribe` / `viz.unsubscribe`).

### 3.7 Device and Bluetooth probe
- `devices.list()` returns id, name, transport (`usb | bluetooth | builtin | hdmi | other`), supported sample rates/formats, exclusive capability and the default flag.
- **Bluetooth codec detection:**
  - **Linux:** read the active sink node's PipeWire properties (`api.bluez5.codec`, plus LDAC quality where exposed). v1 uses `pw-dump` JSON and refreshes on `devices.changed`. Example result: `{codec: "ldac", quality: "hq", bitrateKbps: 990}`.
  - **Windows / macOS:** detect that the endpoint is Bluetooth (Windows: endpoint `PKEY_Device_EnumeratorName == BTHENUM`/`BTHLEDevice`; macOS: `kAudioDevicePropertyTransportType == Bluetooth`). Codec is `null`, and the UI shows "Codec not reported by the OS". **Never guess.**

### 3.8 Signal-path report
Sent as the `player.signalPath` event whenever any stage changes:
```jsonc
{
  "source":   { "container": "m4a", "codec": "aac-lc", "bitrateKbps": 320, "sampleRate": 44100, "bitDepth": 16, "channels": 2 },
  "decoder":  { "name": "symphonia", "gapless": { "delay": 2112, "padding": 1024 } },
  "stages":   [ { "kind": "replaygain", "active": true, "detail": "-6.4 dB (track)" },
                { "kind": "eq", "active": true, "detail": "Warm" },
                { "kind": "resampler", "active": false, "detail": "rate matched" },
                { "kind": "volume", "active": true, "detail": "82%" } ],
  "output":   { "deviceId": "...", "name": "WH-1000XM5", "transport": "bluetooth", "mode": "shared", "sampleRate": 44100, "format": "f32" },
  "bluetooth":{ "codec": "ldac", "quality": "hq", "bitrateKbps": 990 },   // or null fields
  "bitPerfect": false,
  "bitPerfectScope": null,              // "device" | "os" when true
  "underruns": 0,
  "notes": []                            // e.g. "Rate change: brief gap"
}
```

### 3.9 Library
- **Scanner:** parallel directory walk (`jwalk`). Tags are read with `lofty`. Files whose `(mtime, size)` haven't changed are skipped. After the initial scan, `notify` watches the folders with 2 s debouncing. Missing files are marked `missing` (kept for 30 days so playlists survive a disk being temporarily unmounted), then purged.
- **Metadata:** title, artist(s), album, album artist, track/disc numbers, year, genre(s), composer, duration, codec, bitrate, sample rate, bit depth, channels, ReplayGain values, gapless info, MusicBrainz IDs when present, and an embedded-lyrics flag. Multi-value artist/genre tags are split on the tag's native multi-value fields only; there is no heuristic splitting on "&" or "feat.".
- **Album grouping:** keyed by (album artist or, if absent, artist, plus album title). A disc-number-aware track order.
- **Artwork:** embedded art first, then `cover|folder|front.(jpg|png|webp)` in the track's folder. Each image is resized once to 64/300/1000 px WebP, stored in `artwork/` by content hash, and a **palette** (dominant, vibrant and muted colours) is extracted for dynamic backgrounds.
- **Schema (SQLite, WAL, versioned migrations):**
  `folders`, `tracks`, `albums`, `artists`, `track_artists`, `album_artists`, `genres`, `track_genres`, `artwork`, `playlists`, `playlist_items`, `play_history`, `tracks_fts` (FTS5 over title, artist, album, album artist, composer).
- **Playlists:** user playlists (create, rename, delete, reorder, add/remove), M3U8 import/export. Smart playlists are out of scope for v1. Built-in auto views: Recently Added, Recently Played, Most Played.
- **Lyrics:** embedded `USLT`/`SYLT`/`©lyr`/`LYRICS` tags, or a sidecar `<basename>.lrc` next to the file. Synced (LRC with timestamps, including enhanced word-level timing when present) or plain text. Returned as `{ synced: bool, lines: [{ t_ms?, text }] }`.

### 3.10 OS media integration
`souvlaki` provides Windows SMTC, Linux MPRIS and macOS MPNowPlayingInfoCenter/MPRemoteCommandCenter: hardware media keys, the lock screen and system flyouts. Now-playing metadata and artwork (a file URL from the cache) are pushed on every track change.

---

## 4. Protocol (Electron main ↔ engine)

### 4.1 Transport
- **stdin (main → engine):** newline-delimited JSON-RPC 2.0 requests.
- **stdout (engine → main):** newline-delimited JSON-RPC responses and notifications (events), interleaved with **binary frames**. A frame is the byte `0x1E`, a `u32` little-endian length, then the payload. JSON lines never start with `0x1E`, so the parser tells them apart by the first byte.
- **stderr:** structured logs (`tracing`, JSON format), which main writes to `logs/engine.log`.
- **Handshake:** on startup the engine sends `engine.ready { version, protocolVersion, features: ["fdk-aac", "opus", "dsd", ...] }`. Main refuses to talk to an engine whose `protocolVersion` doesn't match.

### 4.2 Type generation
Types are defined once in the `protocol` crate and exported to TypeScript with `ts-rs` into `shared/protocol/`. A CI check fails if the generated files are stale.

### 4.3 Methods
| Namespace | Methods |
|---|---|
| `player` | `play(queueItemId?)`, `pause`, `toggle`, `stop`, `next`, `prev`, `seek(ms)`, `setVolume(0..1)`, `setMode("processed"\|"bitPerfect")`, `setDevice(id)`, `state()` |
| `queue` | `get`, `set(trackIds, startIndex)`, `add(trackIds)`, `insertNext(trackIds)`, `move(from, to)`, `remove(itemIds)`, `clear`, `setShuffle(bool)`, `setRepeat("off"\|"all"\|"one")` |
| `library` | `folders`, `addFolder(path)`, `removeFolder(id)`, `rescan(folderId?)`, `albums(q)`, `album(id)`, `artists(q)`, `artist(id)`, `genres(q)`, `tracks(q)`, `track(id)`, `search(text, limit)`, `artwork(id, size)` → file path, `recent(kind)` |
| `playlists` | `list`, `get(id)`, `create(name)`, `rename(id, name)`, `delete(id)`, `addTracks(id, trackIds, index?)`, `removeItems(id, itemIds)`, `reorder(id, from, to)`, `importM3u(path)`, `exportM3u(id, path)` |
| `dsp` | `get`, `setEq(bands[10], preamp)`, `setEqEnabled(bool)`, `presets`, `savePreset(name)`, `deletePreset(name)`, `setCrossfade(seconds)`, `setReplayGain("track"\|"album"\|"off")` |
| `lyrics` | `get(trackId)` |
| `devices` | `list` |
| `viz` | `subscribe`, `unsubscribe` |

The list queries (`q`) take `{ filter?, sort, offset, limit }` and return `{ items, total }`.

### 4.4 Events
`engine.ready`, `player.state` (status, current queue item, track summary, positionMs, durationMs; sent at 4 Hz while playing and on every change; the UI interpolates in between), `player.signalPath`, `queue.changed`, `library.scanProgress`, `library.changed`, `devices.changed`, `engine.error`.

### 4.5 Error codes
Standard JSON-RPC codes, plus domain codes in the `-32000…-32099` range: `TRACK_NOT_FOUND`, `FILE_MISSING`, `UNSUPPORTED_FORMAT`, `CODEC_NOT_IN_BUILD`, `DECODE_FAILED`, `DEVICE_UNAVAILABLE`, `EXCLUSIVE_DENIED`, `DB_ERROR`, `INVALID_ARGUMENT`.

---

## 5. Electron app

### 5.1 Tooling
electron-vite (main/preload/renderer builds), electron-builder (packaging), React 19, TypeScript (strict), Zustand (stores), TanStack Virtual (long lists), TanStack Router (views), CSS Modules plus CSS custom properties as design tokens. No component library: all components are custom, to own the look.

### 5.2 Main process
- **EngineSupervisor:**
  - Spawns `aurora-engine` (from `resources/bin/<platform>-<arch>/` when packaged, or the cargo target dir in dev).
  - Correlates request IDs and fans events out to all windows.
  - **Restart policy:** exponential back-off (0.5 s, 1 s, 2 s, …). After 3 crashes within 60 s it stops retrying and shows an error banner with "Restart engine" and "Open logs" actions.
  - On restart the engine restores `session.json` itself (queue, current item, position; paused).
- **Windows:**
  - Main window: frameless with a custom title bar on Windows/Linux, native traffic lights on macOS.
  - Mini-player: 340×110, always-on-top, frameless, remembers its position.
  - Tray icon with play/pause/next/prev and show/hide.
- `settings.json` holds the theme accent, window bounds, sidebar state and last view.

### 5.3 Renderer views
| View | Contents |
|---|---|
| Home | Recently added albums, recently played, most played, quick-access playlists |
| Albums | Virtualized grid; sort by title / artist / year / date added |
| Album detail | Art, metadata header (with format/bit-depth/rate in mono), track table, play / shuffle / add-to-queue |
| Artists / Artist detail | Grid, then albums + top tracks by play count |
| Genres | Genre list → albums |
| Playlists / Playlist detail | Track table with drag-to-reorder |
| Search | Instant FTS search as you type, grouped into tracks / albums / artists |
| Queue | Up next, with drag-to-reorder, remove, clear, and "playing from" context |
| Now Playing (expanded) | Large art, tabs: Lyrics / Queue / Signal Path / Credits; spectrum visualizer; album-palette background |
| Settings | Library folders, audio (device, mode, buffer), DSP (EQ, crossfade, ReplayGain), appearance, about/licenses |

Context menus everywhere: play next, add to queue, add to playlist, go to album/artist, show in folder, and track info (a full technical metadata dialog).

### 5.4 Persistent player bar ("Studio" style)
A 74 px bar spanning the bottom, under the icon rail and content, on the darker `--surface` with a top border. The layout is a common convention; distinctness comes from the Studio tokens, the square amber play button and the monospace readouts:
- **Left:** 46 px art, title, artist, like-to-playlist shortcut.
- **Centre:** shuffle, prev, square amber play button, next, repeat, and the seek bar with monospace timestamps.
- **Right:** format badges (`BIT-PERFECT`, `24/192`, `LDAC`), queue toggle, lyrics toggle, device picker, volume, mini-player button.

### 5.5 Visual design system ("Studio")
- **Colour tokens:** `--bg #17151A`, `--surface #100E12`, `--raised #1B181F`, `--border #2A2530`, `--text #EFE9E1`, `--text-muted` (text at 55% opacity), `--accent #FFB547` (amber), `--accent-2 #FF7A3D` (ember), `--ok #6FD39A`.
- **Dynamic colour:** in Now Playing and album headers, a gradient from the album palette's muted colour into `--bg`. The accent can optionally tint toward the palette's vibrant colour (setting; off by default).
- **Typography:** Inter (UI) and JetBrains Mono (technical readouts: formats, rates, timestamps, the signal path), both OFL and bundled.
- **Shape:** 4–6 px radii, square-ish play button, a slim 64 px icon rail with expandable labels (rail ↔ 220 px sidebar).
- **Icons:** Lucide (ISC license).
- **Motion:** 150–200 ms ease-out transitions; respects `prefers-reduced-motion`.
- **Trade-dress guardrails:** no `#1DB954`-family greens as accent, no Circular/Gotham-like fonts, no circular green play button, no edge-to-edge black `#121212`/`#181818` palette, original iconography and copy.

### 5.6 Keyboard shortcuts
Space play/pause · ←/→ seek ±5 s · Shift+←/→ prev/next · ↑/↓ volume · Ctrl/Cmd+F search · Ctrl/Cmd+L lyrics · Ctrl/Cmd+M mini-player · Ctrl/Cmd+, settings. All shortcuts are listed in Settings.

### 5.7 Accessibility
Full keyboard navigation, visible focus rings (amber), ARIA roles on lists and transport controls, text contrast ≥ 4.5:1 against all surfaces.

---

## 6. Error handling
| Situation | Behaviour |
|---|---|
| Corrupt / unsupported file | Mark the track `unplayable` with the reason in the DB, skip to next, show a non-blocking notice and a ⚠ in track lists. The queue never stalls. |
| File missing at play time | Same as above with `FILE_MISSING`; the scanner reconciles later. |
| Buffer underrun | Insert silence, increment the `underruns` counter in the signal path, grow the buffer on repeats (up to 8 s). |
| Device removed / changed | Pause, switch to the system default device, send `devices.changed`, notify. |
| Exclusive access denied | Fall back to shared mode; the signal path shows `bitPerfect: false` with the reason. |
| Codec feature missing | `CODEC_NOT_IN_BUILD` with a readable message. |
| Engine crash | Supervisor restart policy (§5.2). Engine threads catch panics at their boundaries and log backtraces; a panic in the output thread triggers a clean process exit so the supervisor can restart. |
| DB corruption | On open failure, move `library.db` to `library.db.corrupt-<ts>`, recreate it and rescan; notify the user. |
| Protocol mismatch | Main shows "Engine version mismatch: reinstall Aurora" and doesn't proceed. |

---

## 7. Testing strategy
- **Rust unit tests:** container probing, AAC profile routing, gapless trim maths, DSD decimation filter response, DoP packing, biquad coefficients, crossfade curves, ReplayGain peak protection, the queue/shuffle/repeat state machine, LRC parsing.
- **Golden decode tests:** `fixtures/generate.sh` (committed) uses ffmpeg/sox/fdkaac/opusenc to generate short synthetic signals (sine sweeps, impulses, silence) in every format, so there's no copyrighted audio. These encoders are **test-time tools only** and are never shipped. Generated fixtures are not committed; they're regenerated locally and cached in CI. Lossless formats must decode **bit-exact**; lossy formats are checked by correlation/SNR against the reference signal.
- **Gapless tests:** a sweep is split into consecutive files, encoded, played through the null device, and the boundary is checked for continuity (no inserted or dropped samples).
- **Bit-perfect test:** a recording null output device asserts that the received bytes match the decoded source PCM in bit-perfect mode, including 16/24/32-bit and 44.1–384 kHz cases.
- **Protocol integration tests:** spawn the real `aurora-engine` binary and drive it with JSON-RPC scripts (play, seek, queue edits, library scan of the fixtures). Runs in CI on all three OSes.
- **Library performance:** a benchmark scanning a synthetic 100k-file tag set (tiny files with real tags) and timing queries; asserts the §1 targets.
- **Frontend:** Vitest for stores, the IPC client and formatting helpers. Playwright (Electron) smoke tests: launch → add fixture folder → play → seek → open signal path → mini-player.
- **Manual test matrix (per release):** a wired USB DAC on each OS, a Bluetooth LDAC headset on Linux, AAC Bluetooth on macOS/Windows.

---

## 8. Repository layout
```
engine/                  Cargo workspace (§3.1)
app/                     Electron app
  src/main/              supervisor, windows, tray, settings
  src/preload/           typed window.aurora API
  src/renderer/          React UI (views/, components/, stores/, styles/)
shared/protocol/         generated TS types (ts-rs output, committed)
fixtures/                generate.sh (outputs git-ignored)
docs/superpowers/        specs/ and plans/
.github/workflows/       CI + release (Milestone 4)
LICENSE                  MIT
THIRD_PARTY_NOTICES.md   incl. fdk-aac license, libopus BSD, fonts OFL, Lucide ISC
```

---

## 9. Milestones
Each milestone gets its own implementation plan and ends with something runnable.

**M1 — Audio engine + app shell**
- Rust workspace, every decoder from §3.3, gapless, both output modes on all three OSes, device + Bluetooth probe, signal-path report, protocol + `ts-rs` types, CLI.
- Electron shell with EngineSupervisor, preload API, and a minimal Studio-styled screen: open file(s), transport controls, seek, device/mode picker, signal-path panel.
- **Done when:** golden, gapless and bit-perfect tests pass in CI on all OSes, and a user can open any supported file and see an accurate signal path.

**M2 — Library + full UI**
- Scanner/watcher, DB + FTS, artwork + palette, all views in §5.3, queue, playlists + M3U8, persistent player bar, OS media integration, keyboard shortcuts, session restore.
- **Done when:** the 100k benchmark meets its targets and the Playwright smoke tests pass.

**M3 — Extras**
- EQ + presets, crossfade with smart skip, ReplayGain UI, synced lyrics view, expanded Now Playing with dynamic colours, spectrum visualizer, mini-player window, tray.

**M4 — Release**
- GitHub Actions: build the engine for 6 targets (and also without the fdk-aac feature), package with electron-builder (NSIS + portable zip for Windows, DMG for macOS, AppImage + deb + rpm for Linux). Auto-update through electron-updater with GitHub Releases. Signing is optional and enabled when secrets are present; unsigned builds are documented.

---

## 10. Out of scope (v1)
Streaming services, cloud sync, online metadata/lyrics/artwork fetching, scrobbling, smart playlists, parametric EQ, a light theme, i18n, mobile/remote control, CD ripping, tag editing, podcast support, controlling the Bluetooth codec choice from the app.

## 11. Risks and open questions
- **fdk-aac licensing:** the Fraunhofer license permits redistribution but grants no patent rights, and some distros treat it as non-free. Mitigation: it's a cargo feature, CI also produces a no-fdk build, and the notice is in THIRD_PARTY_NOTICES.
- **WASAPI exclusive / CoreAudio hog mode quirks:** device-specific behaviour (format negotiation, rate switching latency). Mitigation: a capability probe before opening, graceful fallback to shared mode, and the manual DAC test matrix.
- **Windows/macOS Bluetooth codec visibility:** there's no public API, so the codec display is Linux-only by design. If a reliable documented API emerges, `probe` is the single place to add it.
- **`souvlaki` on Windows needs an HWND from Electron:** if this proves fragile in a separate process, a fallback is SMTC via the `windows` crate with `SystemMediaTransportControlsInterop::GetForWindow` using the passed handle.
- **Rust isn't installed** on the development machine yet. M1's plan starts with toolchain setup (rustup, plus MSVC build tools on Windows, and the C toolchain for libopus/fdk-aac).
