# Aurora

Aurora is a native hi-res music player: a Rust audio engine (`aurora-engine`) runs as a sidecar process and an Electron app provides the UI. The two talk JSON-RPC 2.0 over stdio.

This is milestone 1: the audio engine and the app shell. Playback of local files, the signal-path panel and the bit-perfect toggle work. Library, EQ, lyrics and the other planned features are not built yet.

## Prerequisites

- Rust (stable; the toolchain is pinned by `rust-toolchain.toml`)
- CMake and a C/C++ toolchain (on Windows, Visual Studio Build Tools), needed to build libopus and fdk-aac
- Node 24
- ffmpeg, only to generate the test fixtures

## Commands

Run these from the repository root.

```sh
npm --prefix app install   # once, to install the app dependencies
npm run fixtures           # generate audio fixtures into fixtures/out (needs ffmpeg and bash)
npm run dev                # build the engine, then start the Electron app in dev mode
npm test                   # engine tests (cargo) and app unit tests (vitest)
npm run e2e                # build the engine and the app, then run the Electron smoke test
```

The end-to-end test drives the real app against the real engine with a silent `null` output (`AURORA_OUTPUT=null`), so it needs no audio device. It expects the fixtures from `npm run fixtures`.

## Library benchmark

The library must scan 100k tracks in under 2 minutes on an SSD, and every paging or search query must take under 50 ms. An ignored test checks both. It generates about 100k tiny tagged WAV files into the cargo target directory (once, then reuses them), scans them into a fresh database, then times each query as the median of five runs. Set `AURORA_BENCH_TRACKS` to change the corpus size.

```sh
cargo test --release --manifest-path engine/Cargo.toml -p library --test bench_100k -- --ignored --nocapture
```

Measured on Windows 11 Pro, Intel Core i5-12400F, 32 GB RAM, NVMe SSD (100,000 tracks, release build):

| Step | Result |
| --- | --- |
| Generate corpus (one time) | 71.5 s |
| First scan | 15.2 s (17.3 s on the first run) |
| Rescan, nothing changed | 2.3 s |
| Open an existing 100k library (includes `quick_check`) | about 0.4 s |

| Query | Median |
| --- | --- |
| albums by title | 1.2 ms |
| albums by artist, page at offset 4000 | 8.6 ms |
| albums by year, descending | 2.7 ms |
| albums recently added | 15.2 ms |
| tracks by title, page at offset 50,000 | 25.4 ms |
| tracks by artist, page at offset 50,000 | 27.5 ms |
| tracks of one artist | 0.2 ms |
| albums of one artist | 0.2 ms |
| artists page | 0.4 ms |
| search, artist prefix | 0.8 ms |
| search, broad 2-character prefix ("ka", about 27,000 matches) | 44.7 ms |
| search, two words | 3.4 ms |
| album detail | 0.1 ms |
| artist detail | 0.3 ms |

Windows Defender real-time scanning can slow corpus generation and the first scan. If the scan misses its target, exclude the target directory or run the test a second time.

## Bluetooth codec display

LDAC and similar codecs are Bluetooth transmission codecs, not file formats. Aurora hands unaltered audio to the OS (exclusive or bit-perfect where possible) so the Bluetooth stack can use its highest tier.

Aurora only shows the active Bluetooth codec where the OS reports it. On Linux it is read from PipeWire. On Windows and macOS there is no public API for it: Aurora detects that the output is a Bluetooth device and shows "Codec not reported by the OS". It never guesses.

## License

MIT, see [LICENSE](LICENSE). Third-party components and their licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
