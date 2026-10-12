# M3 execution notes

Outcomes, rulings and deferred findings from the subagent-driven execution of docs/superpowers/plans/2026-10-12-m3-extras.md (branch m3-extras). Generated from the execution ledger and the final-fix wave.

## Outcomes

- All 16 plan tasks done, then a whole-branch final review (0 Critical, 1 Important, 11 minors) and a final fix wave, listed below.
- Suites after the fix wave: engine workspace 499 passed, 0 failed, 2 ignored (`cargo test --workspace`); app 185 tests in 40 files (Vitest); e2e 3 Playwright specs (smoke, library, m3). `cargo clippy --workspace --all-targets -- -D warnings` and `npm run typecheck` are clean.
- 100k-track benchmark (quiet machine): first scan 16.7 s, rescan 2.5 s, slowest query 27.4 ms. An earlier run showed an 86 s first scan under load; re-running on a quiet machine gave the numbers above, so it was load, not a regression.
- Final fix wave: the Credits tab says credits (not lyrics) are library-only; lyrics decode CP1252 and UTF-16 with a BOM; signal-path titles for ReplayGain, EQ and Crossfade; a bit-perfect to processed return-path test; the two 300 ms-sleep player tests now wait on the feeder (`NullHandle::buffered_frames`), and the EQ one also waits for the applied change before pulling; `dsp.json` keeps valid presets and valid settings and resets only what is invalid (struct-level `serde(default)`, per-preset parsing, name checks); the mini-player's default position is on the primary display; the EQ filter memory is reset when the EQ switches on; the spectrum canvas size is rounded; README gaps (Opus rescan, crossfade without a known length, EQ above 8 channels).

## Rulings

- Task 3: Ruling: fix the plan-mandated staleness — refresh_dsp_stages rewrites DSP stages on every marker (each with its own idx, since Task 4 makes stages per-track), event only for the front marker — spec §3.8 signal path must describe what plays; Task 4 builds on it — cost if wrong: a few lines.
- Task 4: Ruling: brief's refresh_dsp_stages instruction (use the audible idx for all) is superseded by the Task 3 fix — each marker's stages use that marker's own idx (`|m| self.dsp_stages(m.idx, allowed)`) — ReplayGain differs per track — cost if wrong: one line
- Task 5: Ruling: Important 2 is plan-mandated test weakness — add content checks + a ≥4 s-fade seek-while-mixing test — the plan's Review Focus claims this coverage — cost if wrong: test time only.
- Task 7: Ruling: fix both plan-mandated defects (checked arithmetic → tag skipped; saturating offset shift; only non-empty plain fallbacks; drop bare [key:value] lines from sidecar plain text) — spec §3.9 + Review Focus 3 ("nothing panics", "first plain source wins") — cost if wrong: a few lines.
- Task 9: Ruling: fix all three; also drop a window's subscription when its page reloads/navigates or its renderer dies (did-start-navigation main frame / render-process-gone) — Review Focus 4 (subscriptions follow windows, engine stops when nobody wants frames) — cost if wrong: small.
- Task 13: Ruling: carried to Task 15 — mount `<LyricsView key={trackId} trackId={trackId} />` so each track gets a fresh query state; add a Task 15 test for a track change — fixes it at the only mount point without changing the shared hook — cost if wrong: one prop.
- Final: Ruling: fix wave includes F1 Credits-tab copy, F2 CP1252 + UTF-16-BOM lyrics decoding, F3 STAGE_TITLE for replaygain/eq/crossfade, F5 bit-perfect→processed return test, F6 deterministic waits for the 300 ms-sleep tests, F7 serde defaults on DSP types + keep valid presets / reset only invalid fields, F9 mini default on primary display, F10 EqState reset on enable, Task14 Math.round canvas size, F11 README gaps, and m3-execution-notes.md (rulings, deferred list, manual checks) — all cheap, user-visible or CI-affecting — cost if wrong: one fix wave.
- Final: Ruling: Important 1 manual checks — automatable parts are covered (e2e: mini-player always-on-top, visualizer frames, EQ stage); by-ear crossfade/EQ checks and tray/mini-player on macOS/Linux need a human and real hardware, so they're recorded in m3-execution-notes.md as pending human checks — I can't listen or run other OSes — cost if wrong: an audible issue surfaces in manual testing.
- Final: Ruling: deferred to M4/pre-release — F4 crossfade tail cap (unbounded if total_frames underestimates), F8 lyrics.get file I/O on the RPC thread — both need design beyond a fix wave; noted in execution notes — cost if wrong: a rare dropout or a slow-share stall until M4.
- Final: Ruling (declined-to-judge items): lyrics for non-library files (spec defines lyrics.get(trackId)), crossfade stage "active" despite smart skip (stage describes the setting), queued RG not refreshed after rescan, equal-power sum >0 dBFS (spec-mandated curve), missing peak blocks boosts (plan-mandated), Wayland always-on-top (platform), macOS/Linux runtime (manual checks) — all stand as-is — cost if wrong: minor UX polish later.

