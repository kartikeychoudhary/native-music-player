import { describe, expect, it } from 'vitest'
import { join } from 'node:path'
import { enginePath } from '../src/main/engine/enginePath'

describe('enginePath', () => {
  const base = { resourcesPath: '/res', appPath: '/repo/app', arch: 'x64', env: {} }
  it('uses the cargo target dir in dev', () => {
    expect(enginePath({ ...base, isPackaged: false, platform: 'linux' })).toBe(join('/repo/app', '..', 'engine', 'target', 'debug', 'aurora-engine'))
  })
  it('uses resources/bin/<platform>-<arch> when packaged, with .exe on Windows', () => {
    expect(enginePath({ ...base, isPackaged: true, platform: 'win32' })).toBe(join('/res', 'bin', 'win32-x64', 'aurora-engine.exe'))
  })
  it('honours AURORA_ENGINE_PATH', () => {
    expect(enginePath({ ...base, isPackaged: true, platform: 'darwin', env: { AURORA_ENGINE_PATH: '/x/engine' } })).toBe('/x/engine')
  })
})
