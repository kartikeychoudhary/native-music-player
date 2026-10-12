import { describe, expect, it } from 'vitest'
import { DEV_BUILD, parseBuildInfo, readBuildInfo } from '../src/main/buildInfo'

describe('build info', () => {
  it('parses what build-info.ts writes and ignores junk', () => {
    expect(parseBuildInfo({ variant: 'nofdk', macSigned: true, commit: 'abc' })).toEqual({ variant: 'nofdk', macSigned: true, commit: 'abc' })
    expect(parseBuildInfo({ variant: 'weird', macSigned: 'yes', commit: 5 })).toEqual(DEV_BUILD)
    expect(parseBuildInfo(null)).toEqual(DEV_BUILD)
  })
  it('reads resources/build-info.json only when packaged, and survives a missing file', () => {
    const read = (p: string) => { if (p.replace(/\\/g, '/') === '/res/build-info.json') return '{"variant":"nofdk","macSigned":false,"commit":null}'; throw new Error('ENOENT') }
    expect(readBuildInfo({ isPackaged: true, resourcesPath: '/res', read }).variant).toBe('nofdk')
    expect(readBuildInfo({ isPackaged: false, resourcesPath: '/res', read })).toEqual(DEV_BUILD)
    expect(readBuildInfo({ isPackaged: true, resourcesPath: '/elsewhere', read })).toEqual(DEV_BUILD)
  })
})
