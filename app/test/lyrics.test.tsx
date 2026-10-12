// @vitest-environment jsdom
import { act, cleanup, fireEvent, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { LyricsView } from '../src/renderer/components/LyricsView'
import { activeLine, activeWord } from '../src/renderer/lib/lyrics'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, renderInRouter, resetStores } from './helpers'

afterEach(cleanup)

const synced = { synced: true, lines: [{ tMs: 1000, text: 'One' }, { tMs: 3000, text: 'Two', words: [{ tMs: 3000, text: 'Tw' }, { tMs: 3500, text: 'o' }] }, { tMs: 6000, text: 'Three' }] }

describe('lyrics helpers', () => {
  it('finds the line and word being sung', () => {
    expect(activeLine(synced.lines, 0)).toBe(-1)
    expect(activeLine(synced.lines, 1000)).toBe(0)
    expect(activeLine(synced.lines, 2999)).toBe(0)
    expect(activeLine(synced.lines, 3000)).toBe(1)
    expect(activeLine(synced.lines, 99_999)).toBe(2)
    expect(activeLine([{ text: 'plain' }], 5000)).toBe(-1)
    expect(activeWord(synced.lines[1].words!, 3400)).toBe(0)
    expect(activeWord(synced.lines[1].words!, 3600)).toBe(1)
  })
})

describe('LyricsView', () => {
  const playing = (positionMs: number, trackId: number | null = 7) => ({
    ...usePlayer.getInitialState().state, status: 'paused' as const, positionMs, durationMs: 10_000,
    track: { path: '/m/a.flac', title: 'A', artist: null, album: null, durationMs: 10_000, trackId, artworkId: null },
  })

  it('marks the current synced line and seeks when a line is clicked', async () => {
    resetStores()
    const { api, calls } = fakeApi({ 'lyrics.get': () => synced } as never)
    const seek = vi.fn()
    usePlayer.setState({ api, seek, engine: { state: 'ready' }, state: playing(3200) })
    await renderInRouter(<LyricsView trackId={7} />)
    expect(calls).toContainEqual(['lyrics.get', { trackId: 7 }])
    await screen.findByText('Three')
    // "Two" is split into word spans, so check the marked line's whole text.
    expect(document.querySelector('[aria-current="true"]')?.textContent).toBe('Two')
    expect(document.querySelectorAll('[data-sung]').length).toBe(1) // "Tw" sung, "o" not yet
    await act(async () => { usePlayer.setState({ state: playing(6500), stamp: performance.now() }) })
    expect(screen.getByText('Three').closest('[aria-current="true"]')).toBeTruthy()
    fireEvent.click(screen.getByText('One'))
    expect(seek).toHaveBeenCalledWith(1000)
  })

  it('shows plain lyrics, missing lyrics and non-library tracks', async () => {
    resetStores()
    const { api } = fakeApi({ 'lyrics.get': () => ({ synced: false, lines: [{ text: 'Verse' }, { text: '' }, { text: 'Chorus' }] }) } as never)
    usePlayer.setState({ api, engine: { state: 'ready' }, state: playing(0) })
    await renderInRouter(<LyricsView trackId={7} />)
    expect(await screen.findByText('Chorus')).toBeTruthy()
    expect(document.querySelector('[aria-current="true"]')).toBeNull()
    cleanup()
    const none = fakeApi({ 'lyrics.get': () => null } as never)
    usePlayer.setState({ api: none.api, engine: { state: 'ready' }, state: playing(0) })
    await renderInRouter(<LyricsView trackId={7} />)
    expect(await screen.findByText(/No lyrics for this track/)).toBeTruthy()
    cleanup()
    usePlayer.setState({ api: none.api, engine: { state: 'ready' }, state: playing(0, null) })
    await renderInRouter(<LyricsView trackId={null} />)
    expect(screen.getByText(/available for tracks in your library/)).toBeTruthy()
  })
})
