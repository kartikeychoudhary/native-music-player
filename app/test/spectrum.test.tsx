// @vitest-environment jsdom
import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { Spectrum } from '../src/renderer/components/Spectrum'
import { smooth } from '../src/renderer/lib/spectrum'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, resetStores } from './helpers'

afterEach(cleanup)

describe('smooth', () => {
  it('rises instantly and falls gradually unless motion is reduced', () => {
    const prev = new Float32Array([0.5, 0.5])
    const next = new Uint8Array([255, 0])
    const out = smooth(prev, next, false)
    expect(out[0]).toBe(1)
    expect(out[1]).toBeCloseTo(0.44, 5)
    expect(Array.from(smooth(prev, next, true))).toEqual([1, 0])
  })
})

describe('Spectrum', () => {
  beforeEach(() => {
    resetStores()
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  })

  it('spectrum_unsubscribes_when_hidden_or_unmounted', () => {
    const { api } = fakeApi()
    let deliver: ((b: Uint8Array) => void) | null = null
    api.onSpectrum.mockImplementation((cb: (b: Uint8Array) => void) => { deliver = cb; return () => { deliver = null } })
    usePlayer.setState({ api })
    const { unmount } = render(<Spectrum />)
    expect(api.setViz).toHaveBeenLastCalledWith(true)
    act(() => { deliver!(new Uint8Array(64).fill(128)) })
    expect(screen.getByRole('img', { name: 'Spectrum' }).dataset.frames).toBe('1')
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true })
    document.dispatchEvent(new Event('visibilitychange'))
    expect(api.setViz).toHaveBeenLastCalledWith(false)
    Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true })
    document.dispatchEvent(new Event('visibilitychange'))
    expect(api.setViz).toHaveBeenLastCalledWith(true)
    unmount()
    expect(api.setViz).toHaveBeenLastCalledWith(false)
    expect(deliver).toBeNull()
  })
})
