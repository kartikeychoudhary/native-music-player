// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { MiniApp } from '../src/renderer/mini/MiniApp'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, resetStores } from './helpers'

afterEach(cleanup)

describe('MiniApp', () => {
  beforeEach(resetStores)

  it('shows the current track and drives playback and windows', () => {
    const { api } = fakeApi()
    const toggle = vi.fn(), next = vi.fn(), prev = vi.fn()
    usePlayer.setState({
      api, toggle, next, prev,
      state: { ...usePlayer.getState().state, status: 'playing', positionMs: 30_000, durationMs: 120_000, track: { path: '/m/a.flac', title: 'Dawn', artist: 'Alpha', album: 'First Light', durationMs: 120_000, trackId: 1, artworkId: null } },
    })
    render(<MiniApp api={api} />)
    expect(screen.getByText('Dawn')).toBeTruthy()
    expect(screen.getByText('Alpha')).toBeTruthy()
    expect((screen.getByRole('progressbar') as HTMLProgressElement).value).toBeGreaterThanOrEqual(25)
    fireEvent.click(screen.getByRole('button', { name: 'Pause' }))
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    fireEvent.click(screen.getByRole('button', { name: 'Previous' }))
    expect([toggle, next, prev].every((f) => f.mock.calls.length === 1)).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Open Aurora' }))
    expect(api.showMain).toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Close mini-player' }))
    expect(api.toggleMini).toHaveBeenCalledTimes(1)
    fireEvent.keyDown(window, { key: 'm', ctrlKey: true })
    expect(api.toggleMini).toHaveBeenCalledTimes(2)
  })

  it('says when nothing is playing', () => {
    const { api } = fakeApi()
    usePlayer.setState({ api })
    render(<MiniApp api={api} />)
    expect(screen.getByText('Nothing playing')).toBeTruthy()
  })
})
