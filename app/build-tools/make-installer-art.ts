import { copyFileSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { iconPixels } from '../src/shared/iconPixels.ts'
import { encodeBmp24 } from './bmp.ts'
import { REPO_ROOT } from './versions.ts'

const CHARCOAL: [number, number, number] = [0x17, 0x15, 0x1a]
const WHITE: [number, number, number] = [0xff, 0xff, 0xff]

/** BGRA canvas of width x height, transparent except for the `size` x `size` app mark at (x, y). */
function canvasWithMark(width: number, height: number, size: number, x: number, y: number): Uint8Array {
  const canvas = new Uint8Array(width * height * 4)
  const mark = iconPixels(size)
  for (let row = 0; row < size; row++) canvas.set(mark.subarray(row * size * 4, (row + 1) * size * 4), ((y + row) * width + x) * 4)
  return canvas
}

/** Wizard side panel (welcome and finish pages, and the uninstaller's): 164 x 314, charcoal, the mark centred near the top. */
export function sidebarBmp(): Buffer {
  return encodeBmp24(164, 314, canvasWithMark(164, 314, 96, (164 - 96) / 2, 48), CHARCOAL)
}

/** Wizard header bar (the white strip of the inner pages): 150 x 57, the mark at the right. */
export function headerBmp(): Buffer {
  return encodeBmp24(150, 57, canvasWithMark(150, 57, 41, 101, 8), WHITE)
}

const BUILD = join(REPO_ROOT, 'app/build')
const FILES: [string, () => Buffer][] = [
  ['installerSidebar.bmp', sidebarBmp],
  ['installerHeader.bmp', headerBmp],
]

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--check')) {
    const stale = FILES.filter(([name, make]) => { try { return !readFileSync(join(BUILD, name)).equals(make()) } catch { return true } })
    if (stale.length) { console.error(`${stale.map(([n]) => `app/build/${n}`).join(', ')} stale: run node app/build-tools/make-installer-art.ts`); process.exit(1) }
  } else {
    for (const [name, make] of FILES) writeFileSync(join(BUILD, name), make())
    copyFileSync(join(REPO_ROOT, 'LICENSE'), join(BUILD, 'license.txt')) // the installer's license page
  }
}
