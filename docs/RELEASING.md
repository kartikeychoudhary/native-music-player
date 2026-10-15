# Releasing Aurora

A release is a `v*` tag. The Release workflow (`.github/workflows/release.yml`) builds the engine for six targets, packages the app for Windows, macOS and Linux, smoke-tests each package, and attaches everything to a **draft** GitHub release. You test the draft by hand, then publish it. Nothing reaches users until you publish, because the auto-updater only sees published releases.

## Before the first release

1. **Make the repository public, or fix Actions billing.** The workflow cannot run while the repository is private and Actions billing is blocked. Making the repository public also makes the arm64 runners (`windows-11-arm`, `ubuntu-24.04-arm`) free, and it lets the installed app read releases without a token, which auto-update needs. Until then, installed copies cannot see updates.
2. **Decide whether to keep the name "Aurora".** The spec treats renaming as cheap until M4, and after the first release it costs users their update path. If you change it, change these together: `productName` and `appId` in `app/electron-builder.yml`, `productName` in `app/package.json`, and `APP_ID` in `app/src/main/appIdentity.ts`. The Windows media flyout, the updater and the uninstall-entry check all key on them. Also update the executable path in the `package` job's matrix (`Aurora.exe`, `Aurora.app`) and the asset names in the README.

## Signing secrets (optional)

Windows releases are currently unsigned. Signing through SignPath is wired into the workflow but dormant; see `docs/SIGNING.md`.

Add these as repository secrets (Settings, Secrets and variables, Actions) to sign the macOS builds.

| Secret | Value |
| --- | --- |
| `MAC_CSC_LINK` | A base64-encoded `.p12` "Developer ID Application" certificate. |
| `MAC_CSC_KEY_PASSWORD` | The password of that `.p12`. |
| `APPLE_ID` | The Apple ID used for notarization. |
| `APPLE_APP_SPECIFIC_PASSWORD` | An app-specific password for that Apple ID. |
| `APPLE_TEAM_ID` | The Apple developer team ID. |

Without them the macOS builds are ad-hoc signed (the first launch needs **Open Anyway**), and the Windows builds are unsigned (SmartScreen warns on first run). Unsigned macOS builds cannot auto-install updates, so they fall back to notify mode: the app tells the user a new version exists and opens the release page. Windows installers update either way.

## Cutting a release

From a clean `main`:

1. Set the version everywhere it lives (app, engine crates, `Cargo.lock`):

   ```sh
   node app/build-tools/set-version.ts X.Y.Z
   ```

   Use `X.Y.Z-rc.1` style versions for prereleases. The workflow marks any version with a hyphen as a prerelease, and release builds do not offer prereleases to users.
2. Run the tests: `npm test`.
3. Commit the change as `chore: release vX.Y.Z`.
4. Tag and push:

   ```sh
   git tag vX.Y.Z && git push origin main vX.Y.Z
   ```

   The `version` job fails the run early if the tag, `app/package.json` and the engine version disagree.

   If SignPath is configured (`docs/SIGNING.md`), the Windows job pauses twice for you to approve a signing request in SignPath: the binaries first, then the installer. It waits up to 90 minutes for each.
5. Wait for the Release workflow to go green on all six engine targets and all three package jobs, then download the draft release's assets from the Releases page.

## Manual matrix, before publishing

Automation cannot cover audio hardware or other operating systems. Run the per-release matrix from the spec (section 7) with the draft's assets:

- a wired USB DAC on each OS (bit-perfect and exclusive output);
- a Bluetooth LDAC headset on Linux (the signal path shows the LDAC codec);
- AAC Bluetooth on macOS and Windows (the signal path says the codec is not reported).

And these checks that belong to packaging and updates:

- Install with the NSIS installer and play a track. The media flyout shows "Aurora" with its icon.
- Install the previous release, publish the new draft, and confirm the old app offers the update. It should install it on **Restart now** while music was playing, relaunch, and resume playing the queue.
- Check that the AppImage, deb and rpm start on Ubuntu 24.04 and on a current Fedora.
- Check that the unsigned macOS build opens after **Open Anyway**.
- Check that the no-fdk build plays FLAC and shows "Not in this build" in About.
- Run the "Pending human checks" from `docs/superpowers/m3-execution-notes.md`.

## Publishing

Edit the draft's release notes (the workflow fills in generated notes), then publish it. If SignPath is configured, only drafts from tag pushes are publishable: a by-hand run is test-signed, and its installers carry a certificate that SmartScreen does not trust. The updater sees only published releases, so the draft is the gate: a build that fails the matrix is deleted, not hidden. To test the updater with a prerelease, publish a `vX.Y.Z-rc.N` release as a prerelease. Installed release builds ignore it.

## Dry runs

To build and test everything without making a release, go to Actions, Release, Run workflow. Leave "draft" unticked to stop after the package jobs (the installers are kept as workflow artifacts). Tick "draft" to also get a draft release, which you can delete afterwards. A manual run on a branch does not need a tag, but the version job still checks that the app and engine versions agree.
