# Aurora — Windows Signing, Installer and Seamless Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:**
- Sign every Windows executable Aurora ships, through the free SignPath Foundation open-source programme.
- Turn the Windows installer into a polished, branded setup with file associations and a clean-uninstall option.
- Make updates seamless: signed, silent, differential, and resuming playback after the restart.

**Architecture:**
- **Why two passes.** SignPath signs files that a GitHub Actions run uploads as an artifact, after a human approves the request. electron-builder, though, has to sign `Aurora.exe` *after* it edits the exe's resources and *before* it builds the installer.
- **Pass 1, collect:** a custom electron-builder sign hook copies every file electron-builder wants signed into `sign/to-sign/`, named by content hash. The engine binaries are added too.
- **SignPath request 1** signs that folder.
- **Pass 2, apply:** the hook swaps each file for its signed copy by hash.
- **SignPath request 2** signs the finished `setup.exe`.
- **Finalize:** a tool rewrites `latest.yml`'s hashes and regenerates the blockmap, so electron-updater accepts the signed installer and can still download updates differentially.
- **Fallback:** without SignPath configured, nothing changes. The build stays unsigned, exactly as in M4.

**Tech Stack:**
- Unchanged: electron-builder 26.17.0, electron-updater 6.8.10, Node 24 build tools, GitHub Actions.
- New: the GitHub action `signpath/github-action-submit-signing-request` (pinned to a major version, `@v1` unless its README says otherwise).
- No new npm packages. `yaml` 2.9.1 is already a dev dependency.

**Spec:**
- `docs/superpowers/specs/2026-10-10-aurora-music-player-design.md` §9 M4: "Signing is optional and enabled when secrets are present."
- This plan supersedes the M4 Windows certificate path (`WIN_CSC_LINK`) with SignPath.
- Earlier rulings are in `docs/superpowers/m4-execution-notes.md`.

## Global Constraints

### Signing
- **SignPath settings (GitHub):**
  - repository **variables**: `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`;
  - repository **secret**: `SIGNPATH_API_TOKEN`.
- **Off switch:** signing runs only when `vars.SIGNPATH_ORGANIZATION_ID != ''`. Otherwise the Windows build is unsigned and behaves exactly as today.
- **Signing policy:** tag pushes use `release-signing`, which needs a human approval in SignPath. Manual (`workflow_dispatch`) runs use `test-signing`, which signs with SignPath's test certificate.
- **Artifact configuration slugs:** `binaries` and `installer`. Their XML lives in `.signpath/artifact-configurations/binaries.xml` and `installer.xml`, and the maintainer pastes it into SignPath.
- **Publisher:** the signing certificate's subject CN is `SignPath Foundation`. `win.signtoolOptions.publisherName: SignPath Foundation` is written into `app-update.yml`, so electron-updater refuses an update not signed by that publisher.
  - A release-signed installer whose signer CN differs **fails the release job**.
- **What gets signed:**
  - **Every app binary:** `Aurora.exe` for both archs; every `.dll` in the app folders; `aurora-engine.exe` for both archs × both variants.
  - **The installer** (`Aurora-<v>-win-setup.exe`).
  - **The uninstaller, best effort:** pass 2 can only reuse its signed copy if electron-builder regenerates it byte-identically. If not, it stays unsigned with a warning; it is never a build failure.

### Installer
- **Assisted NSIS installer**, kept per-user by default (`perMachine: false`):
  - per-user installs update silently without a UAC prompt;
  - per-machine is still offered, and its updates prompt for admin.
- **Branding:**
  - generated BMPs: a 164×314 sidebar and a 150×57 header, in the Studio palette (charcoal `#17151A`, the amber app mark);
  - an MIT license page, from `app/build/license.txt`, a committed copy of `LICENSE`.
- **Clean uninstall:** a real uninstall asks "Also delete your Aurora library…?". It never asks, and never deletes anything, during an update (`${isUpdated}`) or in silent mode (`/SD IDNO`).
- **File associations:**
  - every extension in `AUDIO_EXTENSIONS` (`app/src/shared/api.ts`): `mp3 m4a m4b mp4 aac flac wav aif aiff aifc ogg oga opus dsf dff`;
  - name `Audio`, role `Viewer`;
  - opening files replaces the queue and plays from the first file;
  - opening files while Aurora runs goes to the running instance.

### Seamless updates
- Updates download in the background (M4 behaviour) and install silently on **Restart now** or on quit.
- If music was playing when the user clicked **Restart now**, playback resumes after the relaunch, from the saved queue position. A plain quit never resumes. Launching with files to open never resumes.

### Unchanged
- Build tools are erasable TypeScript run by plain `node`, with relative imports carrying `.ts`. The electron-builder hooks are CommonJS (`.cjs`), because electron-builder `require`s them.
- New npm packages are pinned exactly (none are expected).
- The existing app and engine suites stay green after every task. The release workflow still can't be run by the executor: it is checked by its structure test.

### Dev machine (Windows, Git Bash)
- Prefix cargo with `export PATH="$PATH:/c/Users/karti/.cargo/bin"`.
- The user may have Aurora running, which locks `engine/target/debug/aurora-engine.exe`. Use `CARGO_TARGET_DIR=engine/target-verify` and delete it afterwards.
- Never play audible audio (`AURORA_OUTPUT=null`).

### Human steps (not code; documented in `docs/SIGNING.md`)
- Apply to SignPath Foundation.
- Once accepted, create the project, the two artifact configurations and the policies.
- Add the GitHub variables and secret.
- Approve each release's two signing requests.

## Review Focus

1. **The apply pass finds no signed copy.** This happens if `Aurora.exe` isn't byte-identical between passes, or a file was never collected. The build must **fail loudly**; it must never ship unsigned binaries silently.
   - Pinned by `sign.cjs` tests.
   - Proven end-to-end locally by Task 2's collect → fake-sign → apply run.
2. **A signed installer with a stale `latest.yml`.** That makes every user's update fail its sha512 check. Pinned by Task 3's `finalize-windows` tests and its workflow step.
3. **Wrong publisher.** If `publisherName` doesn't match the release certificate, every installed copy rejects every update. Pinned by Task 4's release-signing publisher check, which fails the release before anything is published.
4. **The clean-uninstall prompt during an update.** electron-updater runs the old uninstaller silently, so the prompt must never appear there, and nothing may be deleted. Pinned by the macro's `${isUpdated}` guard and `/SD IDNO`, plus a silent install/uninstall check in Task 5.
5. **Files opened before the engine is ready, or while Aurora is already running.** Neither may be dropped. Pinned by `createFileOpener` tests and the open-with e2e test.

## File Structure

| Path | Responsibility | Task |
|---|---|---|
| `app/build-tools/signCache.cjs`, `app/build-tools/sign.cjs` | Hash-named collect/apply cache; electron-builder sign hook | 1 |
| `app/electron-builder.yml` (win and nsis sections), `app/build-tools/package-ci.ts`, `app/build-tools/fake-sign.ts` | Wiring the two passes | 2 |
| `app/build-tools/finalize-windows.ts`, `app/build-tools/check-signatures.ps1` | latest.yml and blockmap after installer signing; signature verification | 3 |
| `.github/workflows/release.yml`, `.signpath/artifact-configurations/*.xml` | SignPath in CI | 4 |
| `app/build-tools/bmp.ts`, `app/build-tools/make-installer-art.ts`, `app/build/installer*.bmp`, `app/build/license.txt`, `app/build/installer.nsh` | Installer polish | 5 |
| `app/src/main/openFiles.ts`, `app/src/main/index.ts`, `app/e2e/open-with.spec.ts` | File associations | 6 |
| `app/src/main/resumeAfterUpdate.ts` | Resume playback after an update restart | 7 |
| `README.md`, `docs/SIGNING.md`, `docs/RELEASING.md` | Code-signing policy, onboarding, release steps | 8 |

---

### Task 1: Signing cache and electron-builder sign hook

**Files:**
- Create: `app/build-tools/signCache.cjs`, `app/build-tools/sign.cjs`
- Test: `app/test/sign.test.ts`

