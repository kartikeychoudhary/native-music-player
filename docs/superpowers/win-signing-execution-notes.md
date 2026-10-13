# Windows signing, installer and updates: execution notes

Plan: `docs/superpowers/plans/2026-10-13-windows-signing-installer.md`. Executed subagent-driven on branch `win-signing`: one implementer and one reviewer per task, then a whole-branch review and one fix wave. Every task ended review-clean.

## Outcome

- **Signing is not active.** SignPath Foundation declined the application on 2026-10-13, for lack of public visibility, and the maintainer chose unsigned Windows builds for now.
- **The signing pipeline is dormant.** It is in the repo, but the release workflow only signs when `vars.SIGNPATH_ORGANIZATION_ID` is set. `docs/SIGNING.md` explains how to switch it on, with SignPath or another signer.
- **Shipping regardless of signing:**
  - the branded NSIS wizard, with a license page and a clean-uninstall prompt (skipped during updates);
  - "Open with Aurora" file associations (`Aurora.Audio`; every audio extension except `.mp4`);
  - resuming playback after a **Restart now** update.

## Rulings

Decisions taken during execution, each with what it costs if wrong.

- Ruling: README code-signing policy + privacy policy + Install signing note committed early on main (c7c5cc6) and merged into win-signing (3d8bbb6) — the SignPath application's download/privacy URLs must resolve before the user submits — Task 8 now only extends the README (no duplicate sections) — cost if wrong: none
- Task 2: Ruling: apply-mode staging stages the UNSIGNED engine (after checking a signed copy exists) and lets the sign hook swap it — electron-builder runs the hook on resources/bin/**/aurora-engine.exe too, so staging the signed copy made the hook miss and throw with real signatures (fake-sign hid it) — cost if wrong: none (same bytes end up signed)
- Task 2: Ruling: CLI uses process.exitCode instead of process.exit(2) (libuv assertion crash on Node 24.11.1/Windows); AURORA_CI_ARCHES via limitArches() naming targets so arch flags apply — cost if wrong: none
- Task 2: review Approved; Ruling (regraded Important by effect): publisherName set only for apply-mode (signed) builds via -c.win.signtoolOptions.publisherName — with it in the yml, every build incl. unsigned releases (before SignPath approval, or if declined) would reject all non-SignPath updates and strand users — cost if wrong: unsigned builds keep M4's no-verification updater (status quo)
- Task 4: implementer done 00c17f2; Ruling: SignPath action pinned @v3 (current major; v1 in plan was stale), inputs confirmed against action.yml, output dir is extracted with names preserved — cost if wrong: none
- Task 4: review Needs fixes — Important: 60-min job timeout cancels release signing (2 human approvals); regraded Important by effect: test-signed builds carried publisherName (a published test-signed draft would reject all updates) → Ruling: publisher only for release-signed apply builds (--release-signed on push) — cost if wrong: test-signed builds don't verify update signatures (status quo); order test strengthened (plan-mandated weak test); overwrite: true on uploads
- Task 6: review Needs fixes. Rulings: (a) linux.mimeTypes instead of association mimeType — electron-builder writes a malformed /usr/share/mime/packages XML from a joined string — cost if wrong: none; (b) ProgID name Aurora.Audio not Audio — plan said Audio, but the uninstaller deletes the whole generic key (FileAssociation.nsh advises AppName.Type) — cost if wrong: macOS CFBundleTypeName text only; (c) associations = AUDIO_EXTENSIONS minus mp4 (usually video) — cost if wrong: double-clicking an audio-only .mp4 won't offer Aurora by default; (d) opener requeues on engine loss, onError → engine log, debounce with 2 s cap (Windows multi-select spreads second-instance events); (e) argv paths must be files (statSync isFile)
- Ruling: keep the SignPath pipeline in the repo but dormant (gated on vars.SIGNPATH_ORGANIZATION_ID; unset → unsigned exactly as M4) — ready for a later reapplication or paid plan — cost if wrong: unused workflow steps to maintain
- Ruling: Task 8 re-scoped to the unsigned reality — README must stop claiming SignPath signing and drop the SignPath attribution/code-signing-policy section (keep the privacy policy); SIGNING.md documents how to switch the dormant pipeline on later; RELEASING.md says releases are unsigned — cost if wrong: docs edit
- Task 8: implementer done 8e513a1; Ruling: WIN_CSC mentions left in historical plan files (records of earlier plans) — cost if wrong: none
- Final: Ruling: one fix wave = Important 1–3 + timeout 330 only when SignPath configured (unsigned path exactly as before) + clean dangling .ext defaults on uninstall (only where value == Aurora.Audio) — cost if wrong: small diff

