import { execFileSync } from 'node:child_process'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { stageEngine } from './stage-engine.ts'
import { REPO_ROOT } from './versions.ts'

type Platform = 'win32' | 'darwin' | 'linux'
export const TRIPLES: Record<Platform, { x64: string; arm64: string }> = {
  win32: { x64: 'x86_64-pc-windows-msvc', arm64: 'aarch64-pc-windows-msvc' },
  darwin: { x64: 'x86_64-apple-darwin', arm64: 'aarch64-apple-darwin' },
  linux: { x64: 'x86_64-unknown-linux-gnu', arm64: 'aarch64-unknown-linux-gnu' },
}
const FLAG: Record<Platform, string> = { win32: '--win', darwin: '--mac', linux: '--linux' }
const PORTABLE: Record<Platform, string> = { win32: 'zip', darwin: 'zip', linux: 'tar.gz' }

export function builderArgs(o: { platform: Platform; variant: 'standard' | 'nofdk'; signed: boolean }): string[] {
  const args = o.variant === 'standard'
    ? [FLAG[o.platform], '--x64', '--arm64', '--publish', 'never']
    : [FLAG[o.platform], PORTABLE[o.platform], '--x64', '--arm64', '--publish', 'never', '-c.directories.output=dist-nofdk', '-c.artifactName=${productName}-${version}-${os}-${arch}-nofdk.${ext}']
  if (o.platform === 'darwin' && !o.signed) args.push('-c.mac.identity=-') // ad-hoc: arm64 macOS refuses unsigned code
  return args
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const platform = process.argv[process.argv.indexOf('--platform') + 1] as Platform
  if (!(platform in TRIPLES)) { console.error('usage: package-ci.ts --platform win32|darwin|linux [--signed]'); process.exit(2) }
  const signed = process.argv.includes('--signed')
  const app = join(REPO_ROOT, 'app')
  const sh = process.platform === 'win32'
  const node = (script: string, ...a: string[]) => execFileSync(process.execPath, [join(app, 'build-tools', script), ...a], { stdio: 'inherit' })
  const npx = (...a: string[]) => execFileSync('npx', a, { cwd: app, stdio: 'inherit', shell: sh })
  const exe = platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'
  const targets = Object.values(TRIPLES[platform]).flatMap((t) => ['--target', t])
  npx('electron-vite', 'build')
  for (const variant of ['standard', 'nofdk'] as const) {
    for (const arch of ['x64', 'arm64'] as const) stageEngine({ src: join(REPO_ROOT, 'engines', arch, variant, exe), appRoot: app, platform, arch })
    node('licenses.ts', ...targets, ...(variant === 'nofdk' ? ['--no-fdk'] : []))
    node('build-info.ts', '--variant', variant, ...(platform === 'darwin' && signed ? ['--mac-signed'] : []))
    npx('electron-builder', ...builderArgs({ platform, variant, signed }))
  }
}
