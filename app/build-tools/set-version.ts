import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { REPO_ROOT, SEMVER, setEngineVersion } from './versions.ts'

const v = process.argv[2]
if (!v || !SEMVER.test(v)) { console.error('usage: node app/build-tools/set-version.ts X.Y.Z[-pre]'); process.exit(2) }
const cargo = join(REPO_ROOT, 'engine/Cargo.toml')
writeFileSync(cargo, setEngineVersion(readFileSync(cargo, 'utf8'), v))
const sh = process.platform === 'win32'
// Updates the workspace crates' entries in Cargo.lock and nothing else.
execFileSync('cargo', ['update', '--workspace', '--offline', '--manifest-path', cargo], { stdio: 'inherit', shell: sh })
execFileSync('npm', ['--prefix', join(REPO_ROOT, 'app'), 'version', v, '--no-git-tag-version', '--allow-same-version'], { stdio: 'inherit', shell: sh })
console.log(`version=${v}`)
