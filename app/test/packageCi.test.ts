import { describe, expect, it } from 'vitest'
import { builderArgs } from '../build-tools/package-ci.ts'

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
})