**Interfaces:**
- `signCache.cjs` exports:
  - `cacheName(file: string): string` returns `<first 16 hex chars of sha256(bytes)>-<basename>`;
  - `collect(file, dir): string` copies the file to `dir/cacheName(file)` and returns that name;
  - `signedCopy(file, dir): string | null`;
  - `kindOf(file): 'installer' | 'uninstaller' | 'binary'`.
- `sign.cjs`:
  - exports `sign(config: { path: string })` as `module.exports` and `.default`;
  - reads `AURORA_SIGN_MODE` (`off` | `collect` | `apply`; unset means off) and `AURORA_SIGN_DIR`;
  - uses `<dir>/to-sign` and `<dir>/signed`.

- [ ] **Step 1: Write the failing tests** in `app/test/sign.test.ts`:

```ts
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'

const req = createRequire(import.meta.url)
const cache = req('../build-tools/signCache.cjs') as { cacheName: (f: string) => string; collect: (f: string, d: string) => string; signedCopy: (f: string, d: string) => string | null; kindOf: (f: string) => string }
const sign = req('../build-tools/sign.cjs') as (c: { path: string }) => Promise<void>

const tmp = () => mkdtempSync(join(tmpdir(), 'aurora-sign-'))
const file = (dir: string, name: string, body: string) => { const p = join(dir, name); writeFileSync(p, body); return p }

describe('sign cache', () => {
  it('names a file by its content hash and base name', () => {
    const d = tmp()
    const a = file(d, 'Aurora.exe', 'one'), b = file(join(d), 'other.exe', 'one')
    expect(cache.cacheName(a)).toMatch(/^[0-9a-f]{16}-Aurora\.exe$/)
    expect(cache.cacheName(a).slice(0, 16)).toBe(cache.cacheName(b).slice(0, 16))
    writeFileSync(a, 'two')
    expect(cache.cacheName(a).slice(0, 16)).not.toBe(cache.cacheName(b).slice(0, 16))
  })
  it('finds the signed copy of exactly the collected bytes', () => {
    const d = tmp(), f = file(d, 'ffmpeg.dll', 'dll bytes')
    const name = cache.collect(f, join(d, 'to-sign'))
    expect(readdirSync(join(d, 'to-sign'))).toEqual([name])
    mkdirSync(join(d, 'signed'))
    writeFileSync(join(d, 'signed', name), 'signed dll bytes')
    expect(readFileSync(cache.signedCopy(f, join(d, 'signed'))!, 'utf8')).toBe('signed dll bytes')
    writeFileSync(f, 'changed')
    expect(cache.signedCopy(f, join(d, 'signed'))).toBeNull()
  })
  it('tells the installer and uninstaller from app binaries', () => {
    expect(cache.kindOf('C:\\d\\Aurora-0.1.0-win-setup.exe')).toBe('installer')
    expect(cache.kindOf('/d/Aurora-0.1.0-win-setup.__uninstaller.exe')).toBe('uninstaller')
    expect(cache.kindOf('/d/win-unpacked/Aurora.exe')).toBe('binary')
    expect(cache.kindOf('/d/win-unpacked/vk_swiftshader.dll')).toBe('binary')
  })
})

describe('sign hook', () => {
  afterEach(() => { delete process.env.AURORA_SIGN_MODE; delete process.env.AURORA_SIGN_DIR; vi.restoreAllMocks() })

  it('does nothing when signing is off', async () => {
    const d = tmp(), f = file(d, 'Aurora.exe', 'x')
    await sign({ path: f })
    process.env.AURORA_SIGN_MODE = 'off'
    await sign({ path: f })
    expect(readFileSync(f, 'utf8')).toBe('x')
  })
  it('collects binaries and the uninstaller but never the installer', async () => {
    const d = tmp()
    process.env.AURORA_SIGN_MODE = 'collect'; process.env.AURORA_SIGN_DIR = d
    await sign({ path: file(d, 'Aurora.exe', 'exe') })
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.__uninstaller.exe', 'un') })
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.exe', 'setup') })
    expect(readdirSync(join(d, 'to-sign')).map((n) => n.slice(17)).sort()).toEqual(['Aurora-0.1.0-win-setup.__uninstaller.exe', 'Aurora.exe'])
  })
  it('swaps in the signed copy, and fails loudly for a binary that was never signed', async () => {
    const d = tmp(), f = file(d, 'Aurora.exe', 'exe')
    mkdirSync(join(d, 'signed'))
    writeFileSync(join(d, 'signed', cache.cacheName(f)), 'signed exe')
    process.env.AURORA_SIGN_MODE = 'apply'; process.env.AURORA_SIGN_DIR = d
    await sign({ path: f })
    expect(readFileSync(f, 'utf8')).toBe('signed exe')
    await expect(sign({ path: file(d, 'libEGL.dll', 'never collected') })).rejects.toThrow(/no signed copy of .*libEGL\.dll/)
  })
  it('leaves an unmatched uninstaller unsigned with a warning, and skips the installer', async () => {
    const d = tmp(); mkdirSync(join(d, 'signed'))
    process.env.AURORA_SIGN_MODE = 'apply'; process.env.AURORA_SIGN_DIR = d
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.__uninstaller.exe', 'regenerated') })
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.exe', 'setup') })
    expect(warn).toHaveBeenCalledTimes(1)
  })
  it('refuses to run without a sign dir or with an unknown mode', async () => {
    const d = tmp(), f = file(d, 'Aurora.exe', 'x')
    process.env.AURORA_SIGN_MODE = 'collect'
    await expect(sign({ path: f })).rejects.toThrow(/AURORA_SIGN_DIR/)
    process.env.AURORA_SIGN_DIR = d; process.env.AURORA_SIGN_MODE = 'later'
    await expect(sign({ path: f })).rejects.toThrow(/unknown AURORA_SIGN_MODE/)
  })
})
```

- [ ] **Step 2: Run the tests and check they fail**

Run: `npm --prefix app test -- sign`
Expected: FAIL, `Cannot find module '../build-tools/signCache.cjs'`.

- [ ] **Step 3: Implement.** First `app/build-tools/signCache.cjs`:

```js
// Where the two-pass Windows signing (docs/SIGNING.md) keeps files: each file electron-builder wants
// signed is collected under a name derived from its unsigned bytes, and its signed copy is found again
// by the same name. A file that changed between the passes simply has no signed copy.
const { createHash } = require('node:crypto')
const { copyFileSync, existsSync, mkdirSync, readFileSync } = require('node:fs')
const { basename, join } = require('node:path')

function cacheName(file) {
  return `${createHash('sha256').update(readFileSync(file)).digest('hex').slice(0, 16)}-${basename(file)}`
}

function collect(file, dir) {
  mkdirSync(dir, { recursive: true })
  const name = cacheName(file)
  copyFileSync(file, join(dir, name))
  return name
}

function signedCopy(file, dir) {
  const p = join(dir, cacheName(file))
  return existsSync(p) ? p : null
}

/** The installer is signed after it is built; the uninstaller is signed when its bytes repeat; everything else must be signed. */
function kindOf(file) {
  const name = basename(file.replace(/\\/g, '/')).toLowerCase()
  if (name.endsWith('__uninstaller.exe')) return 'uninstaller'
  if (/-win-setup\.exe$/.test(name)) return 'installer'
  return 'binary'
}

module.exports = { cacheName, collect, signedCopy, kindOf }
```

Then `app/build-tools/sign.cjs`:

```js
// electron-builder custom sign hook (win.signtoolOptions.sign). Aurora's Windows binaries are signed by
// SignPath in CI in two passes (docs/SIGNING.md): a `collect` build gathers every file electron-builder
// wants signed, SignPath signs them, and an `apply` build swaps the signed copies in by content hash.
// The installer is signed after it is built, so it is skipped here. Without AURORA_SIGN_MODE nothing happens.
const { copyFileSync } = require('node:fs')
const { join } = require('node:path')
const { collect, kindOf, signedCopy } = require('./signCache.cjs')

async function sign(config) {
  const mode = process.env.AURORA_SIGN_MODE
  const file = config.path
  if (!mode || mode === 'off') return
  const kind = kindOf(file)
  if (kind === 'installer') return
  const dir = process.env.AURORA_SIGN_DIR
  if (!dir) throw new Error('aurora-sign: AURORA_SIGN_DIR is not set')
  if (mode === 'collect') { collect(file, join(dir, 'to-sign')); return }
  if (mode !== 'apply') throw new Error(`aurora-sign: unknown AURORA_SIGN_MODE ${mode}`)
  const signed = signedCopy(file, join(dir, 'signed'))
  if (signed) { copyFileSync(signed, file); return }
  if (kind === 'uninstaller') { console.warn(`aurora-sign: no signed copy of ${file}; the uninstaller stays unsigned`); return }
  throw new Error(`aurora-sign: no signed copy of ${file} (collect pass and apply pass built different bytes, or it was never collected)`)
}

module.exports = sign
module.exports.default = sign
```

