# M4 execution notes

Plan: `docs/superpowers/plans/2026-10-12-m4-release.md`. Spec: `docs/superpowers/specs/2026-10-10-aurora-music-player-design.md` §9 M4.
Executed subagent-driven on branch `m4-release`: one implementer and one reviewer per task, then a whole-branch review and one fix wave. Every task ended review-clean. Tasks 7, 8 and 9 each needed one fix round. The final review found no Critical or Important issues.

## Rulings

Decisions taken during execution, each with what it costs if wrong.

- Ruling P1: installKind picks `path.win32` when `o.platform === 'win32'` and `path.posix` otherwise (the plan's "must not call path.win32 on macOS/Linux" reasoning was wrong: the platform being modelled is the parameter, not the host) — the test must pass on all three CI OSes — cost if wrong: none (pure function).
- Ruling P2: `bundledPackages(target, outDir)` takes its output dir from the config (`resolve('build/generated')`, cwd-relative like the existing aliases) instead of importing `REPO_ROOT` — avoids import.meta.url pointing at electron-vite's bundled temp config — cost if wrong: one parameter.
- Task 3: Ruling: Cargo.lock written with LF by cargo on this CRLF checkout — harmless (core.autocrlf=true normalizes on staging); set-version.ts left as is — cost if wrong: a noisy git status after a bump
- Task 3: Ruling P3: add app/build-tools/package.json {"type":"module"} (carried into Task 4) to silence Node's MODULE_TYPELESS_PACKAGE_JSON warning on every build-tool run without changing the app's module mode — cost if wrong: one file
- Task 7: Ruling: REQUIRED_TEXTS `audiopus_sys` → `opusic-sys` (the crate actually in the graph; its root LICENSE holds the Opus BSD text) — plan named the wrong crate — cost if wrong: none
- Task 7: Ruling: crate set filtered by `cargo tree -p aurora-engine --target <t> -e normal [--no-default-features]` because `cargo metadata` unifies features workspace-wide (fixtures-gen pulls engine-core default features → fdk-aac stayed in the no-fdk list) — cost if wrong: one helper
- Task 8: Ruling: opus static CRT via `.cargo/msvc-static-crt.cmake` toolchain file (OPUS_STATIC_RUNTIME on) wired by per-target CMAKE_TOOLCHAIN_FILE_* — the brief's `[env] CMAKE_MSVC_RUNTIME_LIBRARY` cannot work (cmake doesn't read it; opus sets its runtime from its own option) — cost if wrong: a cmake file to maintain
- Task 8: Ruling: dist-local names targets per platform (`--win nsis zip` …) because electron-builder ignores CLI arch flags when targets list their own archs — carry into Task 10 (package-ci builderArgs) — cost if wrong: wrong-arch local packages
- Task 8: Ruling: package.json gets homepage = GitHub repo and author = Kartikey Choudhary <kartikey31choudhary@gmail.com> (the address already public in the repo's git history) — deb/rpm require a maintainer email — cost if wrong: the user's email ships in Linux package metadata; change to another address in app/package.json
- Task 8: Ruling: ${platform} in extraResources is the build host's platform — CI packages each OS on its own native runner, and afterPack fails a cross-OS build loudly, so no per-platform extraResources blocks — cost if wrong: someone cross-packaging gets a clear afterPack error
- Task 9: Ruling: `Python-2.0` added to licenses ALLOWED (argparse via electron-updater→js-yaml; PSF-style permissive, ships its LICENSE) — cost if wrong: one allowlist entry
- Task 9: Ruling: canonical MIT fallback extended to npm packages using package.json `author` (lazy-val ships no license file) — cost if wrong: none
- Task 9: Ruling: AutoUpdaterLike.on callback typed `(arg: any)` instead of `(arg: never)` (never made neither the real AppUpdater nor the fake type-check) — cost if wrong: looser typing on one interface
- Task 10: Ruling: package job copies release files flat into upload/ before upload-artifact (multiple paths keep their dist/ and dist-nofdk/ subfolders relative to the common ancestor, which would break `sha256sum *` and `gh release create dist/*`) — cost if wrong: one extra step
- Final: Ruling: one fix wave for the cheap pre-tag items — sysctl `|| true`; try/finally around quitAndInstall; libstdc++ in allowlist; session save before io.shutdown; automatic checks don't publish 'checking'; no-fdk smoke step in release.yml; README fixes — each removes a first-release failure or support risk for a one-line cost — cost if wrong: small diff
- Final: Ruling (declined-to-judge): first-CI-run risks (runner labels, llvm-objdump on arm, pipewire 0.3.48, Playwright vs hardened ad-hoc app) and signing/notarization stand as pending human checks — unverifiable without CI/secrets
- Final: Ruling (declined-to-judge): packaged builds honouring AURORA_ENGINE_PATH stands (pre-M4; env control already implies code execution); inherited APPIMAGE env from a launcher stands (electron-updater behaviour, rare)
- Final: Ruling: install() swallows a synchronous quitAndInstall throw (logged) and always schedules the quit fallback — the fallback quits cleanly — cost if wrong: an install error is only in the log

Also recorded:
- **Pinned versions:**
  - electron-builder 26.17.0 (dev);
  - electron-updater 6.8.10 (runtime, the only `dependencies` entry);
  - yaml 2.9.1 (dev).
- **Electron behaviour:** Electron honours `--user-data-dir`, so the packaged smoke test isolates its profile without any code change.
- **electron-vite 5:** it externalizes `dependencies` by default, so `electron-updater` stays a runtime `require`.

## Deferred minors

None was judged a blocker by the final review.

- Task 1: minor (deferred): unmixed-write branch in crossfade_step now unreachable (unmixed_end always 0); stale Crossfade/pos doc comments; awkward "(plus nothing: …)" in tail.rs doc
- Task 1: minor (deferred): no player-level test for an underestimated total_frames / early writes before tail_done (memory bound covered by TailBuffer unit test)
- Task 2: minor (deferred): worker-thread panic → no reply (note: the engine's panic hook exits the process on any thread's panic, so the supervisor restarts it; not silent in practice)
- Task 2: minor (deferred): AURORA_TEST_IO_DELAY_MS compiled into release builds (brief-mandated); unreachable -32601 fallback in worker; no integration test for drain-on-shutdown or slow-method notification
- Task 2: minor (deferred, final review): slow calls can now complete out of order w.r.t. later fast calls — confirm no app code relies on reply order for lyrics.get / library.track
- Task 3: minor (deferred): check-version `--tag` with no value fails open; SEMVER accepts leading zeros (Cargo.toml written before npm validates); shell:true unquoted path + DEP0190 warning in set-version; no CRLF/EOF test cases; setEngineVersion string replacer interprets $
- Task 4: minor (deferred): AboutSection appInfo() promise lacks .catch and unmount guard; label/value pairs not a <dl> (Shortcuts uses dl); HE-AAC row shows bare "…" when engine not ready; EngineReady import out of alphabetical order; no openLogs assertion
- Task 5: minor (deferred): installKind `exists` optional vs Interfaces "required" (falls back to existsSync); cli test proves flag acceptance only (effect covered by Windows unit test)
- Task 6: minor (deferred): make-icon.ts prints MODULE_TYPELESS_PACKAGE_JSON (iconPixels.ts lives outside build-tools' type:module scope); iconPixels assumes inset < size/2; icon test samples only centre/edge pixels
- Task 7: minor (deferred): copyright lines carry Cargo.toml author emails verbatim; npm packages with allowed license but no text hard-fail (no authors fallback); linkedCrates/CLI untested; resolve('build/generated') cwd-dependent; npm closure keyed by dir with mixed separators (deduped later)
- Task 8: minor (deferred): msvc-static-crt.cmake sets OPUS_STATIC_RUNTIME without FORCE (stale CMakeCache / restored rust-cache keeps /MD objects → LNK4098 warnings, benign); dist-local crashes cryptically on unsupported platforms; smoke test leaves its temp dir; Linux deps allowlist unverified on a real Linux engine (e.g. libdbus via MPRIS)
- Task 8: minor (deferred): unused _path/_profile destructured names in packaged.spec.ts
- Task 9: minor (deferred): renderer updateState() lacks .catch; Check now enabled (no-op) in 'ready'
- Task 9: minor (deferred, final review): on signed macOS, MacUpdater.quitAndInstall may wait on Squirrel — the 5 s quit fallback could cut a slow handoff short; install() has no re-entry guard; a synchronous throw from quitAndInstall skips the fallback
- Task 10: minor (deferred): workflow_dispatch on a tag ref creates a draft even with "draft" unticked (use github.event_name == 'push'); no structural test for the flattened dist upload / merge-multiple hand-off; AppArmor sysctl copied to the ubuntu-22.04 package job may fail if the key is absent

Final-review minors still open after the fix wave:
- In the packaged spec, the standard-variant negative `without fdk-aac` assertion doesn't auto-wait.
- An automatic auto-mode check that finds nothing leaves the state at `idle`, where notify mode shows `current`.
- No test covers `update-downloaded` arriving during an automatic auto-mode check.

## Pending human checks

These need CI, other operating systems, signing secrets or real hardware. None could be run by the executor. Record results here when done.

1. **CI is blocked.** The repository is private and Actions billing blocks Linux and macOS runs. The free arm64 runners are public-repository only.
   - Unblock it by making the repository public, or by fixing billing.
   - Then run Release by hand (Actions → Release → Run workflow).
   - Watch these first-run risks:
     - the `windows-11-arm` and `macos-15-intel` runner labels;
     - `llvm-objdump` on the arm runners;
     - the engine building against PipeWire 0.3.48 headers on `ubuntu-22.04`. If that fails, move Linux to 24.04 and raise the documented glibc floor;
     - Playwright driving the ad-hoc signed macOS app;
     - the no-fdk smoke step;
     - the `libstdc++` entry in the Linux deps allowlist.
2. **Auto-update can't work until the repository is public.** electron-updater reads GitHub Releases without a token only from a public repository.
3. **Installer and updater rehearsal.**
   - Install an rc build from the NSIS installer.
   - Publish the next rc, and confirm the banner, **Restart now** while music plays, and relaunch with the queue restored (paused).
   - No packaged auto-mode app has run electron-updater yet; the local smoke test runs `win-unpacked`, which is notify mode.
   - On signed macOS, check that the 5 s quit fallback doesn't cut short Squirrel's handoff.
4. **Windows media flyout:** on an NSIS install, it shows "Aurora" with the icon.
5. **Fresh Windows VM** without the VC++ redistributable: the installed app starts and plays.
6. **macOS:**
   - the ad-hoc signed arm64 build opens after **Open Anyway**;
   - the x64 build runs on Intel or under Rosetta;
   - the signed and notarized path works once secrets exist.
7. **Linux:** the AppImage, deb and rpm on Ubuntu 22.04, Ubuntu 24.04 and Fedora 41+. Check that they start, play, and show the tray. Also check the mini-player's always-on-top behaviour on Wayland.
8. **Spec §7 hardware matrix:**
   - a wired USB DAC on each OS;
   - a Bluetooth LDAC headset on Linux;
   - AAC Bluetooth on macOS and Windows.
9. **Carried over:** the pending checks in `docs/superpowers/m3-execution-notes.md` (crossfade and EQ by ear, tray and mini-player on every OS, visualizer CPU, media keys, a visual and keyboard pass).

## Decisions for the maintainer before the first release

- **Name:** keep "Aurora", or rename now (`productName`, `appId`, `APP_ID`, `RELEASES`; see `docs/RELEASING.md`).
- **Linux package maintainer email:** the deb and rpm metadata carry `kartikey31choudhary@gmail.com`, the address already in the git history. To use a different address, change `author.email` in `app/package.json`.
- **Update check default:** it is on by default, with a switch in Settings → About. The spec's "no network features in v1" yielded to M4's auto-update requirement.
