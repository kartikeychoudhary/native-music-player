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

## Bluetooth codec display

LDAC and similar codecs are Bluetooth transmission codecs, not file formats. Aurora hands unaltered audio to the OS (exclusive or bit-perfect where possible) so the Bluetooth stack can use its highest tier.

Aurora only shows the active Bluetooth codec where the OS reports it. On Linux it is read from PipeWire. On Windows and macOS there is no public API for it: Aurora detects that the output is a Bluetooth device and shows "Codec not reported by the OS". It never guesses.

## License

MIT, see [LICENSE](LICENSE). Third-party components and their licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