- [ ] **Step 4: Run the tests**

Run: `npm --prefix app test -- sign && npm --prefix app run typecheck`
Expected: 8 passed; typecheck clean.

- [ ] **Step 5: Commit**

```bash
git add app/build-tools/signCache.cjs app/build-tools/sign.cjs app/test/sign.test.ts
git commit -m "build: hash-keyed collect/apply sign hook for two-pass Windows signing"
```

---

### Task 2: Wire the two passes into packaging

**Files:**
- Create: `app/build-tools/fake-sign.ts`
- Modify:
  - `app/electron-builder.yml` (the `win:` section);
  - `app/build-tools/package-ci.ts`;
  - `app/test/packageCi.test.ts`.

**Interfaces:**
- **Consumes:** Task 1's `signCache.cjs` and `sign.cjs`.
- **Produces:**
  - `package-ci.ts` accepts `--sign-mode collect|apply` and `--sign-dir <dir>` (Windows only; other platforms reject the flags with exit 2).
  - `builderArgs(o)` gains `signMode?: 'collect' | 'apply'`. In `collect` mode the result is `['--win', 'nsis', '--x64', '--arm64', '--publish', 'never']`; the no-fdk variant and the zip are skipped.
  - `node app/build-tools/fake-sign.ts <signDir>` copies `to-sign/*` into `signed/` unchanged. It is a local and test stand-in for SignPath that proves the mechanics, not real signatures.

- [ ] **Step 1: Write the failing tests.** Add to `app/test/packageCi.test.ts`:

```ts
  it('the collect pass builds only the standard NSIS installer (both arches), to gather every file to sign', () => {
    expect(builderArgs({ platform: 'win32', variant: 'standard', signed: false, signMode: 'collect' })).toEqual(['--win', 'nsis', '--x64', '--arm64', '--publish', 'never'])
  })
  it('the apply pass builds exactly what an unsigned run builds', () => {
    for (const variant of ['standard', 'nofdk'] as const) {
      expect(builderArgs({ platform: 'win32', variant, signed: false, signMode: 'apply' })).toEqual(builderArgs({ platform: 'win32', variant, signed: false }))
    }
  })
```

Add to `app/test/installKind.test.ts`, next to the yml match test:

```ts
it('signs through the collect/apply hook as SignPath Foundation, sha256 only, exe and dll', () => {
  const yml = readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')
  expect(yml).toMatch(/signtoolOptions:\s+sign: build-tools\/sign\.cjs\s+publisherName: SignPath Foundation\s+signingHashAlgorithms: \[sha256\]/)
  expect(yml).toMatch(/signExts: \[\.exe, \.dll\]/)
})
```

Run: `npm --prefix app test -- packageCi installKind`
Expected: FAIL. `signMode` is ignored and the yml lacks `signtoolOptions`.

- [ ] **Step 2: Configure electron-builder.** In `app/electron-builder.yml`, under `win:`, after `target:`, add:

```yaml
  # Signing is done by SignPath in CI in two passes (docs/SIGNING.md); build-tools/sign.cjs collects or
  # applies signed copies and does nothing unless AURORA_SIGN_MODE is set. publisherName goes into
  # app-update.yml: electron-updater rejects any update not signed by this publisher.
  signtoolOptions:
    sign: build-tools/sign.cjs
    publisherName: SignPath Foundation
    signingHashAlgorithms: [sha256]
  signExts: [.exe, .dll]
```

- [ ] **Step 3: Extend `package-ci.ts`.**
  - **`builderArgs`:** add the optional `signMode`. `collect` returns the arg list from the test above; `apply` and undefined keep today's arguments.
  - **CLI flags:** parse `--sign-mode` and `--sign-dir` (resolved to absolute paths). Reject them on non-Windows platforms with exit 2.
  - **In `collect` mode:**
    1. Run `electron-vite build`.
    2. Stage the **standard** engines for both arches.
    3. Collect all four engines (`engines/<arch>/<variant>/aurora-engine.exe`) into `<signDir>/to-sign` with `collect` (`createRequire(import.meta.url)('./signCache.cjs')`).
    4. Write licenses and build-info for standard.
    5. Run electron-builder with the collect args and `AURORA_SIGN_MODE=collect` and `AURORA_SIGN_DIR=<signDir>` in its environment (pass `env: { ...process.env, … }` to that `execFileSync`).
    6. Print `to-sign: <n> files`.
  - **In `apply` mode:**
    - Run exactly today's flow for both variants, with `AURORA_SIGN_MODE=apply`.
    - Staging takes each engine from `signedCopy(src, <signDir>/signed)` instead of `src`. If any engine has no signed copy, throw: `no signed copy of engines/<arch>/<variant>/aurora-engine.exe`.
  - **Without `--sign-mode`:** behaviour is byte-for-byte today's. Existing tests still pass.

- [ ] **Step 4: Write `fake-sign.ts`:**

```ts
// Local stand-in for SignPath: "signs" sign/to-sign by copying each file to sign/signed unchanged.
// It proves the collect -> apply mechanics (every file found again by hash); it adds no signatures.
import { copyFileSync, mkdirSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'

const dir = resolve(process.argv[2] ?? 'sign')
mkdirSync(join(dir, 'signed'), { recursive: true })
const names = readdirSync(join(dir, 'to-sign'))
for (const n of names) copyFileSync(join(dir, 'to-sign', n), join(dir, 'signed', n))
console.log(`fake-signed ${names.length} files into ${join(dir, 'signed')}`)
```

- [ ] **Step 5: Run the tests**

Run: `npm --prefix app test -- packageCi installKind && npm --prefix app run typecheck`
Expected: pass.

- [ ] **Step 6: Prove both passes end to end on this machine, for x64 only.**
  - `package-ci.ts` needs engines in `engines/<arch>/<variant>/`. Build a release engine, both standard and `--no-default-features`, into separate target dirs, and copy them into `engines/x64/standard/` and `engines/x64/nofdk/`.
  - For the local run, temporarily restrict to x64 by passing an environment override such as `AURORA_CI_ARCHES=x64`. Add that to `package-ci.ts` as a documented test-only knob; it defaults to `x64,arm64`.
  - Then run:

```bash
export PATH="$PATH:/c/Users/karti/.cargo/bin"
rm -rf sign && AURORA_CI_ARCHES=x64 node app/build-tools/package-ci.ts --platform win32 --sign-mode collect --sign-dir sign
node app/build-tools/fake-sign.ts sign
AURORA_CI_ARCHES=x64 node app/build-tools/package-ci.ts --platform win32 --sign-mode apply --sign-dir sign
```

Expected:
- **Collect:** prints `to-sign: N files`. N should be at least 2 engines plus `Aurora.exe` plus the Electron DLLs (`ffmpeg.dll`, `libEGL.dll`, `libGLESv2.dll`, `vk_swiftshader.dll`, `d3dcompiler_47.dll`, …) plus the uninstaller.
- **Apply:** completes with **no** `no signed copy` error. This proves `Aurora.exe` and the DLLs are byte-identical between passes.
- **Uninstaller:** either matched, or warned. Record which; it decides whether the uninstaller gets signed in CI.

Then run the packaged smoke test against `app/dist/win-unpacked/Aurora.exe` (`AURORA_PACKAGED_EXE=dist/win-unpacked/Aurora.exe npm --prefix app run e2e:packaged`).
Expected: 1 passed.

