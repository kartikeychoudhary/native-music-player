// @vitest-environment jsdom
import { cleanup, fireEvent, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { renderAt } from './helpers'

afterEach(cleanup)

describe('PlayerBar', () => {
  beforeEach(() => usePlayer.setState(usePlayer.getInitialState()))

  it('toggles playback through the store', async () => {
    await renderAt('/')
    const toggle = vi.fn(async () => {})
    act(() => usePlayer.setState({ toggle, state: { ...usePlayer.getState().state, status: 'paused', track: { path: '/a.flac', title: 'a', durationMs: 1000, trackId: null, artist: null, album: null, artworkId: null } } }))
    fireEvent.click(screen.getByRole('button', { name: 'Play' }))
    expect(toggle).toHaveBeenCalled()
    expect(screen.getByTestId('now-title').textContent).toBe('a')
  })

  it('disables software volume in bit-perfect mode', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, mode: 'bitPerfect' } }))
    const vol = screen.getByRole('slider', { name: 'Volume' }) as HTMLInputElement
    expect(vol.disabled).toBe(true)
    expect(vol.title).toBe('Bit-perfect: use your system or DAC volume')
    expect(screen.getByRole('switch', { name: 'Bit-perfect' }).getAttribute('aria-checked')).toBe('true')
  })
})