## Deferred minors

None was judged a blocker by the final review.

- Task 1: minor (deferred): apply accepts a signed copy that is empty or identical to the input (Task 4's check-signatures.ps1 catches unsigned output); no test for a matched uninstaller swap; 'off' test doesn't assert no to-sign dir
- Task 2: minor (deferred): YML_TARGETS duplicates yml target lists (test-only knob); CLI rejections untested; bare-CN publisherName logs a DN warning in electron-updater; uninstaller in practice never signed (bytes differ every build); arm64 not proven locally
- Task 3: minor (deferred): check-signatures untested under pwsh 7 (CI uses it); publisher compare case-insensitive (-ne); finalize CLI/buildBlockMap integration untested automatically
- Task 4: minor (deferred): pe-file-set inside <zip-file> needs a SignPath dry run to confirm
- Task 5: minor (deferred): SetShellVarContext current not restored (harmless while per-user); electron-updater cache in %LOCALAPPDATA%\aurora-updater not removed (mention in docs); make-installer-art --check skips license.txt; bmp.ts lacks buffer-length guard
- Task 6: minor (deferred, final-review triage): isFile's statSync throws on EACCES/EPERM → uncaught in the top-level startupFiles initializer (crash at launch with an unreadable audio path in argv) — wrap in try/catch; onOpen inside try could requeue played files; flush not serialized (open during an in-flight setPaths replaces the queue); mime list count untested
- Task 7: minor (deferred): stale resume flag after a failed install resumes at an arbitrary later launch (bounded, once); macOS open-file before ready not counted as openingFiles; play() failure not logged; no test for last-status-wins / rejected play
- Task 8: fix round 1/5 (4 addressed, 0 open; commits 8e513a1..9b1067a); minor (deferred): checklist states the default-app precondition rather than an explicit step
- Final: minor (deferred): macOS open-file before ready not counted by resumer (post-update only); multi-select past the 2 s cap replaces instead of appends; dormant-pipeline minors (SignPath dry run will cover)
- Final: minor (deferred): `app/test/openFiles.test.ts` contains a literal NUL byte, so git treats it as binary. Replace it with `'bad\0name.mp3'`.
- Final: minor (deferred): `process.cwd()` at module top level in `index.ts` throws if the launch directory was deleted. Same class as the fixed `isFile` crash, but rare.

## Known limitation: installing takes over classic file-type defaults

electron-builder's `APP_ASSOCIATE` writes `HKCU\Software\Classes\.<ext>` (default) = `Aurora.Audio` at install.
- **Effect on install:** for users whose audio defaults are classic registry associations, rather than a Windows "UserChoice", installing Aurora makes Aurora the handler for those types.
- **Effect on uninstall:** it clears only the defaults that still point at Aurora. It cannot restore the previous default, because electron-builder keeps no backup.
- **Seen on the dev machine:** the test install replaced VLC's `.mp3`/`.flac`/… defaults.
- **Possible fix:** register Windows associations from `installer.nsh` instead (ProgID, `OpenWithProgids` and `Applications\Aurora.exe\SupportedTypes`, without touching the `.ext` default). Keep `fileAssociations` for macOS and Linux only.

## Pending human checks

1. **Manual installer checklist** in `docs/SIGNING.md`, on the first release:
   - the branding and license page;
   - per-user install with no admin prompt;
   - the uninstall prompt: **No** keeps the data, **Yes** deletes it;
   - double-click once Aurora is the default app;
   - an update with music playing resumes.
2. **Per-machine uninstall answering Yes:** check that the HKLM uninstall entry and associations are removed. The executor couldn't do an admin install.
3. **Linux "Open with" from Nautilus/Dolphin:** check that it plays the file (`%F`, `file://` decoding).
4. **If signing is switched on later:** run a SignPath dry run (`workflow_dispatch`, test-signing) to confirm the artifact configurations (`<zip-file><pe-file-set>`) and `check-signatures.ps1` under `pwsh` 7.
