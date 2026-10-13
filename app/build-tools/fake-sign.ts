// Local stand-in for SignPath: "signs" sign/to-sign by copying each file to sign/signed unchanged.
// It proves the collect -> apply mechanics (every file found again by hash); it adds no signatures.
import { copyFileSync, mkdirSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'

const dir = resolve(process.argv[2] ?? 'sign')
mkdirSync(join(dir, 'signed'), { recursive: true })
const names = readdirSync(join(dir, 'to-sign'))
for (const n of names) copyFileSync(join(dir, 'to-sign', n), join(dir, 'signed', n))
console.log(`fake-signed ${names.length} files into ${join(dir, 'signed')}`)
