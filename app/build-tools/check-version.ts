import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { REPO_ROOT, checkVersions, engineVersion } from './versions.ts'

const i = process.argv.indexOf('--tag')
const tag = i > 0 ? process.argv[i + 1] : undefined
const app = JSON.parse(readFileSync(join(REPO_ROOT, 'app/package.json'), 'utf8')).version as string
const engine = engineVersion(readFileSync(join(REPO_ROOT, 'engine/Cargo.toml'), 'utf8'))
const problems = checkVersions({ app, engine, tag })
if (problems.length) {
  for (const p of problems) console.error(p)
  process.exit(1)
}
console.log(`version=${app}`)
