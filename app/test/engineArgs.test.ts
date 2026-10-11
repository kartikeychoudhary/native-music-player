import { describe, expect, it } from 'vitest'
import { engineArgs, resolveDataDir } from '../src/main/engineArgs'

describe('engine arguments', () => {
  it('passes the data dir to rpc mode', () => {
    expect(engineArgs('/u/Aurora')).toEqual(['rpc', '--data-dir', '/u/Aurora'])
  })
  it('honours AURORA_DATA_DIR only in unpackaged builds', () => {
    const env = { AURORA_DATA_DIR: '/tmp/test-data' }
    expect(resolveDataDir({ isPackaged: false, env, userData: '/u/Aurora' })).toBe('/tmp/test-data')
    expect(resolveDataDir({ isPackaged: true, env, userData: '/u/Aurora' })).toBe('/u/Aurora')
    expect(resolveDataDir({ isPackaged: false, env: {}, userData: '/u/Aurora' })).toBe('/u/Aurora')
  })
})