If **apply fails** because `Aurora.exe` differs between passes, stop and report BLOCKED with the two hashes. The design depends on this determinism.

Clean up `sign/` and `engines/` afterwards, and add both to `.gitignore`.

- [ ] **Step 7: Commit**

```bash
git add app/electron-builder.yml app/build-tools/package-ci.ts app/build-tools/fake-sign.ts app/test/packageCi.test.ts app/test/installKind.test.ts .gitignore
git commit -m "build: collect and apply passes for SignPath signing of the Windows app"
```

---

### Task 3: Finalize the signed installer (latest.yml, blockmap) and verify signatures

**Files:**
- Create: `app/build-tools/finalize-windows.ts`, `app/build-tools/check-signatures.ps1`
- Test: `app/test/finalizeWindows.test.ts`

**Interfaces:**
- `updateLatestYml(text: string, infos: Record<string, { sha512: string; size: number; blockMapSize?: number }>): string`:
  - rewrites each `files[]` entry whose `url` is a key of `infos`;
  - rewrites the top-level `sha512`, and `blockMapSize` if that key existed, for the entry whose `url` equals the top-level `path`;
  - leaves every other key and its order unchanged;
  - throws if a key in `infos` is not in `files`.
- CLI `node app/build-tools/finalize-windows.ts --dist app/dist`:
  - for every `files[].url` that exists in `dist/`, computes sha512 (base64) and size;
  - if `<file>.blockmap` exists, rebuilds it with electron-builder's own `buildBlockMap(file, 'gzip', file + '.blockmap')` from `app-builder-lib/out/targets/blockmap/blockmap`;
  - rewrites `latest.yml`;
  - prints each file's old and new sha512 prefix.
- `check-signatures.ps1 -Path <file-or-dir>[,...] [-Publisher 'SignPath Foundation']`:
  - checks every `.exe` and `.dll` under the paths, recursively;
  - fails (exit 1, listing the offenders) when any is unsigned (`Get-AuthenticodeSignature` status `NotSigned`, or no `SignerCertificate`);
  - with `-Publisher`, also fails when the signer's CN differs (parse `SignerCertificate.Subject`, take the `CN=` value);
  - test-signed files have status `UnknownError`/`NotTrusted` and still count as signed.

- [ ] **Step 1: Write the failing tests** in `app/test/finalizeWindows.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'
import { updateLatestYml } from '../build-tools/finalize-windows.ts'

const latest = `version: 0.1.0
files:
  - url: Aurora-0.1.0-win-setup.exe
    sha512: OLDSHA==
    size: 100
path: Aurora-0.1.0-win-setup.exe
sha512: OLDSHA==
releaseDate: '2026-10-13T08:00:00.000Z'
`

describe('finalize windows', () => {
  it('rewrites the signed installer hash and size everywhere it appears, keeping the rest', () => {
    const out = updateLatestYml(latest, { 'Aurora-0.1.0-win-setup.exe': { sha512: 'NEWSHA==', size: 123 } })
    const y = parse(out)
    expect(y.files[0]).toEqual({ url: 'Aurora-0.1.0-win-setup.exe', sha512: 'NEWSHA==', size: 123 })
    expect(y.sha512).toBe('NEWSHA==')
    expect(y.version).toBe('0.1.0')
    expect(y.releaseDate).toBe('2026-10-13T08:00:00.000Z')
    expect(out.indexOf('version')).toBeLessThan(out.indexOf('files'))
  })
  it('updates blockMapSize only when the entry had one', () => {
    const withMap = latest.replace('    size: 100\n', '    size: 100\n    blockMapSize: 7\n')
    expect(parse(updateLatestYml(withMap, { 'Aurora-0.1.0-win-setup.exe': { sha512: 'N==', size: 1, blockMapSize: 9 } })).files[0].blockMapSize).toBe(9)
    expect(parse(updateLatestYml(latest, { 'Aurora-0.1.0-win-setup.exe': { sha512: 'N==', size: 1, blockMapSize: 9 } })).files[0]).not.toHaveProperty('blockMapSize')
  })
  it('refuses a file latest.yml does not list', () => {
    expect(() => updateLatestYml(latest, { 'Other.exe': { sha512: 'N==', size: 1 } })).toThrow(/Other\.exe/)
  })
})
```

Run: `npm --prefix app test -- finalizeWindows`
Expected: FAIL, the module is missing.

- [ ] **Step 2: Implement `finalize-windows.ts`.**
  - Use `parseDocument` from `yaml` and edit nodes in place, so key order and formatting are preserved.
  - Put the CLI behind the `import.meta.url` guard.
  - Load `buildBlockMap` with `createRequire(import.meta.url)('app-builder-lib/out/targets/blockmap/blockmap')`. Check the export name in `node_modules`; it is `buildBlockMap(file, compression, outFile)` and returns `{ size, sha512, blockMapSize? }`.

- [ ] **Step 3: Write `check-signatures.ps1`.** A `param()` block, `Get-ChildItem -Recurse -Include *.exe,*.dll` for directories, the checks described above, a summary line, and `exit 1` on any offender.

- [ ] **Step 4: Test it**

Run: `npm --prefix app test -- finalizeWindows && npm --prefix app run typecheck`
Expected: 3 passed; typecheck clean.

Run: `powershell -NoProfile -File app/build-tools/check-signatures.ps1 -Path app/dist/win-unpacked; echo "exit $?"`
Expected: exit 1. It lists `Aurora.exe` etc. as unsigned, because Task 2's local run used fake signing.

Run it against `C:\Windows\System32\notepad.exe` with `-Publisher 'Microsoft Windows'`.
Expected: exit 0.

Then run it against notepad with `-Publisher 'SignPath Foundation'`.
Expected: exit 1 (publisher mismatch).

Run: `node app/build-tools/finalize-windows.ts --dist app/dist`. That dist is Task 2's apply build, if it is still present; otherwise run `npm run package` first.
Expected: the sha512 values match the files as they are, because nothing changed, and the blockmap is rebuilt. `git diff` shows nothing tracked.

- [ ] **Step 5: Commit**

```bash
git add app/build-tools/finalize-windows.ts app/build-tools/check-signatures.ps1 app/test/finalizeWindows.test.ts
git commit -m "build: refresh latest.yml and the blockmap after signing; signature check script"
```

---

### Task 4: SignPath in the release workflow

**Files:**
- Create: `.signpath/artifact-configurations/binaries.xml`, `.signpath/artifact-configurations/installer.xml`
- Modify: `.github/workflows/release.yml`, `app/test/releaseWorkflow.test.ts`

**Interfaces:**
- **Consumes:**
  - Task 2's `--sign-mode` and `--sign-dir`;
  - Task 3's `finalize-windows.ts` and `check-signatures.ps1`.
- **Produces:** the Windows package-job steps described below, all gated by `matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''`. The existing unsigned "Package" step gets the negated condition for Windows, so exactly one packaging path runs.

- [ ] **Step 1: Check the action's current interface.** Fetch the README of https://github.com/SignPath/github-action-submit-signing-request with WebFetch. Confirm:
  - the current major tag;
  - the input names: `api-token`, `organization-id`, `project-slug`, `signing-policy-slug`, `artifact-configuration-slug`, `github-artifact-id`, `wait-for-completion`, `wait-for-completion-timeout-in-seconds`, `output-artifact-directory`.

  Also fetch SignPath's artifact-configuration documentation (https://about.signpath.io/documentation/artifact-configuration) and confirm the XML syntax below. If either differs, follow the docs and record the difference.

- [ ] **Step 2: Write the failing workflow tests.** Add to `releaseWorkflow.test.ts`:

