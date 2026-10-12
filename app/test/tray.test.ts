import { describe, expect, it, vi } from 'vitest'
vi.mock('electron', () => ({ Menu: {}, Tray: class {}, nativeImage: {} }))
import { trayMenuTemplate, trayTooltip, type TrayState } from '../src/main/tray'
import { trayIconPixels } from '../src/main/trayIcon'

const base: TrayState = { playing: false, mainVisible: true, miniOpen: false, title: null, artist: null }

describe('tray', () => {
  it('tray_menu_reflects_state_and_drives_actions', () => {
    const a = { toggle: vi.fn(), next: vi.fn(), prev: vi.fn(), toggleMain: vi.fn(), toggleMini: vi.fn(), quit: vi.fn() }
    const labels = (s: TrayState) => trayMenuTemplate(s, a).filter((i) => i.type !== 'separator').map((i) => i.label)
    expect(labels(base)).toEqual(['Play', 'Next', 'Previous', 'Hide Aurora', 'Mini-player', 'Quit Aurora'])
    expect(labels({ ...base, playing: true, mainVisible: false, miniOpen: true })).toEqual(['Pause', 'Next', 'Previous', 'Show Aurora', 'Close mini-player', 'Quit Aurora'])
    const items = trayMenuTemplate(base, a)
    for (const i of items) (i.click as (() => void) | undefined)?.()
    expect([a.toggle, a.next, a.prev, a.toggleMain, a.toggleMini, a.quit].every((f) => f.mock.calls.length === 1)).toBe(true)
  })

  it('tooltip names the track', () => {
    expect(trayTooltip(base)).toBe('Aurora')
    expect(trayTooltip({ ...base, title: 'Dawn', artist: 'Alpha' })).toBe('Dawn — Alpha')
    expect(trayTooltip({ ...base, title: 'Dawn' })).toBe('Dawn')
  })

  it('draws an amber square with a dark play triangle and transparent corners', () => {
    for (const size of [16, 32]) {
      const px = trayIconPixels(size)
      expect(px.length).toBe(size * size * 4)
      const at = (x: number, y: number) => Array.from(px.subarray((y * size + x) * 4, (y * size + x) * 4 + 4))
      expect(at(0, 0)[3]).toBe(0) // transparent corner (rounded)
      expect(at(Math.floor(size * 0.15), Math.floor(size / 2))).toEqual([0x47, 0xb5, 0xff, 0xff]) // amber, BGRA
      expect(at(Math.floor(size / 2), Math.floor(size / 2))).toEqual([0x1a, 0x15, 0x17, 0xff]) // dark triangle centre
    }
  })
})
