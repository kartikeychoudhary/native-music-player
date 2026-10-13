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
