import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { iconPixels } from '../src/shared/iconPixels.ts'
import { encodePng } from './png.ts'
import { REPO_ROOT } from './versions.ts'

export const ICON_SIZE = 1024
/** Transparent margin (macOS icon grid: an 824 px body in a 1024 px canvas). */
export const ICON_INSET = 100

export function iconPng(): Buffer { return encodePng(ICON_SIZE, ICON_SIZE, iconPixels(ICON_SIZE, ICON_INSET)) }

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const path = join(REPO_ROOT, 'app/build/icon.png')
  if (process.argv.includes('--check')) {
    const same = (() => { try { return readFileSync(path).equals(iconPng()) } catch { return false } })()
    if (!same) { console.error('app/build/icon.png is stale: run node app/build-tools/make-icon.ts'); process.exit(1) }
  } else writeFileSync(path, iconPng())
}
