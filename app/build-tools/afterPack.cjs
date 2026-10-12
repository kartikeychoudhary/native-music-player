// electron-builder afterPack hook: every packed app must contain its arch's engine, executable
// (artifact round-trips can drop the exec bit). electron-builder requires this file, so it is CommonJS.
const { chmodSync, existsSync } = require('node:fs')
const { join } = require('node:path')

const ARCH = { 0: 'ia32', 1: 'x64', 2: 'armv7l', 3: 'arm64', 4: 'universal' } // builder-util's Arch enum

function enginePathInApp(appOutDir, platform, arch, productFilename) {
  const resources = platform === 'darwin' ? join(appOutDir, `${productFilename}.app`, 'Contents', 'Resources') : join(appOutDir, 'resources')
  return join(resources, 'bin', `${platform}-${arch}`, platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine')
}

async function afterPack(ctx) {
  const arch = ARCH[ctx.arch] ?? String(ctx.arch)
  const engine = enginePathInApp(ctx.appOutDir, ctx.electronPlatformName, arch, ctx.packager.appInfo.productFilename)
  if (!existsSync(engine)) throw new Error(`no engine for ${ctx.electronPlatformName}-${arch} at ${engine} (stage it with app/build-tools/stage-engine.ts)`)
  chmodSync(engine, 0o755)
}

module.exports = afterPack
module.exports.default = afterPack
module.exports.enginePathInApp = enginePathInApp