## Deferred minor findings

Findings that remain after the fix wave, by task. Items the wave fixed were removed; partly fixed ones were reworded.

- Task 1: minor (deferred): commit trailer names Haiku instead of Opus (harness attribution) — cosmetic
- Task 1: minor (deferred): protocol bands Vec<f32> unvalidated in protocol crate — validation belongs to Task 6 (DspStore::set_eq) — check there
- Task 2: minor (deferred): channels > 8 clamp mis-maps interleave — carried to Task 3 (bypass EQ when channels > 8)
- Task 2: minor (deferred): headroom counts bands inactive above Nyquist (conservative)
- Task 2: minor (deferred): NaN band gain poisons filter state (no is_finite guard; Task 6 validation should prevent)
- Task 2: minor (deferred): seqlock single-writer by convention; test single-threaded; no chunked/channel-independence/I24/I32 active-path tests; denormals
- Task 3: minor (deferred): no test for >8ch EQ bypass, EQ→volume→fade order, or BitPerfect-mode processed fallback
- Task 3: minor (deferred): eq stage shows active for >8ch processed stream while apply_eq=false
- Task 3: minor (deferred): eq_state is not reset on seek (brief transient); disable→enable is fixed (reset when the EQ switches on)
- Task 3: minor (deferred): refresh_dsp_stages may emit SignalPath for a front marker not yet shown (pending_start)
- Task 4: minor (deferred): Opus files scanned before M3 need a rescan to pick up R128 gains (documented in the README)
- Task 4: minor (deferred): rg_stage strings (off / no data / album / peak-limited / bypassed) untested
- Task 4: minor (deferred): enrich + restore closure untested behaviorally
- Task 4: minor (deferred): SetReplayGain resyncs even when the mode is unchanged (audible re-seek for no change)
- Task 4: minor (deferred): rg_stage "-0.0 dB" / "+0.0 dB (track)" cosmetics; entries_for queries album/disc/track twice
- Task 5: minor (deferred, plan-mandated): tail buffering holds up to ~13 s of A (≈160 MB worst case), unbounded if total_frames underestimates, burst decode with no writes — suggest streaming unmixed tail frames
- Task 5: minor (deferred): xfade_declined keyed on A's idx only, not remapped in rehome (crossfade wrongly suppressed after InsertNext/Move)
- Task 5: minor (deferred): edit_queue resync during an active mix has no fade (audible jump)
- Task 5: minor (deferred): seek within A during tail reuses A's EOF decoder — seek-after-EOF unverified for opus/fdk decoders
- Task 5: minor (deferred): M2 seek-during-callback can repeat one callback period (position published at end of fill)
- Task 6: minor (deferred): no test pins "SetReplayGain only on change"
- Task 6: minor (deferred): load-fallback test coupled to pretty-JSON text; no fsync before rename; dsp mutex held across send/notify (brief-mandated, no practical impact)
- Task 7: minor (deferred): blank Lyrics key hides non-blank UnsyncLyrics (filter outside find_map); only primary/first tag consulted
- Task 7: minor (deferred): [offset:] applies only to lines after it; non-numeric offset resets to 0; [00:01][ar:x]text keeps tag in text; sidecar read uncapped
- Task 7: minor (deferred): untested — SYLT vs plain sources, Latin-1/BOM end-to-end through a file (the CP1252 and UTF-16 decoders themselves are unit-tested), offset on word times, more malformed tags, USLT/©lyr/UNSYNCEDLYRICS
- Task 7: minor (deferred): sidecar_plain drops untimed lines like "[Chorus: x2]" (indistinguishable from metadata); embedded plain lyrics get no metadata stripping
- Task 8: minor (deferred, plan-mandated): viz thread wakes at 30 Hz even with viz off; every stream open builds a fresh Analyzer/FFT plan
- Task 8: minor (deferred): stale ~46 ms window across off→on; duplicated sample decode vs bytes_to_f32; tap pre-fade level; tap tests I16 only; viz_rpc doesn't test unsubscribe/pause/JSON-after-frame
- Task 9: minor (deferred): no cap on declared frame length (corrupt header stalls JSON); zero-length frame untested; decoder reallocates per push (quadratic for multi-MB JSON lines)
- Task 9: minor (deferred): unsubscribe/resubscribe race can send a duplicate subscribe; retry test doesn't actually verify the .catch
- Task 9: minor (deferred): hub drop/sync not covered through real Electron send (e2e covers smoke/library only)
- Task 10: minor (deferred): WindowManager untested beyond miniPosition; macOS activate doesn't restore main while mini is open; MiniApp aria-labels/title outside strings.ts; straddling mini position resets
- Task 11: minor (deferred): createTray dedupe/click untested (Tray/Menu mocked); onVisibilityChange untested; dedupe key includes title/artist (menu rebuilt on track change)
- Task 11: minor (deferred): tray destroy() never called; Windows tooltip 127-char limit unclamped; Linux ignores tray click when a context menu is set
- Task 12: minor (deferred, plan-mandated): fast slider drag can flicker from stale dsp.changed echoes (no coalescing); presets list not reloaded when dsp.changed names an unknown preset
- Task 12: minor (deferred): savePreset/deletePreset success + error paths untested; bit-perfect disable asserted on 3 controls only; sliders lack aria-valuetext (dB); crossfade/RG rows not dimmed like EQ panel; failed reload after failure leaves optimistic value
- Task 13: minor (deferred): empty timed lines render an empty focusable button; binary search assumes sorted, all-timed (true for engine output); ticker runs for plain/no lyrics; Nothing-playing / reduced-motion / scrollIntoView untested; scrollIntoView may scroll ancestors; lookup errors read as "no lyrics"
- Task 14: minor (deferred): the held spectrum frame is not redrawn on resize or accent change (the fractional-DPR reallocation is fixed)
- Task 14: minor (deferred): per-frame getComputedStyle; reduced-motion read once per effect; setViz is per-window boolean (two Spectrum instances in one window would conflict); draw path / reduced-motion / rAF cancel untested
- Task 15: minor (deferred, plan-mandated): Ctrl+L on Now Playing resets stored tab to lyrics before going back; toggleGo has no canGoBack fallback (PlayerBar does) — inconsistent; tabs lack roving tabIndex/arrow keys
- Task 15: minor (deferred): useAccentTint wiring untested; Signal path tab assertion weak; embedded QueueView duplicates the Queue h1; --accent-2 not tinted
- Task 16: minor (deferred): e2e finds mini window by title (fragile; URL '#/mini' would be sturdier); title fix in renderer (page-title-updated in main would be at the source); protocol note doesn't state bin type; README benchmark table not refreshed

