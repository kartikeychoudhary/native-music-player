// @vitest-environment jsdom
import { cleanup, fireEvent, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, renderAt } from './helpers'

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

describe('PlayerBar (M2)', () => {
  const playing = { path: '/m/a.flac', title: 'Dawn', durationMs: 60_000, trackId: 7, artist: 'Alpha', album: 'Light', artworkId: 3 }

  it('shows artwork, artist and the library actions for the current track', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, status: 'playing', track: playing } }))
    expect(screen.getByRole('img', { name: 'Dawn' }).getAttribute('src')).toBe('aurora-art://art/3/64')
    expect(screen.getByText('Alpha')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Add to playlist' }))
    expect(screen.getByRole('menuitem', { name: 'New playlist…' })).toBeTruthy()
  })

  it('disables add-to-playlist for files outside the library', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, track: { ...playing, trackId: null, artworkId: null } } }))
    expect((screen.getByRole('button', { name: 'Add to playlist' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('toggles shuffle and cycles repeat', async () => {
    const { api, calls } = fakeApi()
    await renderAt('/', api)
    const shuffle = screen.getByRole('button', { name: 'Shuffle' })
    expect(shuffle.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(shuffle)
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setShuffle', { shuffle: true }]))
    fireEvent.click(screen.getByRole('button', { name: 'Repeat: off' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setRepeat', { repeat: 'all' }]))
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, shuffle: true, repeat: 'all' } }))
    expect(screen.getByRole('button', { name: 'Shuffle' }).getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(screen.getByRole('button', { name: 'Repeat: all' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setRepeat', { repeat: 'one' }]))
  })

  it('describes the seek position for screen readers', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, status: 'paused', positionMs: 65_000, durationMs: 252_000, track: playing } }))
    expect(screen.getByRole('slider', { name: 'Seek' }).getAttribute('aria-valuetext')).toBe('1:05 of 4:12')
  })
})
