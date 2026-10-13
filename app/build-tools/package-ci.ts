import { execFileSync } from 'node:child_process'
import { mkdirSync, readdirSync, rmSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { stageEngine } from './stage-engine.ts'
import { REPO_ROOT } from './versions.ts'

type Platform = 'win32' | 'darwin' | 'linux'
type Arch = 'x64' | 'arm64'
export type SignMode = 'collect' | 'apply'
/** The publisher SignPath signs as. Only release-signed builds carry it: electron-updater rejects updates from anyone else. */
export const SIGNPATH_PUBLISHER = 'SignPath Foundation'
export const TRIPLES: Record<Platform, { x64: string; arm64: string }> = {
  win32: { x64: 'x86_64-pc-windows-msvc', arm64: 'aarch64-pc-windows-msvc' },
  darwin: { x64: 'x86_64-apple-darwin', arm64: 'aarch64-apple-darwin' },
  linux: { x64: 'x86_64-unknown-linux-gnu', arm64: 'aarch64-unknown-linux-gnu' },
}
const FLAG: Record<Platform, string> = { win32: '--win', darwin: '--mac', linux: '--linux' }
const PORTABLE: Record<Platform, string> = { win32: 'zip', darwin: 'zip', linux: 'tar.gz' }
/** The targets electron-builder.yml lists per platform (each with arch: [x64, arm64]). */
const YML_TARGETS: Record<Platform, string[]> = { win32: ['nsis', 'zip'], darwin: ['dmg', 'zip'], linux: ['AppImage', 'deb', 'rpm'] }
const ARCH_FLAGS = ['--x64', '--arm64']

/**
 * In the `collect` signing pass only the standard NSIS installer is built: it contains every file electron-builder signs.
 * The `apply` pass builds what an unsigned run builds. With `releaseSigned` (the release-signing policy, not a test-signed
 * build) it also sets the publisher electron-updater will require of updates, so a test-signed build never carries it.
 */
export function builderArgs(o: { platform: Platform; variant: 'standard' | 'nofdk'; signed: boolean; signMode?: SignMode; releaseSigned?: boolean }): string[] {
  if (o.signMode === 'collect') return ['--win', 'nsis', '--x64', '--arm64', '--publish', 'never']
  const args = o.variant === 'standard'
    ? [FLAG[o.platform], '--x64', '--arm64', '--publish', 'never']
    : [FLAG[o.platform], PORTABLE[o.platform], '--x64', '--arm64', '--publish', 'never', '-c.directories.output=dist-nofdk', '-c.artifactName=${productName}-${version}-${os}-${arch}-nofdk.${ext}']
  if (o.platform === 'darwin' && !o.signed) args.push('-c.mac.identity=-') // ad-hoc: arm64 macOS refuses unsigned code
  if (o.signMode === 'apply' && o.releaseSigned) args.push(`-c.win.signtoolOptions.publisherName=${SIGNPATH_PUBLISHER}`)
  return args
}

/**
 * electron-builder's environment in a signing pass. The build cache is off: on a machine with a certificate it could
 * restore an unsigned Aurora.exe in the apply pass without calling the sign hook.
 */
export function builderEnv(signMode: SignMode | undefined, signDir: string, env: NodeJS.ProcessEnv): NodeJS.ProcessEnv | undefined {
  if (!signMode) return undefined
  return { ...env, AURORA_SIGN_MODE: signMode, AURORA_SIGN_DIR: signDir, ELECTRON_BUILDER_DISABLE_BUILD_CACHE: 'true' }
}

/** Empties <signDir>/to-sign before a collect pass, so files from an earlier run are never sent for signing. */
export function resetToSign(signDir: string): void {
  rmSync(join(signDir, 'to-sign'), { recursive: true, force: true })
  mkdirSync(join(signDir, 'to-sign'), { recursive: true })
}

/**
 * Test-only (AURORA_CI_ARCHES): narrows builder arguments to some arches for a local run. electron-builder ignores
 * the arch flags for targets that list their own archs, so a bare platform flag gets the yml's targets named.
 */
export function limitArches(args: string[], platform: Platform, arches: readonly Arch[]): string[] {
  if (ARCH_FLAGS.every((f) => arches.includes(f.slice(2) as Arch))) return args
  const out = args.filter((a) => !ARCH_FLAGS.includes(a))
  let at = out.indexOf(FLAG[platform]) + 1
  const named = at < out.length && !out[at].startsWith('-')
  if (named) while (at < out.length && !out[at].startsWith('-')) at++
  out.splice(at, 0, ...(named ? [] : YML_TARGETS[platform]), ...arches.map((a) => `--${a}`))
  return out
}

/**
 * The engine binary to stage. In the apply pass this is still the unsigned engine, once its signed copy is known to
 * exist: electron-builder runs the sign hook on resources/bin/<platform>-<arch>/aurora-engine.exe too, and the hook swaps in the
 * signed copy by the unsigned bytes' hash, as for Aurora.exe and the DLLs. A staged signed engine has no signed copy
 * of its own, so the hook would fail the build.
 */
export function engineToStage(o: { src: string; name: string; signMode?: SignMode; signedCopy: (src: string) => string | null }): string {
  if (o.signMode !== 'apply') return o.src
  if (!o.signedCopy(o.src)) throw new Error(`no signed copy of ${o.name}`)
  return o.src
}

function cli(): number {
  const usage = 'usage: package-ci.ts --platform win32|darwin|linux [--signed] [--sign-mode collect|apply --sign-dir <dir> [--release-signed]]'
  const arg = (n: string) => { const i = process.argv.indexOf(`--${n}`); return i > 0 ? process.argv[i + 1] : undefined }
  const platform = process.argv[process.argv.indexOf('--platform') + 1] as Platform
  if (!(platform in TRIPLES)) { console.error(usage); return 2 }
  const signed = process.argv.includes('--signed')
  const signing = process.argv.includes('--sign-mode') || process.argv.includes('--sign-dir')
  const releaseSigned = process.argv.includes('--release-signed')
  const signMode = arg('sign-mode') as SignMode | undefined
  const signDirArg = arg('sign-dir')
  if (signing && platform !== 'win32') { console.error('package-ci.ts: --sign-mode and --sign-dir are Windows only'); return 2 }
  if (signing && ((signMode !== 'collect' && signMode !== 'apply') || !signDirArg || signDirArg.startsWith('--'))) { console.error(usage); return 2 }
  if (releaseSigned && signMode !== 'apply') { console.error('package-ci.ts: --release-signed needs --sign-mode apply'); return 2 }
  const signDir = signDirArg ? resolve(signDirArg) : ''
  // Test-only knob for local runs: AURORA_CI_ARCHES=x64 builds one arch. CI always builds both.
  const arches = (process.env.AURORA_CI_ARCHES ?? 'x64,arm64').split(',').map((a) => a.trim()) as Arch[]
  if (!arches.length || arches.some((a) => a !== 'x64' && a !== 'arm64')) { console.error('package-ci.ts: AURORA_CI_ARCHES must list x64 and/or arm64'); return 2 }
  const app = join(REPO_ROOT, 'app')
  const sh = process.platform === 'win32'
  const node = (script: string, ...a: string[]) => execFileSync(process.execPath, [join(app, 'build-tools', script), ...a], { stdio: 'inherit' })
  const npx = (...a: string[]) => execFileSync('npx', a, { cwd: app, stdio: 'inherit', shell: sh })
  const env = builderEnv(signMode, signDir, process.env)
  // With a shell, arguments are joined unescaped: quote the ones with spaces (the SignPath publisher).
  const quote = (a: string) => (sh && a.includes(' ') ? `"${a}"` : a)
  const builder = (args: string[]) => execFileSync('npx', ['electron-builder', ...limitArches(args, platform, arches).map(quote)], { cwd: app, stdio: 'inherit', shell: sh, env })
  const exe = platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'
  const engine = (arch: Arch, variant: string) => join(REPO_ROOT, 'engines', arch, variant, exe)
  const targets = Object.values(TRIPLES[platform]).flatMap((t) => ['--target', t])
  const signCache = createRequire(import.meta.url)('./signCache.cjs') as { collect(file: string, dir: string): string; signedCopy(file: string, dir: string): string | null }
  npx('electron-vite', 'build')
  if (signMode === 'collect') {
    resetToSign(signDir)
    for (const arch of arches) stageEngine({ src: engine(arch, 'standard'), appRoot: app, platform, arch })
    for (const variant of ['standard', 'nofdk'] as const) for (const arch of arches) signCache.collect(engine(arch, variant), join(signDir, 'to-sign'))
    node('licenses.ts', ...targets)
    node('build-info.ts', '--variant', 'standard')
    builder(builderArgs({ platform, variant: 'standard', signed, signMode }))
    console.log(`to-sign: ${readdirSync(join(signDir, 'to-sign')).length} files`)
  } else {
    const staged = (arch: Arch, variant: string) => engineToStage({
      src: engine(arch, variant), name: `engines/${arch}/${variant}/${exe}`, signMode, signedCopy: (src) => signCache.signedCopy(src, join(signDir, 'signed')),
    })
    for (const variant of ['standard', 'nofdk'] as const) {
      for (const arch of arches) stageEngine({ src: staged(arch, variant), appRoot: app, platform, arch })
      node('licenses.ts', ...targets, ...(variant === 'nofdk' ? ['--no-fdk'] : []))
      node('build-info.ts', '--variant', variant, ...(platform === 'darwin' && signed ? ['--mac-signed'] : []))
      builder(builderArgs({ platform, variant, signed, signMode, releaseSigned }))
    }
  }
  return 0
}

// process.exit() here trips a libuv assertion on Windows (Node 24), so the exit code is set instead.
if (process.argv[1] === fileURLToPath(import.meta.url)) process.exitCode = cli()