## Deferred to M4

- Crossfade tail cap: the outgoing track's tail is buffered in memory (about 13 s, up to ~160 MB worst case) and is unbounded if a decoder's `total_frames` underestimates the real length. Stream the unmixed tail frames instead, or cap it and fall back to a gapless join. (Task 5 minor, final review F4.)
- `lyrics.get` does file I/O (sidecar read, tag parse) on the RPC thread, so a slow network share can stall other calls until it returns. Move it to a worker or the library thread. (Final review F8.)
- Wayland always-on-top for the mini-player is a compositor limitation; check it in the platform matrix.
- macOS and Linux runtime behaviour of the tray, mini-player and visualizer (only Windows was run); see the pending human checks.
- Carried over from M2 and still open: macOS souvlaki run-loop and MPRIS runtime in the M4 matrix; transitive license notices at packaging; HDD/NAS startup walk cost.
- Declined in the final review as acceptable for M3, to revisit if reported: lyrics for files outside the library (the spec defines `lyrics.get(trackId)`), the crossfade stage reading "active" when smart skip keeps a join gapless (it describes the setting), queued ReplayGain not refreshed after a rescan, equal-power sum above 0 dBFS (spec-mandated curve), a missing peak block boosting (plan-mandated).

## Pending human checks

These need real hardware, ears or other operating systems; none could be run by the executor. Record results here when done.

M3 exit checks (plan, "Manual checks before calling M3 done"):

1. Crossfade by ear: set 4 s, play two tracks from different albums. Expect a smooth equal-power fade, no level dip or jump. The same album played in order must stay gapless. Covered automatically: player tests check the overlap length and content sample by sample, smart skip, short tracks, repeat-one, bit-perfect, and seeks and queue edits during the overlap. Not covered: how it sounds.
2. EQ by ear: sweep the 62 Hz band and hear it within a fraction of a second; switch to bit-perfect and confirm it is bypassed. Covered automatically: a player test checks a change reaches buffered audio within 100 ms, bit-perfect bytes stay identical with the EQ on, and e2e checks the EQ stage in the signal path. Not covered: hearing it.
3. Tray on Windows, macOS and Linux: icon visible, menu labels follow play/pause, Show/Hide works with the main window minimised. Covered automatically: menu, tooltip and icon unit tests. Not covered: any OS.
4. Mini-player: stays on top of other apps; its position survives a restart; after unplugging its monitor it reopens on a visible screen. Covered automatically: e2e checks it is always on top and its controls work; unit tests check the position clamping, including the primary-display default. Not covered: another app on top, restart persistence on a real screen, unplugging a monitor.
5. Visualizer CPU: the engine's CPU use drops back when Now Playing is closed or the window is hidden (`viz.unsubscribe`). Covered automatically: unit tests that the spectrum unsubscribes when hidden or unmounted and that the engine sends no frames while off; e2e checks frames reach the canvas. Not covered: measuring CPU.

Carried over from M2:

6. Press a real media key while playing on Windows and check the media flyout (programmatic SMTC toggle/next already verified).
7. CI on Linux and macOS is blocked by GitHub billing: the cfg-gated MPRIS/macOS code and the unix test branches are compile-unverified.
8. Visual and keyboard pass through every view: Studio palette, amber focus rings, Tab order through the player bar, player bar fit at 940 px.
