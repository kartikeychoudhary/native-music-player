// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TrackTable } from '../src/renderer/components/TrackTable'
import { ContextMenu } from '../src/renderer/components/ContextMenu'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, renderInRouter, resetStores, track } from './helpers'

afterEach(cleanup)

describe('TrackTable', () => {
  beforeEach(() => { resetStores(); usePlayer.setState({ api: fakeApi().api }) })

  const rows = [track(1, 'Dawn'), track(2, 'Noon', { unplayable: 'DECODE_FAILED' }), track(3, 'Dusk', { missing: true })]
  const table = (onPlay = vi.fn()) => renderInRouter(<><TrackTable label="Tracks" count={3} row={(i) => rows[i]} onPlay={onPlay} /><ContextMenu /></>)

  it('renders rows with format and duration and plays on double-click and Enter', async () => {
    const onPlay = vi.fn()
    await table(onPlay)
    const grid = screen.getByRole('grid', { name: 'Tracks' })
    expect(grid.getAttribute('aria-rowcount')).toBe('4')
    const dawn = screen.getByText('Dawn').closest('[role=row]')!
    expect(within(dawn as HTMLElement).getByText('FLAC 16/44.1')).toBeTruthy()
    expect(within(dawn as HTMLElement).getByText('1:01')).toBeTruthy()
    fireEvent.doubleClick(dawn)
    expect(onPlay).toHaveBeenCalledWith(0)
    fireEvent.click(screen.getByText('Dusk'))
    fireEvent.keyDown(grid, { key: 'ArrowUp' })
    fireEvent.keyDown(grid, { key: 'Enter' })
    expect(onPlay).toHaveBeenLastCalledWith(1)
  })

  it('marks unplayable and missing rows', async () => {
    await table()
    expect(screen.getByTitle("Can't play: DECODE_FAILED")).toBeTruthy()
    expect(screen.getByTitle('File missing')).toBeTruthy()
  })

  it('selects ranges with shift and opens the menu for the selection', async () => {
    await table()
    fireEvent.click(screen.getByText('Dawn'))
    fireEvent.click(screen.getByText('Dusk'), { shiftKey: true })
    expect(screen.getAllByRole('row', { selected: true })).toHaveLength(3)
    fireEvent.contextMenu(screen.getByText('Noon'))
    expect(screen.getByRole('menuitem', { name: 'Play next' })).toBeTruthy()
    expect(screen.getByRole('menuitem', { name: 'Go to album' }).getAttribute('aria-disabled')).toBe('true')
  })

  it('highlights the playing track', async () => {
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, track: { path: '/m/Noon.flac', title: 'Noon', durationMs: 1, trackId: 2, artist: null, album: null, artworkId: null } } }))
    await table()
    expect(screen.getByText('Noon').closest('[role=row]')!.getAttribute('data-playing')).toBe('true')
  })

  it('loads unloaded rows of a long keyboard selection before acting on it', async () => {
    const { api, calls } = fakeApi()
    usePlayer.setState({ api })
    const all = Array.from({ length: 100 }, (_, i) => track(i + 1, `T${i}`))
    const loaded = new Map<number, unknown>()
    const onRange = vi.fn(async (a: number, b: number) => { for (let i = a; i <= Math.min(b, 99); i++) loaded.set(i, all[i]) })
    await renderInRouter(<><TrackTable label="Tracks" count={100} row={(i) => loaded.get(i) as never} onRange={onRange} onPlay={vi.fn()} /><ContextMenu /></>)
    expect(loaded.has(99)).toBe(false)
    const grid = screen.getByRole('grid', { name: 'Tracks' })
    fireEvent.keyDown(grid, { key: 'Home' })
    fireEvent.keyDown(grid, { key: 'End', shiftKey: true })
    await act(async () => { fireEvent.keyDown(grid, { key: 'F10', shiftKey: true }) })
    expect(onRange).toHaveBeenLastCalledWith(0, 99)
    fireEvent.click(screen.getByRole('menuitem', { name: 'Play next' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.insertNext', { trackIds: Array.from({ length: 100 }, (_, i) => i + 1) }]))
  })
})
