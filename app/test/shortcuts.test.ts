// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { handleShortcut } from '../src/renderer/lib/shortcuts'
import { usePlayer } from '../src/renderer/stores/player'

function press(target: Element, init: KeyboardEventInit & { key: string }) {
  const e = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })
  Object.defineProperty(e, 'target', { value: target })
  const handled = handleShortcut(e, usePlayer.getState(), 1000)
  return { e, handled }
}

describe('keyboard shortcuts', () => {
  const toggle = vi.fn(async () => {}), next = vi.fn(async () => {}), prev = vi.fn(async () => {}), seek = vi.fn(async () => {})
  beforeEach(() => {
    vi.clearAllMocks()
    usePlayer.setState(usePlayer.getInitialState())
    usePlayer.setState({ toggle, next, prev, seek, stamp: 1000, state: { ...usePlayer.getState().state, status: 'paused', positionMs: 10_000, durationMs: 12_000 } })
  })
  afterEach(() => { document.body.innerHTML = '' })

  it('Space on the body toggles playback and is prevented', () => {
    const { e } = press(document.body, { key: ' ', code: 'Space' })
    expect(toggle).toHaveBeenCalledTimes(1)
    expect(e.defaultPrevented).toBe(true)
  })

  it('Space on a focused button leaves it to the button', () => {
    const b = document.createElement('button'); document.body.append(b)
    const { e } = press(b, { key: ' ', code: 'Space' })
    expect(toggle).not.toHaveBeenCalled()
    expect(e.defaultPrevented).toBe(false)
  })

  it('Space on the Bit-perfect switch leaves it to the switch', () => {
    const sw = document.createElement('span'); sw.setAttribute('role', 'switch'); document.body.append(sw)
    const inner = document.createElement('i'); sw.append(inner)
    press(inner, { key: ' ', code: 'Space' })
    expect(toggle).not.toHaveBeenCalled()
  })

  it('Shift+Right / Shift+Left skip tracks', () => {
    press(document.body, { key: 'ArrowRight', shiftKey: true })
    press(document.body, { key: 'ArrowLeft', shiftKey: true })
    expect(next).toHaveBeenCalledTimes(1)
    expect(prev).toHaveBeenCalledTimes(1)
  })

  it('Right seeks +5s from the live position, clamped to the duration', () => {
    press(document.body, { key: 'ArrowRight' })
    expect(seek).toHaveBeenLastCalledWith(12_000)
  })

  it('Left seeks -5s and clamps at zero', () => {
    usePlayer.setState({ state: { ...usePlayer.getState().state, positionMs: 2000 } })
    press(document.body, { key: 'ArrowLeft' })
    expect(seek).toHaveBeenLastCalledWith(0)
  })

  it('seeks from the interpolated position while playing', () => {
    usePlayer.setState({ state: { ...usePlayer.getState().state, status: 'playing', positionMs: 1000, durationMs: 60_000 }, stamp: 0 })
    press(document.body, { key: 'ArrowRight' })
    expect(seek).toHaveBeenLastCalledWith(7000)
  })

  it('ignores modified keys and already-handled events', () => {
    press(document.body, { key: 'ArrowRight', altKey: true })
    press(document.body, { key: ' ', code: 'Space', ctrlKey: true })
    press(document.body, { key: ' ', code: 'Space', metaKey: true })
    const e = new KeyboardEvent('keydown', { key: ' ', code: 'Space', cancelable: true, bubbles: true })
    e.preventDefault()
    expect(handleShortcut(e, usePlayer.getState(), 0)).toBe(false)
    expect(toggle).not.toHaveBeenCalled()
    expect(seek).not.toHaveBeenCalled()
    expect(next).not.toHaveBeenCalled()
  })
})
