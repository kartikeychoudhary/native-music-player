import { execFileSync } from 'node:child_process'
import { join } from 'node:path'
import { stageEngine } from './stage-engine.ts'
import { REPO_ROOT } from './versions.ts'

// electron-builder ignores --<arch> for targets that list their own archs (all of ours do), unless
// the targets are named on the command line too. These mirror electron-builder.yml.
const TARGETS: Record<string, string[]> = { win32: ['--win', 'nsis', 'zip'], darwin: ['--mac', 'dmg', 'zip'], linux: ['--linux', 'AppImage', 'deb', 'rpm'] }

const sh = process.platform === 'win32'
const run = (cmd: string, args: string[], cwd = REPO_ROOT) => execFileSync(cmd, args, { cwd, stdio: 'inherit', shell: sh })
run('cargo', ['build', '--release', '--locked', '--manifest-path', 'engine/Cargo.toml', '-p', 'aurora-engine'])
stageEngine({ src: join(REPO_ROOT, 'engine/target/release', sh ? 'aurora-engine.exe' : 'aurora-engine'), appRoot: join(REPO_ROOT, 'app'), platform: process.platform, arch: process.arch })
run('npm', ['run', 'dist', '--', '--publish', 'never', ...TARGETS[process.platform], `--${process.arch}`], join(REPO_ROOT, 'app'))
