// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PlayerBar } from '../src/renderer/components/PlayerBar'
import { usePlayer } from '../src/renderer/stores/player'

afterEach(cleanup)

describe('PlayerBar', () => {
  beforeEach(() => usePlayer.setState(usePlayer.getInitialState()))

  it('toggles playback through the store', () => {
    const toggle = vi.fn(async () => {})
    usePlayer.setState({ toggle, state: { ...usePlayer.getState().state, status: 'paused', track: { path: '/a.flac', title: 'a', durationMs: 1000 } } })
    render(<PlayerBar />)
    fireEvent.click(screen.getByRole('button', { name: 'Play' }))
    expect(toggle).toHaveBeenCalled()
    expect(screen.getByTestId('now-title').textContent).toBe('a')
  })

  it('disables software volume in bit-perfect mode', () => {
    usePlayer.setState({ state: { ...usePlayer.getState().state, mode: 'bitPerfect' } })
    render(<PlayerBar />)
    const vol = screen.getByRole('slider', { name: 'Volume' }) as HTMLInputElement
    expect(vol.disabled).toBe(true)
    expect(vol.title).toBe('Bit-perfect: use your system or DAC volume')
    expect(screen.getByRole('switch', { name: 'Bit-perfect' }).getAttribute('aria-checked')).toBe('true')
  })
})
