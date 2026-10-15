// Renders the NSIS wizard art from build-tools/installer-art/art.html (through Electron, at the exact bitmap sizes)
// into app/build/installerSidebar.bmp and installerHeader.bmp, plus PNG previews in app/build/generated/.
// The BMPs are committed assets: text rendering differs between machines, so re-run this by hand after editing
// art.html rather than checking for staleness. Also refreshes app/build/license.txt (the license page).
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { encodeBmp24 } from './bmp.ts'
import { encodePng } from './png.ts'
import { REPO_ROOT } from './versions.ts'

const ART = [
  { view: 'sidebar', file: 'installerSidebar.bmp', width: 164, height: 314 },
  { view: 'header', file: 'installerHeader.bmp', width: 150, height: 57 },
]

const app = join(REPO_ROOT, 'app')
const electron = createRequire(import.meta.url)('electron') as string // the binary's path
const out = mkdtempSync(join(tmpdir(), 'aurora-art-'))
try {
  mkdirSync(join(app, 'build', 'generated'), { recursive: true })
  for (const a of ART) {
    execFileSync(electron, [join(app, 'build-tools', 'render-installer-art.cjs'), a.view, join(out, `${a.view}.bgra`)], { stdio: 'inherit' })
    const bgra = new Uint8Array(readFileSync(join(out, `${a.view}.bgra`)))
    if (bgra.length !== a.width * a.height * 4) throw new Error(`${a.view}: got ${bgra.length} bytes, expected ${a.width * a.height * 4}`)
    writeFileSync(join(app, 'build', a.file), encodeBmp24(a.width, a.height, bgra, [0x17, 0x15, 0x1a]))
    writeFileSync(join(app, 'build', 'generated', `${a.view}-preview.png`), encodePng(a.width, a.height, bgra))
    console.log(`wrote app/build/${a.file}`)
  }
  copyFileSync(join(REPO_ROOT, 'LICENSE'), join(app, 'build', 'license.txt'))
} finally {
  rmSync(out, { recursive: true, force: true })
}
