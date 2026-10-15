// Electron main script: renders build-tools/installer-art/art.html for one view at its exact NSIS bitmap size
// and writes it as raw BGRA. Usage: electron render-installer-art.cjs <sidebar|header> <out.bgra>.
// One view per run: a second offscreen load in the same session fails with ERR_FAILED. Driven by make-installer-art.ts.
const { app, BrowserWindow } = require('electron')
const { writeFileSync } = require('node:fs')
const { join } = require('node:path')

const SIZES = { sidebar: { width: 164, height: 314 }, header: { width: 150, height: 57 } }

app.commandLine.appendSwitch('force-device-scale-factor', '1')
app.disableHardwareAcceleration()

app.whenReady().then(async () => {
  const [view, outFile] = process.argv.slice(-2)
  try {
    const { width, height } = SIZES[view]
    const win = new BrowserWindow({ width, height, useContentSize: true, show: false, frame: false, webPreferences: { offscreen: true } })
    await win.loadFile(join(__dirname, 'installer-art', 'art.html'), { query: { view } })
    await win.webContents.executeJavaScript('document.fonts.ready.then(() => true)')
    await new Promise((r) => setTimeout(r, 300)) // let the offscreen compositor paint the loaded fonts
    let image = await win.webContents.capturePage({ x: 0, y: 0, width, height })
    const size = image.getSize()
    if (size.width !== width || size.height !== height) image = image.resize({ width, height, quality: 'best' })
    writeFileSync(outFile, image.toBitmap())
  } catch (e) {
    console.error(e)
    process.exitCode = 1
  }
  app.quit()
})