```ts
  it('signs Windows through SignPath only when configured, with the binaries then the installer', () => {
    const steps = wf.jobs.package.steps
    const signpath = steps.filter((s) => s.uses?.startsWith('signpath/github-action-submit-signing-request@'))
    expect(signpath.map((s) => s.with!['artifact-configuration-slug'])).toEqual(['binaries', 'installer'])
    for (const s of signpath) {
      expect((s as { if?: string }).if).toContain("vars.SIGNPATH_ORGANIZATION_ID != ''")
      expect(s.with!['signing-policy-slug']).toBe("${{ github.event_name == 'push' && 'release-signing' || 'test-signing' }}")
      expect(s.with!['api-token']).toBe('${{ secrets.SIGNPATH_API_TOKEN }}')
      expect(s.with!['wait-for-completion']).toBe(true)
    }
    const unsigned = steps.find((s) => s.name === 'Package')! as { if?: string }
    expect(unsigned.if).toContain("vars.SIGNPATH_ORGANIZATION_ID == ''")
  })
  it('finalizes latest.yml after the installer is signed and checks the publisher on release builds', () => {
    const runs = wf.jobs.package.steps.map((s) => s.run ?? '').join('\n')
    expect(runs).toContain('finalize-windows.ts')
    expect(runs).toMatch(/check-signatures\.ps1[^\n]*-Publisher 'SignPath Foundation'/)
    const order = (needle: string) => wf.jobs.package.steps.findIndex((s) => (s.run ?? '').includes(needle) || JSON.stringify(s.with ?? {}).includes(needle))
    expect(order('--sign-mode collect')).toBeLessThan(order('binaries'))
    expect(order('binaries')).toBeLessThan(order('--sign-mode apply'))
    expect(order('--sign-mode apply')).toBeLessThan(order('"installer"'))
    expect(order('"installer"')).toBeLessThan(order('finalize-windows.ts'))
  })
  it('keeps the artifact configurations next to the workflow', () => {
    for (const slug of ['binaries', 'installer']) expect(existsSync(join(__dirname, `../../.signpath/artifact-configurations/${slug}.xml`))).toBe(true)
  })
```

Add `existsSync` to the test's imports.

Run: `npm --prefix app test -- releaseWorkflow`
Expected: the 3 new tests FAIL.

- [ ] **Step 3: Write the artifact configurations.** First `binaries.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<!-- SignPath artifact configuration "binaries": the GitHub artifact is a zip of sign/to-sign/, every
     file named <hash>-<name>. Sign every executable and DLL in it (Aurora.exe, Electron's DLLs,
     aurora-engine.exe, the uninstaller). See docs/SIGNING.md. -->
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file-set>
      <include path="*.exe" min-matches="1" max-matches="unbounded" />
      <include path="*.dll" min-matches="1" max-matches="unbounded" />
      <for-each>
        <authenticode-sign />
      </for-each>
    </pe-file-set>
  </zip-file>
</artifact-configuration>
```

Then `installer.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<!-- SignPath artifact configuration "installer": the GitHub artifact is a zip holding the one NSIS
     installer Aurora-<version>-win-setup.exe. See docs/SIGNING.md. -->
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file-set>
      <include path="*-win-setup.exe" min-matches="1" max-matches="1" />
      <for-each>
        <authenticode-sign />
      </for-each>
    </pe-file-set>
  </zip-file>
</artifact-configuration>
```

- [ ] **Step 4: Edit `release.yml`.**
  - **Existing `Package` step:** add `if: matrix.os != 'windows' || vars.SIGNPATH_ORGANIZATION_ID == ''`.
  - **New steps**, after it and before the smoke steps, all with `if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''`:

```yaml
      - name: Collect Windows binaries for signing
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        run: node app/build-tools/package-ci.ts --platform win32 --sign-mode collect --sign-dir sign
      - id: unsigned-binaries
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        uses: actions/upload-artifact@v4
        with: { name: windows-binaries-unsigned, path: sign/to-sign/, if-no-files-found: error }
      - name: Sign the binaries (SignPath)
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        uses: signpath/github-action-submit-signing-request@v1
        with:
          api-token: '${{ secrets.SIGNPATH_API_TOKEN }}'
          organization-id: '${{ vars.SIGNPATH_ORGANIZATION_ID }}'
          project-slug: '${{ vars.SIGNPATH_PROJECT_SLUG }}'
          signing-policy-slug: "${{ github.event_name == 'push' && 'release-signing' || 'test-signing' }}"
          artifact-configuration-slug: binaries
          github-artifact-id: '${{ steps.unsigned-binaries.outputs.artifact-id }}'
          wait-for-completion: true
          wait-for-completion-timeout-in-seconds: 7200 # release signing waits for a human approval
          output-artifact-directory: sign/signed
      - name: Package with the signed binaries
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        run: node app/build-tools/package-ci.ts --platform win32 --sign-mode apply --sign-dir sign
      - name: Check every app binary is signed
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        shell: pwsh
        run: app/build-tools/check-signatures.ps1 -Path app/dist/win-unpacked,app/dist/win-arm64-unpacked,app/dist-nofdk/win-unpacked,app/dist-nofdk/win-arm64-unpacked
      - id: unsigned-installer
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        uses: actions/upload-artifact@v4
        with: { name: windows-installer-unsigned, path: 'app/dist/*-win-setup.exe', if-no-files-found: error }
      - name: Sign the installer (SignPath)
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        uses: signpath/github-action-submit-signing-request@v1
        with:
          # same inputs as above, with:
          artifact-configuration-slug: installer
          github-artifact-id: '${{ steps.unsigned-installer.outputs.artifact-id }}'
          output-artifact-directory: sign/installer
      - name: Use the signed installer and refresh latest.yml
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        run: |
          cp sign/installer/*-win-setup.exe app/dist/
          node app/build-tools/finalize-windows.ts --dist app/dist
      - name: Check the installer signature
        if: matrix.os == 'windows' && vars.SIGNPATH_ORGANIZATION_ID != ''
        shell: pwsh
        run: |
          $files = Get-ChildItem app/dist/*-win-setup.exe | ForEach-Object FullName
          if ('${{ github.event_name }}' -eq 'push') { app/build-tools/check-signatures.ps1 -Path $files -Publisher 'SignPath Foundation' }
          else { app/build-tools/check-signatures.ps1 -Path $files }
```

  Write the second SignPath step with all its inputs in full. The comment above only marks which inputs differ.
  - **Signing secrets:** remove `WIN_CSC_LINK` and `WIN_CSC_KEY_PASSWORD` from the Package step's `CSC_LINK` / `CSC_KEY_PASSWORD` expressions. Windows signing is SignPath only now; macOS keeps its secrets.
  - **Smoke tests:** the existing ones run after these steps and use the signed app.

- [ ] **Step 5: Run the tests**

Run: `npm --prefix app test -- releaseWorkflow && npm --prefix app test && npm --prefix app run typecheck`
Expected: all pass.

The workflow itself runs for the first time in CI, after the human onboarding in `docs/SIGNING.md`. Say so in the report.

- [ ] **Step 6: Commit**

```bash
git add .signpath .github/workflows/release.yml app/test/releaseWorkflow.test.ts
git commit -m "ci: sign Windows binaries and installer through SignPath; refresh latest.yml after signing"
```

---

### Task 5: Installer polish (branding, license page, clean-uninstall option)

**Files:**
- Create:
  - `app/build-tools/bmp.ts`
  - `app/build-tools/make-installer-art.ts`
  - `app/build/installerSidebar.bmp`, `app/build/installerHeader.bmp` (generated, committed)
  - `app/build/license.txt` (a copy of `LICENSE`)
  - `app/build/installer.nsh`
- Modify: `app/electron-builder.yml` (the `nsis:` section)
- Test: `app/test/installerArt.test.ts`

**Interfaces:**
- `encodeBmp24(width: number, height: number, bgra: Uint8Array, background: [number, number, number]): Buffer`:
  - writes a 24-bit bottom-up BMP with rows padded to 4 bytes;
  - alpha-composites each pixel over `background` (RGB);
  - outputs a 54-byte `BITMAPFILEHEADER` + `BITMAPINFOHEADER`.
- `make-installer-art.ts` exports `sidebarBmp(): Buffer` and `headerBmp(): Buffer`. Its CLI writes both files, and `--check` fails if either is stale (same pattern as `make-icon.ts`).
  - **Sidebar:** 164×314 on charcoal `#17151A`. The app mark is `iconPixels(96)` from `src/shared/iconPixels.ts`, centred horizontally with its top at y=48.
  - **Header:** 150×57 on white (the NSIS header bar is white). The app mark is `iconPixels(41)` at x=101, y=8.

