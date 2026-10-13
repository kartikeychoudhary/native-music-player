import { describe, expect, it } from 'vitest'
import { builderArgs, limitArches } from '../build-tools/package-ci.ts'

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
  it('the apply pass builds exactly what an unsigned run builds', () => {
    for (const variant of ['standard', 'nofdk'] as const) {
      expect(builderArgs({ platform: 'win32', variant, signed: false, signMode: 'apply' })).toEqual(builderArgs({ platform: 'win32', variant, signed: false }))
    }
  })
  it('limits a local run to the requested arches, naming the targets so electron-builder honours the arch flags', () => {
    const std = builderArgs({ platform: 'win32', variant: 'standard', signed: false })
    expect(limitArches(std, 'win32', ['x64', 'arm64'])).toEqual(std)
    expect(limitArches(std, 'win32', ['x64'])).toEqual(['--win', 'nsis', 'zip', '--x64', '--publish', 'never'])
    expect(limitArches(builderArgs({ platform: 'win32', variant: 'standard', signed: false, signMode: 'collect' }), 'win32', ['x64'])).toEqual(['--win', 'nsis', '--x64', '--publish', 'never'])
    expect(limitArches(builderArgs({ platform: 'win32', variant: 'nofdk', signed: false }), 'win32', ['x64']).slice(0, 3)).toEqual(['--win', 'zip', '--x64'])
  })
})
