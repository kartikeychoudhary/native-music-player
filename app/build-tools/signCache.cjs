// Where the two-pass Windows signing (docs/SIGNING.md) keeps files: each file electron-builder wants
// signed is collected under a name derived from its unsigned bytes, and its signed copy is found again
// by the same name. A file that changed between the passes simply has no signed copy.
const { createHash } = require('node:crypto')
const { copyFileSync, existsSync, mkdirSync, readFileSync } = require('node:fs')
const { basename, join } = require('node:path')

function cacheName(file) {
  return `${createHash('sha256').update(readFileSync(file)).digest('hex').slice(0, 16)}-${basename(file)}`
}

function collect(file, dir) {
  mkdirSync(dir, { recursive: true })
  const name = cacheName(file)
  copyFileSync(file, join(dir, name))
  return name
}

function signedCopy(file, dir) {
  const p = join(dir, cacheName(file))
  return existsSync(p) ? p : null
}

/** The installer is signed after it is built; the uninstaller is signed when its bytes repeat; everything else must be signed. */
function kindOf(file) {
  const name = basename(file.replace(/\\/g, '/')).toLowerCase()
  if (name.endsWith('__uninstaller.exe')) return 'uninstaller'
  if (/-win-setup\.exe$/.test(name)) return 'installer'
  return 'binary'
}

module.exports = { cacheName, collect, signedCopy, kindOf }
