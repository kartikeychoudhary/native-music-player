// @vitest-environment jsdom
import { act, cleanup, fireEvent, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)
beforeEach(() => { vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null) })

const playingState = {
  status: 'playing', queueIndex: 0, queueItemId: 1, positionMs: 1000, durationMs: 61_000, volume: 1, mode: 'processed', deviceId: null, shuffle: false, repeat: 'off',
  track: { path: '/m/Dawn.flac', title: 'Dawn', artist: 'Alpha', album: 'Light', durationMs: 61_000, trackId: 7, artworkId: 5 },
}

const api = (overrides: Record<string, unknown> = {}) => fakeApi({
  'player.state': () => playingState,
  'library.artwork': () => ({ path: '/art/5.webp', palette: { dominant: '#334455', vibrant: '#4FC3F7', muted: '#2B3A4A' } }),
  'library.track': () => ({ track: track(7, 'Dawn'), artists: [], genres: [], composer: 'C. Writer', trackTotal: 10, discTotal: 1, replayGain: {}, musicbrainz: { recording: null, release: null, artist: null }, hasLyrics: true, sizeBytes: 1, mtime: 0, gapless: null }),
  'lyrics.get': () => ({ synced: false, lines: [{ text: 'Hello lyrics' }] }),
  'queue.get': () => ({ items: [{ id: 1, path: '/m/Dawn.flac', title: 'Dawn', trackId: 7, artist: 'Alpha', album: 'Light', artworkId: 5, durationMs: 61_000 }], current: 0 }),
  ...overrides,
} as never)

describe('Now Playing', () => {
  it('shows the track over its palette with lyrics, queue, signal and credits tabs', async () => {
    const { api: a } = api()
    await renderAt('/now-playing', a)
    expect(await screen.findByRole('heading', { name: 'Dawn' })).toBeTruthy()
    await vi.waitFor(() => expect(screen.getByTestId('now-playing').style.background.toLowerCase()).toContain('#2b3a4a')) // jsdom keeps hex in a gradient that uses var()
    expect(await screen.findByText('Hello lyrics')).toBeTruthy()
    expect(screen.getByRole('img', { name: 'Spectrum' })).toBeTruthy()
    const tabs = screen.getByRole('tablist')
    fireEvent.click(within(tabs).getByRole('tab', { name: 'Queue' }))
    expect(await screen.findByRole('list', { name: 'Queue' })).toBeTruthy()
    fireEvent.click(within(tabs).getByRole('tab', { name: 'Credits' }))
    expect(await screen.findByText('C. Writer')).toBeTruthy()
    fireEvent.click(within(tabs).getByRole('tab', { name: 'Signal path' }))
    expect(within(tabs).getByRole('tab', { name: 'Signal path' }).getAttribute('aria-selected')).toBe('true')
  })

  it('Ctrl/Cmd+L opens the lyrics and closes them again; the bar buttons work', async () => {
    const { api: a } = api()
    const { router } = await renderAt('/', a)
    await act(async () => { fireEvent.keyDown(window, { key: 'l', ctrlKey: true }) })
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/now-playing'))
    expect(screen.getByRole('tab', { name: 'Lyrics' }).getAttribute('aria-selected')).toBe('true')
    expect(screen.getByRole('button', { name: 'Lyrics' }).getAttribute('aria-pressed')).toBe('true')
    await act(async () => { fireEvent.keyDown(window, { key: 'l', ctrlKey: true }) })
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/'))
    fireEvent.click(screen.getByRole('button', { name: 'Mini-player' }))
    expect(a.toggleMini).toHaveBeenCalled()
    expect(usePlayer.getState().state.track?.title).toBe('Dawn')
  })

  it("does not keep the previous track's lyrics while the next track's lyrics load", async () => {
    let release: (v: unknown) => void = () => {}
    const pending = new Promise((r) => { release = r })
    const { api: a } = api({
      'lyrics.get': (p: { trackId: number }) => (p.trackId === 7 ? { synced: false, lines: [{ text: 'Hello lyrics' }] } : pending),
    })
    await renderAt('/now-playing', a)
    expect(await screen.findByText('Hello lyrics')).toBeTruthy()
    await act(async () => {
      usePlayer.setState({ state: { ...playingState, track: { ...playingState.track, title: 'Dusk', trackId: 8 } } as never })
    })
    expect(await screen.findByRole('heading', { name: 'Dusk' })).toBeTruthy()
    expect(screen.queryByText('Hello lyrics')).toBeNull()
    await act(async () => { release({ synced: false, lines: [{ text: 'Second song' }] }) })
    expect(await screen.findByText('Second song')).toBeTruthy()
    expect(screen.queryByText('Hello lyrics')).toBeNull()
  })

  it('says credits (not lyrics) are library-only for a track outside the library', async () => {
    const { api: a } = api({
      'player.state': () => ({ ...playingState, track: { ...playingState.track, trackId: null } }),
    })
    await renderAt('/now-playing', a)
    expect(await screen.findByRole('heading', { name: 'Dawn' })).toBeTruthy()
    fireEvent.click(within(screen.getByRole('tablist')).getByRole('tab', { name: 'Credits' }))
    expect(await screen.findByText('Credits are available for tracks in your library.')).toBeTruthy()
    expect(screen.queryByText('Lyrics are available for tracks in your library.')).toBeNull()
  })
})
