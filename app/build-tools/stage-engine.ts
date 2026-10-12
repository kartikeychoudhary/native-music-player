import { chmodSync, copyFileSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { REPO_ROOT } from './versions.ts'

/** Copies an engine binary to app/resources/engine/<platform>-<arch>/ (electron-builder's extraResources source). */
export function stageEngine(o: { src: string; appRoot: string; platform: string; arch: string }): string {
  const dir = join(o.appRoot, 'resources/engine', `${o.platform}-${o.arch}`)
  mkdirSync(dir, { recursive: true })
  const dest = join(dir, o.platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine')
  copyFileSync(o.src, dest)
  chmodSync(dest, 0o755)
  return dest
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const arg = (n: string) => { const i = process.argv.indexOf(`--${n}`); return i > 0 ? process.argv[i + 1] : undefined }
  const platform = arg('platform') ?? process.platform, arch = arg('arch') ?? process.arch
  const src = arg('src') ?? join(REPO_ROOT, 'engine/target/release', platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine')
  console.log(`staged ${stageEngine({ src, appRoot: join(REPO_ROOT, 'app'), platform, arch })}`)
}
