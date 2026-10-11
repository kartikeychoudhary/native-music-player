import { describe, expect, it } from 'vitest'
import { engineArgs, resolveDataDir, usesSingleInstanceLock } from '../src/main/engineArgs'

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
  it('takes the single-instance lock except in unpackaged test runs with their own data dir', () => {
    const env = { AURORA_DATA_DIR: '/tmp/test-data' }
    expect(usesSingleInstanceLock({ isPackaged: true, env })).toBe(true)
    expect(usesSingleInstanceLock({ isPackaged: false, env: {} })).toBe(true)
    expect(usesSingleInstanceLock({ isPackaged: false, env })).toBe(false)
  })
})