- [ ] **Step 1: Write the failing tests** in `app/test/installerArt.test.ts`:

```ts
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { encodeBmp24 } from '../build-tools/bmp.ts'
import { headerBmp, sidebarBmp } from '../build-tools/make-installer-art.ts'

/** RGB of pixel (x, y) (top-left origin) in a 24-bit bottom-up BMP. */
function rgb(bmp: Buffer, x: number, y: number): number[] {
  const w = bmp.readInt32LE(18), h = bmp.readInt32LE(22), stride = Math.ceil((w * 3) / 4) * 4
  const at = bmp.readUInt32LE(10) + (h - 1 - y) * stride + x * 3
  return [bmp[at + 2], bmp[at + 1], bmp[at]]
}

describe('installer art', () => {
  it('encodes a padded 24-bit bottom-up BMP, compositing alpha over the background', () => {
    const px = new Uint8Array([0x47, 0xb5, 0xff, 0xff, 0, 0, 0, 0, 0x47, 0xb5, 0xff, 0x80]) // amber, transparent, half amber (BGRA)
    const bmp = encodeBmp24(3, 1, px, [0x17, 0x15, 0x1a])
    expect(bmp.toString('ascii', 0, 2)).toBe('BM')
    expect([bmp.readInt32LE(18), bmp.readInt32LE(22), bmp.readUInt16LE(28)]).toEqual([3, 1, 24])
    expect(bmp.length).toBe(54 + 12) // 9 bytes of pixels padded to 12
    expect(rgb(bmp, 0, 0)).toEqual([0xff, 0xb5, 0x47])
    expect(rgb(bmp, 1, 0)).toEqual([0x17, 0x15, 0x1a])
    expect(rgb(bmp, 2, 0)[0]).toBeGreaterThan(0x17)
    expect(rgb(bmp, 2, 0)[0]).toBeLessThan(0xff)
  })
  it('draws the sidebar on charcoal and the header on white, each with the amber mark', () => {
    const s = sidebarBmp(), h = headerBmp()
    expect([s.readInt32LE(18), s.readInt32LE(22)]).toEqual([164, 314])
    expect([h.readInt32LE(18), h.readInt32LE(22)]).toEqual([150, 57])
    expect(rgb(s, 2, 300)).toEqual([0x17, 0x15, 0x1a])
    expect(rgb(s, 82, 48 + 20)).toEqual([0xff, 0xb5, 0x47]) // the mark's upper body: amber
    expect(rgb(h, 5, 5)).toEqual([0xff, 0xff, 0xff])
    expect(rgb(h, 101 + 8, 8 + 20)).toEqual([0xff, 0xb5, 0x47])
  })
  it('is committed up to date (run node app/build-tools/make-installer-art.ts)', () => {
    expect(readFileSync(join(__dirname, '../build/installerSidebar.bmp')).equals(sidebarBmp())).toBe(true)
    expect(readFileSync(join(__dirname, '../build/installerHeader.bmp')).equals(headerBmp())).toBe(true)
  })
  it('ships the license page text identical to the repository LICENSE', () => {
    expect(readFileSync(join(__dirname, '../build/license.txt'), 'utf8').replace(/\r\n/g, '\n')).toBe(readFileSync(join(__dirname, '../../LICENSE'), 'utf8').replace(/\r\n/g, '\n'))
  })
})
```

Adjust the two amber probe coordinates if the mark's geometry puts them in the triangle. Pick a point inside the amber body to the left of the triangle (u < 0.36 of the mark), and keep the assertion amber.

Run: `npm --prefix app test -- installerArt`
Expected: FAIL, the modules are missing.

- [ ] **Step 2: Implement `bmp.ts` and `make-installer-art.ts`.** Follow `png.ts` and `make-icon.ts`: the CLI guard, `--check`, and `REPO_ROOT`. Generate the art, and copy `LICENSE` to `app/build/license.txt`.

Run: `node app/build-tools/make-installer-art.ts && node app/build-tools/make-installer-art.ts --check && echo ok`
Expected: `ok`.

Look at both BMPs with the Read tool and describe them in the report.

- [ ] **Step 3: Write `app/build/installer.nsh`:**

```nsis
; Included by electron-builder's installer script (nsis.include). On a real uninstall (never during an
; update, which runs the old uninstaller silently, and never in silent mode) offer to remove Aurora's
; library, playlists, settings and cover cache as well. Music files are never touched.
!macro customUnInstall
  ${ifNot} ${isUpdated}
    MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your Aurora library, playlists, settings and cover cache?$\r$\n$\r$\nYour music files are not touched." /SD IDNO IDNO aurora_keep_data
      SetShellVarContext current
      RMDir /r "$APPDATA\Aurora"
    aurora_keep_data:
  ${endIf}
!macroend
```

- [ ] **Step 4: Edit the `nsis:` section.** Keep the existing keys and add:

```yaml
  license: build/license.txt
  installerSidebar: build/installerSidebar.bmp
  uninstallerSidebar: build/installerSidebar.bmp
  installerHeader: build/installerHeader.bmp
  include: build/installer.nsh
  shortcutName: Aurora
  createDesktopShortcut: true
  createStartMenuShortcut: true
  runAfterFinish: true
```

Use `include`, not `script`: a custom `script` stops electron-builder from signing the uninstaller.

- [ ] **Step 5: Build and exercise the installer silently.**

Run: `npm run package`
Expected: success. The electron-builder log shows no NSIS warnings about the sidebar, header or license (copy any NSIS warning lines into the report).

Then, from Git Bash:

```bash
INST="$LOCALAPPDATA/Programs/Aurora"
DATA=$(mktemp -d)
app/dist/Aurora-*-win-setup.exe /S /D="$(cygpath -w "$INST-test")"
ls "$INST-test" | grep -E "Aurora.exe|Uninstall Aurora.exe"
"$INST-test/Uninstall Aurora.exe" /S
```

Expected:
- the silent install creates `Aurora.exe` and `Uninstall Aurora.exe`;
- the silent uninstall removes the install folder;
- **`%APPDATA%\Aurora` still exists**, because silent mode defaults to "No". Check it with `ls "$APPDATA/Aurora"` before and after; it may hold your real library, which must survive.

Do **not** install over or uninstall a real installed Aurora. If `%LOCALAPPDATA%\Programs\Aurora` already exists, stop and report instead of running this step.

Wait for the uninstaller to finish: it relaunches itself from a temp copy, so poll until the folder is gone (up to about 20 s).

The interactive pages (license, branding, the clean-uninstall prompt) are a manual check for `docs/SIGNING.md`'s release checklist (Task 8).

- [ ] **Step 6: Run the tests and commit**

Run: `npm --prefix app test && npm --prefix app run typecheck`
Expected: pass.

```bash
git add app/build-tools/bmp.ts app/build-tools/make-installer-art.ts app/build/installerSidebar.bmp app/build/installerHeader.bmp app/build/license.txt app/build/installer.nsh app/electron-builder.yml app/test/installerArt.test.ts
git commit -m "feat(installer): branded NSIS wizard, license page and an opt-in clean uninstall"
```

---

### Task 6: File associations ("Open with Aurora")

**Files:**
- Create: `app/src/main/openFiles.ts`, `app/e2e/open-with.spec.ts`
- Modify:
  - `app/src/main/index.ts`: argv at start-up, `second-instance`, and macOS `open-file`;
  - `app/electron-builder.yml`: `fileAssociations`;
  - `app/e2e/fixture.ts`: `launchAurora` gets an `args?: string[]` option, appended after the main script;
  - `README.md`: one line under Install.
- Test: `app/test/openFiles.test.ts`

**Interfaces:**
- `audioFilesFromArgv(argv: readonly string[], o: { isPackaged: boolean; cwd: string; exists: (p: string) => boolean }): string[]`:
  - skips `argv[0]`, and also `argv[1]` (the main script) when unpackaged;
  - skips anything starting with `-`;
  - resolves relative paths against `cwd`;
  - keeps files whose extension (case-insensitive) is in `AUDIO_EXTENSIONS` and that exist;
  - preserves order and drops duplicates.
