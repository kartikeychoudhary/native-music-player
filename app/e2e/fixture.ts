import { _electron as electron, type ElectronApplication, type Page } from '@playwright/test'
import { copyFileSync, mkdirSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

export const root = resolve(__dirname, '../..')
const exe = process.platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'

export function tempDir(prefix: string): string { return mkdtempSync(join(tmpdir(), prefix)) }

/** A fresh folder holding copies of generated fixtures (their file stems become the titles). */
export function musicFolder(files: string[]): string {
  const dir = join(tempDir('aurora-music-'), 'Music')
  mkdirSync(dir, { recursive: true })
  for (const f of files) copyFileSync(resolve(root, 'fixtures/out', f), join(dir, f))
  return dir
}

export async function launchAurora(o: { dataDir: string; musicDir?: string; openFiles?: string; speed?: number }): Promise<{ app: ElectronApplication; win: Page }> {
  const app = await electron.launch({
    args: [resolve(__dirname, '../out/main/index.js')],
    env: {
      ...process.env,
      AURORA_ENGINE_PATH: resolve(root, 'engine/target/debug', exe),
      AURORA_OUTPUT: 'null',
      AURORA_NULL_SPEED: String(o.speed ?? 0.5),
      AURORA_OSMEDIA: '0',
      AURORA_DATA_DIR: o.dataDir,
      ...(o.musicDir ? { AURORA_TEST_OPEN_FOLDER: o.musicDir } : {}),
      ...(o.openFiles ? { AURORA_TEST_OPEN_FILES: o.openFiles } : {}),
    },
  })
  return { app, win: await app.firstWindow() }
}
