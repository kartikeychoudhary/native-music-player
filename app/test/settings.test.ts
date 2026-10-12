import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { boundsVisible, loadSettings, SettingsStore } from '../src/main/settings'
import { DEFAULT_SETTINGS, mergeSettings } from '../src/shared/settings'

const dir = () => mkdtempSync(join(tmpdir(), 'aurora-settings-'))

describe('settings', () => {
  afterEach(() => vi.useRealTimers())

  it('merges only valid known fields', () => {
    const s = mergeSettings(DEFAULT_SETTINGS, { sidebarExpanded: true, lastView: '/albums', extra: 1, windowBounds: { x: 1, y: 2, width: 900, height: 700, maximized: false } })
    expect(s).toEqual({ ...DEFAULT_SETTINGS, sidebarExpanded: true, lastView: '/albums', windowBounds: { x: 1, y: 2, width: 900, height: 700, maximized: false } })
    expect(mergeSettings(DEFAULT_SETTINGS, { miniBounds: { x: 5, y: -3 }, accentTint: true })).toMatchObject({ miniBounds: { x: 5, y: -3 }, accentTint: true })
    expect(mergeSettings(DEFAULT_SETTINGS, { miniBounds: { x: 'a' }, accentTint: 'yes' })).toMatchObject({ miniBounds: null, accentTint: false })
    expect(mergeSettings(DEFAULT_SETTINGS, { sidebarExpanded: 'yes', lastView: 'http://evil', windowBounds: { width: 'big' } })).toEqual(DEFAULT_SETTINGS)
    expect(mergeSettings(DEFAULT_SETTINGS, { lastView: '/' + 'a'.repeat(300) }).lastView).toBe('/')
    expect(mergeSettings(DEFAULT_SETTINGS, null)).toEqual(DEFAULT_SETTINGS)
  })

  it('loads defaults for missing or corrupt files', () => {
    const d = dir()
    expect(loadSettings(join(d, 'none.json'))).toEqual(DEFAULT_SETTINGS)
    writeFileSync(join(d, 'bad.json'), '{"sidebarExpanded": tr')
    expect(loadSettings(join(d, 'bad.json'))).toEqual(DEFAULT_SETTINGS)
  })

  it('saves updates after a debounce and on flush', () => {
    vi.useFakeTimers()
    const file = join(dir(), 'settings.json')
    const store = new SettingsStore(file)
    expect(store.update({ sidebarExpanded: true }).sidebarExpanded).toBe(true)
    expect(() => readFileSync(file)).toThrow()
    vi.advanceTimersByTime(300)
    expect(JSON.parse(readFileSync(file, 'utf8')).sidebarExpanded).toBe(true)
    store.update({ lastView: '/artists' })
    store.flush()
    expect(loadSettings(file).lastView).toBe('/artists')
  })

  it('only restores window bounds that are on a screen', () => {
    const screens = [{ x: 0, y: 0, width: 1920, height: 1040 }]
    expect(boundsVisible({ x: 100, y: 100, width: 1280, height: 800, maximized: false }, screens)).toBe(true)
    expect(boundsVisible({ x: 5000, y: 100, width: 1280, height: 800, maximized: false }, screens)).toBe(false)
    expect(boundsVisible({ x: -1200, y: 0, width: 1280, height: 800, maximized: false }, screens)).toBe(true) // 80 px still visible
  })
})