- `createFileOpener(o: { call: (method: 'queue.setPaths' | 'player.play', params: object) => Promise<unknown>; batchMs?: number; setTimer?: (fn: () => void, ms: number) => unknown; onOpen?: () => void })` returns `{ open(paths: string[]): void; engineReady(): void; engineLost(): void }`:
  - paths opened within `batchMs` (default 250) are joined into one queue;
  - while the engine isn't ready they are held, and `engineReady()` flushes them;
  - a flush calls `queue.setPaths({ paths, startIndex: 0 })`, then `player.play({})`, then `onOpen()`;
  - `engineLost()` marks the engine not ready again (supervisor restart).

- [ ] **Step 1: Write the failing unit tests** in `app/test/openFiles.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { audioFilesFromArgv, createFileOpener } from '../src/main/openFiles'

const exists = (p: string) => !p.includes('missing')
describe('audio files from argv', () => {
  it('takes existing audio files after the executable (and the main script when unpackaged)', () => {
    expect(audioFilesFromArgv(['C:\\A\\Aurora.exe', 'C:\\Music\\a.FLAC', '--flag', 'C:\\Music\\notes.txt', 'C:\\Music\\missing.mp3', 'C:\\Music\\b.mp3'], { isPackaged: true, cwd: 'C:\\', exists }))
      .toEqual(['C:\\Music\\a.FLAC', 'C:\\Music\\b.mp3'])
    expect(audioFilesFromArgv(['/usr/bin/electron', '/repo/app/out/main/index.js', '/m/c.opus'], { isPackaged: false, cwd: '/', exists })).toEqual(['/m/c.opus'])
  })
  it('resolves relative paths against the launching directory and drops duplicates', () => {
    const got = audioFilesFromArgv(['/opt/aurora', 'song.ogg', './song.ogg'], { isPackaged: true, cwd: '/home/me/Music', exists })
    expect(got.map((p) => p.replace(/\\/g, '/'))).toEqual([expect.stringMatching(/\/home\/me\/Music\/song\.ogg$/)])
  })
})

describe('file opener', () => {
  function setup() {
    const calls: [string, object][] = []
    const timers: (() => void)[] = []
    const onOpen = vi.fn()
    const o = createFileOpener({ call: async (m, p) => { calls.push([m, p]) }, setTimer: (fn) => timers.push(fn), onOpen })
    const flushTimers = async () => { while (timers.length) timers.shift()!(); await Promise.resolve(); await Promise.resolve() }
    return { o, calls, flushTimers, onOpen }
  }
  it('holds files opened before the engine is ready and plays them once it is', async () => {
    const { o, calls, flushTimers, onOpen } = setup()
    o.open(['/m/a.flac'])
    await flushTimers()
    expect(calls).toEqual([])
    o.engineReady()
    await flushTimers()
    expect(calls).toEqual([['queue.setPaths', { paths: ['/m/a.flac'], startIndex: 0 }], ['player.play', {}]])
    expect(onOpen).toHaveBeenCalledTimes(1)
  })
  it('joins files opened in quick succession into one queue', async () => {
    const { o, calls, flushTimers } = setup()
    o.engineReady()
    o.open(['/m/a.flac']); o.open(['/m/b.flac'])
    await flushTimers()
    expect(calls[0]).toEqual(['queue.setPaths', { paths: ['/m/a.flac', '/m/b.flac'], startIndex: 0 }])
  })
  it('ignores an empty open and holds again after the engine is lost', async () => {
    const { o, calls, flushTimers } = setup()
    o.engineReady(); o.open([]); await flushTimers()
    expect(calls).toEqual([])
    o.engineLost(); o.open(['/m/c.mp3']); await flushTimers()
    expect(calls).toEqual([])
    o.engineReady(); await flushTimers()
    expect(calls.length).toBe(2)
  })
})
```

Run: `npm --prefix app test -- openFiles`
Expected: FAIL, the module is missing.

- [ ] **Step 2: Implement `openFiles.ts`.** Use `node:path` `isAbsolute` and `resolve`. Import `AUDIO_EXTENSIONS` from `@shared/api`. Write the opener as a small closure with `pending: string[]`, `ready: boolean` and `timer`.

- [ ] **Step 3: Wire it in `index.ts`.**
  - **Create the opener** inside `whenReady`:

```ts
const opener = createFileOpener({ call: (m, p) => supervisor.call(m, p), onOpen: () => windows.showMain() })
```

  - **Supervisor status:** `ready` calls `opener.engineReady()`; `restarting`, `failed` and `starting` call `opener.engineLost()`.
  - **Start-up files:**

```ts
opener.open(audioFilesFromArgv(process.argv, { isPackaged: app.isPackaged, cwd: process.cwd(), exists: existsSync }))
```

  - **Second instance:** `app.on('second-instance', (_e, argv, workingDirectory) => { windows?.showMain(); opener?.open(audioFilesFromArgv(argv, { isPackaged: app.isPackaged, cwd: workingDirectory, exists: existsSync })) })`. The opener may not exist yet, so keep a module-level `let opener` and a `pendingOpen: string[]` that `whenReady` drains.
  - **macOS:** register `app.on('open-file', (e, path) => { e.preventDefault(); … same queueing … })` at the top level, **before** `whenReady`.
  - **Test hooks:** `AURORA_TEST_OPEN_FILES` and the dialog paths are unchanged.

- [ ] **Step 4: Configure the associations.** In `electron-builder.yml`, at the top level:

```yaml
# "Open with Aurora" / double-click (the app reads files from argv, second-instance and open-file).
# The extension list mirrors AUDIO_EXTENSIONS in src/shared/api.ts (a test keeps them equal).
fileAssociations:
  - ext: [mp3, m4a, m4b, mp4, aac, flac, wav, aif, aiff, aifc, ogg, oga, opus, dsf, dff]
    name: Audio
    description: Audio file
    role: Viewer
    mimeType: audio/mpeg;audio/mp4;audio/x-m4a;audio/aac;audio/flac;audio/wav;audio/x-wav;audio/aiff;audio/x-aiff;audio/ogg;audio/opus;audio/x-dsf;audio/x-dff
```

Check how electron-builder 26 expects `mimeType` (a string or a list) in `app-builder-lib`'s scheme or docs, and use that form.

Add a test in `installKind.test.ts` that parses the yml with `yaml` and asserts the `ext` list equals `[...AUDIO_EXTENSIONS]`.

- [ ] **Step 5: Write the e2e test** in `app/e2e/open-with.spec.ts`. Use `launchAurora({ dataDir, args: [resolve(root, 'fixtures/out/gap_a.flac')], speed: 0.05 })`. Then:
  - poll `player.state` through `window.aurora.call` until `status === 'playing'`;
  - check that `track.path` ends with `gap_a.flac`;
  - check that the Pause button is visible.

Extend `launchAurora` with the `args` option (appended after the main script path).

- [ ] **Step 6: Run everything**

Run: `npm --prefix app test && npm --prefix app run typecheck && npm run e2e`
Expected: all pass, with 6 e2e tests now. If the running Aurora locks the debug engine, use the target-verify procedure from the common rules: point the fixture at a target-verify engine for the run only, never commit that change, and delete the folder afterwards.

- [ ] **Step 7: Commit**

```bash
git add app/src/main/openFiles.ts app/src/main/index.ts app/electron-builder.yml app/e2e app/test README.md
git commit -m "feat(app): open audio files from the OS (file associations, second instance, macOS open-file)"
```

---

### Task 7: Resume playback after an update restart

**Files:**
- Create: `app/src/main/resumeAfterUpdate.ts`
- Modify: `app/src/shared/settings.ts` (`resumeAfterUpdate: boolean`, default `false`, merged as a boolean), `app/src/main/index.ts`
- Test: `app/test/resumeAfterUpdate.test.ts`, `app/test/settings.test.ts`

**Interfaces:**
- `createResumer(o: { get: () => boolean; set: (v: boolean) => void; play: () => Promise<unknown> })` returns:
  - `noteStatus(status: string): void`, which tracks the last `player.state` status;
  - `beforeUpdateRestart(): void`, which calls `set(true)` if the last status was `playing`;
  - `engineReady(o: { openingFiles: boolean }): void`. If `get()` is true it calls `set(false)` first, then `play()` unless `openingFiles` is true. It acts once per launch and ignores later calls.

