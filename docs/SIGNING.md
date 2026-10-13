# Windows code signing (not enabled yet)

This page is for the maintainer. It says where Windows signing stands, how the pipeline in this repository works, and how to switch it on.

## Status

Windows releases are unsigned. SignPath Foundation's free open-source programme declined the first application (2026-10-13) because the project does not have enough public visibility yet.

The options are:

- reapply once the project has users, stars or articles;
- buy a SignPath subscription;
- use another signer (see "Using another signer").

Until then the release workflow builds unsigned Windows packages, exactly as it did before the pipeline existed.

## How the pipeline works

The pipeline is in the repository but dormant. The release workflow only uses it when the repository variable `SIGNPATH_ORGANIZATION_ID` is set.

**Two passes.** The packaging script runs twice around the signing step:

- `package-ci.ts --sign-mode collect` gathers every file electron-builder wants signed (through `build-tools/sign.cjs`), plus the engines, into `sign/to-sign/`. Each file is named by its content hash.
- SignPath signs those files.
- `package-ci.ts --sign-mode apply` builds again, and the sign hook swaps in the signed copies by hash. A missing copy (except the uninstaller) fails the build.

**Then the installer.** SignPath signs the installer. That changes its bytes, so `finalize-windows.ts` refreshes the sha512 and size in `latest.yml` and rewrites the blockmap. `check-signatures.ps1` verifies the result.

**Publisher.** Only release-signed builds (tag pushes, built with `--release-signed`) write `publisherName: SignPath Foundation` into `app-update.yml`. Installed copies of such a build then accept only updates signed by that publisher. A test-signed build never carries it.

**Uninstaller.** It is signed only if electron-builder regenerates byte-identical bytes, which it normally does not. So it normally stays unsigned, and the build warns about it.

## Switching it on with SignPath

In SignPath:

- create the project;
- add this repository as a trusted build system (the GitHub connector);
- create the artifact configurations `binaries` and `installer` from `.signpath/artifact-configurations/*.xml`;
- check that the signing policies `test-signing` and `release-signing` exist (release signing needs an approver);
- create a CI user API token.

In GitHub (Settings, Secrets and variables, Actions):

- add the repository variables `SIGNPATH_ORGANIZATION_ID` and `SIGNPATH_PROJECT_SLUG`;
- add the secret `SIGNPATH_API_TOKEN`.

**Dry run.** Go to Actions, Release, Run workflow. A by-hand run uses `test-signing`, needs no approval and gives a test certificate that SmartScreen does not trust.

**Each release.** Push the tag, then approve two requests in SignPath: the binaries, then the installer. The job waits up to 90 minutes for each.

**Only drafts from tag pushes are publishable.** A by-hand run is test-signed.

## Using another signer

The collect and apply mechanism is not tied to SignPath. It works with a paid certificate, a cloud HSM or Azure Trusted Signing.

1. In `.github/workflows/release.yml`, replace the two SignPath steps with steps that sign every file in `sign/to-sign/` into `sign/signed/`, keeping the file names, and that sign the installer. Keep the `vars.SIGNPATH_ORGANIZATION_ID` condition on those steps, or replace it with your own on/off switch, because the other signing steps in the workflow are conditioned on it.
2. Set `SIGNPATH_PUBLISHER` in `app/build-tools/package-ci.ts` to the new certificate's subject CN. Rename the constant if you like.
3. Update the publisher in the workflow's "Check the installer signature" step.

## Changing publisher breaks auto-update

Once a release-signed build is out, its users accept only updates signed by the publisher baked into it.

If you change signer later, every installed copy needs one manual reinstall. Plan for it, and say so in the release notes.

## Manual installer checklist

Run this for every release, signed or not:

- The wizard shows the Studio sidebar and header and the MIT license page.
- A per-user install needs no admin prompt.
- With Aurora set as the default app for `.mp3` and `.flac`, double-clicking one opens and plays it in Aurora. Open with Aurora works without that.
- An interactive uninstall asks the clean-up question. **No** keeps `%APPDATA%\Aurora`; **Yes** deletes it.
- An update installed with **Restart now** while music plays relaunches and resumes playback.
- `%LOCALAPPDATA%\aurora-updater`, the updater's download cache, is not removed by the clean uninstall. That is expected.

## Testing installers from Git Bash

Git Bash rewrites `/S` into a path. Set `MSYS_NO_PATHCONV=1` when you run `Aurora-*-win-setup.exe /S /D=...` or `"Uninstall Aurora.exe" /S`.

Never test against a real installed Aurora. Install into a separate folder.
