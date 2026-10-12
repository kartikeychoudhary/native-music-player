import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { REPO_ROOT } from './versions.ts'

const out = join(REPO_ROOT, 'app/build/generated')
mkdirSync(out, { recursive: true })
const info = { variant: process.argv.includes('--variant') && process.argv[process.argv.indexOf('--variant') + 1] === 'nofdk' ? 'nofdk' : 'standard', macSigned: process.argv.includes('--mac-signed'), commit: process.env.GITHUB_SHA ?? null }
writeFileSync(join(out, 'build-info.json'), `${JSON.stringify(info, null, 2)}\n`)
console.log(`build-info: ${JSON.stringify(info)}`)
