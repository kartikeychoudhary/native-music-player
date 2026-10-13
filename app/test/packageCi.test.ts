import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { SIGNPATH_PUBLISHER, builderArgs, builderEnv, engineToStage, limitArches, resetToSign } from '../build-tools/package-ci.ts'

describe('CI packaging arguments', () => {
  it('builds both arches of the standard variant without publishing', () => {
    expect(builderArgs({ platform: 'win32', variant: 'standard', signed: false })).toEqual(['--win', '--x64', '--arm64', '--publish', 'never'])
  })
  it('ad-hoc signs unsigned macOS builds and leaves signed ones to the certificate', () => {
    expect(builderArgs({ platform: 'darwin', variant: 'standard', signed: false })).toContain('-c.mac.identity=-')
    expect(builderArgs({ platform: 'darwin', variant: 'standard', signed: true })).not.toContain('-c.mac.identity=-')
  })
  it('builds the no-fdk variant as portable archives in their own folder', () => {
    const a = builderArgs({ platform: 'linux', variant: 'nofdk', signed: false })
    expect(a).toEqual(expect.arrayContaining(['--linux', 'tar.gz', '-c.directories.output=dist-nofdk', '-c.artifactName=${productName}-${version}-${os}-${arch}-nofdk.${ext}']))
    expect(builderArgs({ platform: 'win32', variant: 'nofdk', signed: false })).toEqual(expect.arrayContaining(['--win', 'zip']))
    expect(builderArgs({ platform: 'darwin', variant: 'nofdk', signed: false })).toEqual(expect.arrayContaining(['--mac', 'zip']))
  })
  it('the collect pass builds only the standard NSIS installer (both arches), to gather every file to sign', () => {
    expect(builderArgs({ platform: 'win32', variant: 'standard', signed: false, signMode: 'collect' })).toEqual(['--win', 'nsis', '--x64', '--arm64', '--publish', 'never'])
  })
  it('the apply pass builds what an unsigned run builds, plus the SignPath publisher for electron-updater', () => {
    const publisher = `-c.win.signtoolOptions.publisherName=${SIGNPATH_PUBLISHER}`
    expect(SIGNPATH_PUBLISHER).toBe('SignPath Foundation')
    for (const variant of ['standard', 'nofdk'] as const) {
      expect(builderArgs({ platform: 'win32', variant, signed: false, signMode: 'apply' })).toEqual([...builderArgs({ platform: 'win32', variant, signed: false }), publisher])
      expect(builderArgs({ platform: 'win32', variant, signed: false })).not.toContain(publisher)
    }
    expect(builderArgs({ platform: 'win32', variant: 'standard', signed: false, signMode: 'collect' })).not.toContain(publisher)
  })
  it('gives electron-builder the sign mode and dir, with its build cache off, only when signing', () => {
    expect(builderEnv(undefined, '', { PATH: 'p' })).toBeUndefined()
    expect(builderEnv('collect', 'C:/s', { PATH: 'p' })).toEqual({ PATH: 'p', AURORA_SIGN_MODE: 'collect', AURORA_SIGN_DIR: 'C:/s', ELECTRON_BUILDER_DISABLE_BUILD_CACHE: 'true' })
    expect(builderEnv('apply', 'C:/s', {})).toMatchObject({ AURORA_SIGN_MODE: 'apply', ELECTRON_BUILDER_DISABLE_BUILD_CACHE: 'true' })
  })
  it('empties to-sign before a collect pass, so nothing stale is sent for signing', () => {
    const dir = mkdtempSync(join(tmpdir(), 'aurora-sign-'))
    try {
      mkdirSync(join(dir, 'to-sign'))
      writeFileSync(join(dir, 'to-sign', 'stale-old.exe'), 'x')
      resetToSign(dir)
      expect(readdirSync(join(dir, 'to-sign'))).toEqual([])
      rmSync(join(dir, 'to-sign'), { recursive: true })
      resetToSign(dir)
      expect(existsSync(join(dir, 'to-sign'))).toBe(true)
    } finally { rmSync(dir, { recursive: true, force: true }) }
  })
  it('limits a local run to the requested arches, naming the targets so electron-builder honours the arch flags', () => {
    const std = builderArgs({ platform: 'win32', variant: 'standard', signed: false })
    expect(limitArches(std, 'win32', ['x64', 'arm64'])).toEqual(std)
    expect(limitArches(std, 'win32', ['x64'])).toEqual(['--win', 'nsis', 'zip', '--x64', '--publish', 'never'])
    expect(limitArches(builderArgs({ platform: 'win32', variant: 'standard', signed: false, signMode: 'collect' }), 'win32', ['x64'])).toEqual(['--win', 'nsis', '--x64', '--publish', 'never'])
    expect(limitArches(builderArgs({ platform: 'win32', variant: 'nofdk', signed: false }), 'win32', ['x64']).slice(0, 3)).toEqual(['--win', 'zip', '--x64'])
  })
  it('stages the unsigned engine in the apply pass, so the sign hook can swap in its signed copy by hash', () => {
    const name = 'engines/x64/standard/aurora-engine.exe'
    const src = 'C:/repo/engines/x64/standard/aurora-engine.exe'
    expect(engineToStage({ src, name, signMode: 'apply', signedCopy: () => 'C:/repo/sign/signed/abc-aurora-engine.exe' })).toBe(src)
    expect(() => engineToStage({ src, name, signMode: 'apply', signedCopy: () => null })).toThrow('no signed copy of engines/x64/standard/aurora-engine.exe')
    expect(engineToStage({ src, name, signedCopy: () => { throw new Error('not consulted') } })).toBe(src)
  })
})