- [ ] **Step 1: Write the failing tests** in `app/test/resumeAfterUpdate.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { createResumer } from '../src/main/resumeAfterUpdate'

function setup(stored = false) {
  let flag = stored
  const play = vi.fn(async () => {})
  const r = createResumer({ get: () => flag, set: (v) => { flag = v }, play })
  return { r, play, flag: () => flag }
}

describe('resume after update', () => {
  it('remembers to resume only if music was playing when the update restart began', () => {
    const a = setup(); a.r.noteStatus('playing'); a.r.beforeUpdateRestart(); expect(a.flag()).toBe(true)
    const b = setup(); b.r.noteStatus('paused'); b.r.beforeUpdateRestart(); expect(b.flag()).toBe(false)
  })
  it('resumes once on the next launch and clears the flag first', () => {
    const { r, play, flag } = setup(true)
    r.engineReady({ openingFiles: false })
    expect(flag()).toBe(false)
    expect(play).toHaveBeenCalledTimes(1)
    r.engineReady({ openingFiles: false }) // an engine restart later in the session
    expect(play).toHaveBeenCalledTimes(1)
  })
  it('does not resume when launched to open files, or after an ordinary quit', () => {
    const a = setup(true); a.r.engineReady({ openingFiles: true }); expect(a.play).not.toHaveBeenCalled(); expect(a.flag()).toBe(false)
    const b = setup(false); b.r.engineReady({ openingFiles: false }); expect(b.play).not.toHaveBeenCalled()
  })
})
```

In `settings.test.ts`, add: `resumeAfterUpdate` defaults to `false`, accepts booleans, and ignores other types.

Run: `npm --prefix app test -- resumeAfterUpdate settings`
Expected: FAIL.

- [ ] **Step 2: Implement.**
  - Add the module and the settings field.
  - In `index.ts`, create the resumer:

```ts
const resumer = createResumer({ get: () => settings.get().resumeAfterUpdate, set: (v) => { settings.update({ resumeAfterUpdate: v }); settings.flush() }, play: () => supervisor.call('player.play', {}) })
```

  - Feed `resumer.noteStatus(s.status)` from the existing `player.state` notification handler.
  - Call `resumer.beforeUpdateRestart()` at the start of the update controller's `beforeInstall`, before the engine stops.
  - Call `resumer.engineReady({ openingFiles: <whether start-up argv held audio files> })` on the first `ready` status.
  - **Ordering:** the engine restores the session (paused) before it announces `engine.ready`, and `player.play` resumes that restored queue. Check this in `engine/crates/aurora-engine/src/rpc/mod.rs` `run()`, where `Command::Restore` is sent before `engine.ready`. Note it in the report.

- [ ] **Step 3: Run everything and commit**

Run: `npm --prefix app test && npm --prefix app run typecheck`
Expected: pass.

```bash
git add app/src/main/resumeAfterUpdate.ts app/src/main/index.ts app/src/shared/settings.ts app/test
git commit -m "feat(app): resume playback after an update restart"
```

---

### Task 8: Code-signing policy, SignPath onboarding and release docs

**Files:**
- Create: `docs/SIGNING.md`
- Modify: `README.md`, `docs/RELEASING.md`

- [ ] **Step 1: README.**
  - **New "Code signing policy" section.** SignPath Foundation requires this on the project page; use exactly these headings:
    - **Attribution, one line:** "Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org)."
    - **Team roles:**
      - committers and reviewers: [kartikeychoudhary](https://github.com/kartikeychoudhary);
      - approvers: [kartikeychoudhary](https://github.com/kartikeychoudhary).
    - **Privacy policy:**
      - Aurora makes one kind of network request: the update check. It asks GitHub's API for the latest release, and installers download from GitHub Releases.
      - It sends no personal data and has no telemetry.
      - Turn it off in Settings → About.
      - It sends nothing else anywhere unless the user asks for it.
  - **Install section:**
    - replace the "unsigned Windows builds" SmartScreen note: Windows builds are signed by SignPath Foundation;
    - keep a shorter note for builds made before signing was set up;
    - add the "Open with Aurora" association line;
    - describe the clean-uninstall question.
  - **Updates section:** add that playback resumes after **Restart now** if music was playing.

- [ ] **Step 2: Write `docs/SIGNING.md`.** It gives the maintainer's one-time onboarding and the per-release steps:
  1. **Apply.** Apply at https://signpath.org/apply. Requirements, summarised from their terms:
     - an OSI license (MIT ✓);
     - a public repository;
     - releases built from source in CI (the Release workflow ✓);
     - the code-signing policy on the project page (README ✓);
     - no malware or PUA behaviour.

     Approval is manual and can take some time.
  2. **Set up SignPath** after acceptance:
     - note the organization ID and project slug;
     - add the GitHub repository as a trusted build system (SignPath's GitHub connector), so requests can only come from this repository's workflows;
     - create artifact configurations `binaries` and `installer`, pasting `.signpath/artifact-configurations/*.xml`;
     - check that the policies `test-signing` and `release-signing` exist; `release-signing` needs an approver;
     - create a CI user API token with submitter permission.
  3. **Configure GitHub:**
     - repository variables `SIGNPATH_ORGANIZATION_ID` and `SIGNPATH_PROJECT_SLUG`;
     - secret `SIGNPATH_API_TOKEN`.

     Until these exist, releases stay unsigned and nothing breaks.
  4. **Do a dry run.** Actions → Release → Run workflow. It uses `test-signing`, so no approval is needed.
     - Check that the Windows job runs both signing requests and that `check-signatures.ps1` passes.
     - The test certificate isn't trusted, so SmartScreen still warns on test builds.
  5. **Each release.** Push the tag. The Windows job waits (up to 2 h per request) for two approvals in SignPath: binaries, then installer. Approve each after checking that the request came from the expected tag and commit.
  6. **How it works:** the two passes, why the uninstaller may stay unsigned, and how `latest.yml` is refreshed.
  7. **Troubleshooting:**
     - "no signed copy of …": the passes built different bytes. Re-run the job, and if it persists, report it.
     - A publisher mismatch failure means the release certificate's CN changed. Explain the consequence: existing installs only accept updates from the old publisher name. Changing `publisherName` therefore breaks auto-update for every installed copy, and each needs one manual reinstall. Do it only deliberately, and say so in the release notes.
  8. **Manual installer checklist** for the first signed release:
     - the wizard shows the branding and the license page;
     - double-clicking an mp3 opens Aurora;
     - the uninstall prompt appears on an interactive uninstall;
     - an update with music playing resumes playback;
     - the file properties' Digital Signatures tab shows SignPath Foundation on `Aurora.exe`, `aurora-engine.exe` and the setup.

- [ ] **Step 3: Update `docs/RELEASING.md`.**
  - Replace the Windows rows of the signing-secrets table (`WIN_CSC_*`) with a pointer to `docs/SIGNING.md`.
  - Add the two SignPath approvals to "Cutting a release".

- [ ] **Step 4: Check the docs and commit**

Run: `grep -n "WIN_CSC" -r docs README.md .github`
Expected: nothing left.

```bash
git add README.md docs/SIGNING.md docs/RELEASING.md
git commit -m "docs: code signing policy, SignPath onboarding and the signed release flow"
```

---

## Manual checks before calling this done

Record these in `docs/superpowers/m4-execution-notes.md` under a new "Windows signing" heading.

1. **SignPath Foundation onboarding** (human): the application is accepted, the project is configured, and the GitHub variables and secret are set.
2. **A `workflow_dispatch` run** with test-signing: both requests complete, `check-signatures.ps1` passes, and the smoke tests pass on the signed app.
3. **The first tagged release:**
   - the installer shows SignPath Foundation in Digital Signatures and no SmartScreen block;
   - the wizard is branded and has the license page;
   - "Open with Aurora" works;
   - the uninstall prompt appears interactively and not during updates.
4. **An update** from that release to the next, with music playing: it downloads differentially (the log shows a blockmap download), installs silently, relaunches, and resumes playback.
