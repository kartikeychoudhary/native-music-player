import { mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { stageEngine } from '../build-tools/stage-engine.ts'

describe('stage engine', () => {
  it('copies the binary to resources/engine/<platform>-<arch>/ and makes it executable', () => {
    const dir = mkdtempSync(join(tmpdir(), 'aurora-stage-'))
    const src = join(dir, 'aurora-engine'); writeFileSync(src, 'bin')
    const dest = stageEngine({ src, appRoot: dir, platform: 'linux', arch: 'arm64' })
    expect(dest.replace(/\\/g, '/')).toBe(`${dir.replace(/\\/g, '/')}/resources/engine/linux-arm64/aurora-engine`)
    expect(readFileSync(dest, 'utf8')).toBe('bin')
    if (process.platform !== 'win32') expect(statSync(dest).mode & 0o111).toBe(0o111)
  })
  it('names the Windows binary .exe', () => {
    const dir = mkdtempSync(join(tmpdir(), 'aurora-stage-'))
    const src = join(dir, 'aurora-engine.exe'); writeFileSync(src, 'bin')
    expect(stageEngine({ src, appRoot: dir, platform: 'win32', arch: 'x64' })).toMatch(/win32-x64[\\/]aurora-engine\.exe$/)
  })
})
