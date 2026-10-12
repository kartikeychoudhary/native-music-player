# Aurora

Aurora is a native hi-res music player: a Rust audio engine (`aurora-engine`) runs as a sidecar process and an Electron app provides the UI. The two talk JSON-RPC 2.0 over stdio.

This is milestone 3: the audio engine, the app shell, the music library and the first extras. Playback of local files, the signal-path panel, the bit-perfect toggle, library browsing (albums, artists, genres, playlists), search and the play queue work, and so do the equalizer, crossfade, ReplayGain, synced lyrics, the spectrum visualizer, the mini-player and the tray.

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
npm run e2e                # build the engine and the app, then run the Electron smoke tests
npm run package            # release engine + Windows/macOS/Linux package for this machine, in app/dist
```

The end-to-end tests drive the real app against the real engine with a silent `null` output (`AURORA_OUTPUT=null`) and a throwaway data directory, so they need no audio device and never touch your library. They expect the fixtures from `npm run fixtures`.

### Packaging locally

`npm run package` builds the release engine, stages it into the app and runs electron-builder for this machine's OS and architecture, without publishing or signing. `app/dist/<os>-unpacked` is the unpacked app; `AURORA_PACKAGED_EXE=<path to its executable> npm --prefix app run e2e:packaged` smoke-tests it.

## Library

Aurora indexes the folders you add into a SQLite library. Everything it stores lives in the OS app-data directory (for example `%APPDATA%\Aurora` on Windows, `~/Library/Application Support/Aurora` on macOS, `~/.config/Aurora` on Linux):

- `library.db`, the track, album and artist index
- `artwork/`, the cover cache (WebP covers at 64, 300 and 1000 px)
- `session.json`, the play queue and position, restored paused on launch
- `settings.json`, window bounds, last view and preferences

The engine takes the location as `aurora-engine rpc --data-dir <dir>`. In unpackaged runs, setting `AURORA_DATA_DIR` points both the engine and the app's own settings and logs at a different folder, which is how the end-to-end tests stay away from your real library.

A file watcher picks up changes in the added folders within about 2 s. Files that disappear are kept as missing for 30 days (so an unplugged drive does not wipe your playlists) and are then removed.

## Library benchmark

The library must scan 100k tracks in under 2 minutes on an SSD, and every paging or search query must take under 50 ms. An ignored test checks both. It generates about 100k tiny tagged WAV files into the cargo target directory (once, then reuses them), scans them into a fresh database, then times each query as the median of five runs. Set `AURORA_BENCH_TRACKS` to change the corpus size.

```sh
cargo test --release --manifest-path engine/Cargo.toml -p library --test bench_100k -- --ignored --nocapture
```

Measured on Windows 11 Pro, Intel Core i5-12400F, 32 GB RAM, NVMe SSD (100,000 tracks, release build):

| Step | Result |
| --- | --- |
| Generate corpus (one time) | 71.5 s |
| First scan | 15.0 s (17.3 s on the first run) |
| Rescan, nothing changed | 2.2 s |
| Open an existing 100k library (includes `quick_check`) | about 0.4 s |

| Query | Median |
| --- | --- |
| albums by title | 0.9 ms |
| albums by artist, page at offset 4000 | 6.5 ms |
| albums by year, descending | 2.3 ms |
| albums recently added | 14.5 ms |
| tracks by title, page at offset 50,000 | 25.8 ms |
| tracks by artist, page at offset 50,000 | 25.0 ms |
| tracks of one artist | 0.2 ms |
| albums of one artist | 0.2 ms |
| artists page | 0.4 ms |
| search, artist prefix | 0.5 ms |
| search, broad 2-character prefix ("ka", about 27,000 matches) | 24.7 ms |
| search, two words | 1.8 ms |
| album detail | 0.1 ms |
| artist detail | 0.3 ms |

Windows Defender real-time scanning can slow corpus generation and the first scan. If the scan misses its target, exclude the target directory or run the test a second time.

## Sound

Settings, Sound holds the DSP features. The EQ and crossfade apply to everything that plays in processed mode, including files opened from outside the library. ReplayGain applies to library tracks only.

- **EQ:** 10 bands with presets. Your own presets are kept in `dsp.json` in the data directory. The EQ is bypassed on outputs with more than 8 channels.
- **Crossfade:** 0 to 12 s. With smart skip, tracks of an album played in order stay gapless; a manual skip uses a short fade instead. When a track's length isn't known, the tracks join gaplessly instead of crossfading.
- **ReplayGain:** track, album or off. Gain is peak-protected so it never clips. It works for library tracks only, and Opus R128 gain tags are included. Opus files scanned before this milestone need a rescan (Settings, Library folders, Rescan all) to pick up their R128 gains.
- **Bit-perfect:** the EQ, crossfade and ReplayGain are all off in bit-perfect mode, and the signal path says so.

## Lyrics

Lyrics come from a sidecar `.lrc` file next to the track (including enhanced word timing), from embedded lyrics tags, or from ID3 SYLT frames. Synced lyrics are preferred over plain text. The lyrics tab of Now Playing follows the current line and seeks when you click a line.

## Windows

- **Mini-player:** Ctrl/Cmd+M opens a small always-on-top window with the track and the playback controls. It remembers where you left it.
- **Tray:** the tray menu has play/pause, next, previous, show/hide, the mini-player and the current track.
- **Now Playing:** Ctrl/Cmd+L opens the expanded Now Playing view on the lyrics tab. It also has the spectrum visualizer.

## Protocol

Besides JSON-RPC, the engine sends binary spectrum frames on stdout: a `0x1E` marker byte, a u32 little-endian length, then a payload whose kind byte is `1` followed by 64 bins. Frames are sent only between `viz.subscribe` and `viz.unsubscribe`, so the visualizer costs nothing while Now Playing is closed.

## Known limitations (M3)

- Crossfade is not applied when the next track would need a different output format (for example a device change mid-transition).
- Files opened from outside the library get no ReplayGain.

## Known limitations (M2)

- DSF and DFF files are indexed with their file names as titles. Tag reading for the DSD containers comes later.
- macOS media keys are best-effort.
- The Windows media flyout shows Aurora's name and icon for installed copies. The portable zip has no Start-menu shortcut, so the flyout shows the engine's file name. Cover art may not appear there: the 300 px cache is WebP, which the system media controls did not load in testing.

## Bluetooth codec display

LDAC and similar codecs are Bluetooth transmission codecs, not file formats. Aurora hands unaltered audio to the OS (exclusive or bit-perfect where possible) so the Bluetooth stack can use its highest tier.

Aurora only shows the active Bluetooth codec where the OS reports it. On Linux it is read from PipeWire. On Windows and macOS there is no public API for it: Aurora detects that the output is a Bluetooth device and shows "Codec not reported by the OS". It never guesses.

## License

MIT, see [LICENSE](LICENSE). Third-party components and their licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
