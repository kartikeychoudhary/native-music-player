// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

const mix = { id: 4, name: 'Mix', trackCount: 2, durationMs: 122_000, updatedAt: 0 }
const detail = { playlist: mix, items: [{ itemId: 10, track: track(1, 'Dawn') }, { itemId: 11, track: track(2, 'Noon') }] }

describe('playlists', () => {
  it('lists playlists, creates one, and imports an M3U', async () => {
    const { api, calls } = fakeApi({
      'playlists.list': () => [mix],
      'playlists.create': (p) => ({ ...mix, id: 5, name: p.name, trackCount: 0 }),
      'playlists.importM3u': () => ({ playlist: { ...mix, id: 6, name: 'Road' }, skipped: 2 }),
    })
    await renderAt('/playlists', api)
    expect(await screen.findByText('Mix')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'New playlist' }))
    act(() => useUi.getState().prompt!.resolve('Chill'))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.create', { name: 'Chill' }]))
    api.pickM3u.mockResolvedValueOnce('/lists/Road.m3u8')
    fireEvent.click(screen.getByRole('button', { name: 'Import M3U' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.importM3u', { path: '/lists/Road.m3u8' }]))
    expect(await screen.findByText('Imported Road · 2 entries not found in your library')).toBeTruthy()
  })

  it('shows a playlist, plays from a row, reorders, removes, renames, exports and deletes', async () => {
    const { api, calls } = fakeApi({ 'playlists.get': () => detail, 'playlists.list': () => [mix], 'playlists.rename': (p) => ({ ...mix, name: p.name }) })
    const { router } = await renderAt('/playlists/4', api)
    expect(await screen.findByRole('heading', { name: 'Mix' })).toBeTruthy()
    const list = screen.getByRole('list', { name: 'Mix' })
    fireEvent.doubleClick(within(list).getByText('Noon'))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [1, 2], startIndex: 1 }]))

    fireEvent.contextMenu(within(list).getByText('Dawn'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move down' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.reorder', { id: 4, from: 0, to: 1 }]))

    fireEvent.contextMenu(within(list).getByText('Noon'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Remove from playlist' }))
    await vi.waitFor(() => expect(calls.some(([m, p]) => m === 'playlists.removeItems' && p.id === 4 && p.itemIds.length === 1)).toBe(true))

    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    act(() => useUi.getState().prompt!.resolve('Better Mix'))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.rename', { id: 4, name: 'Better Mix' }]))

    api.pickM3uSave.mockResolvedValueOnce('/out/Mix.m3u8')
    fireEvent.click(screen.getByRole('button', { name: 'Export M3U' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.exportM3u', { id: 4, path: '/out/Mix.m3u8' }]))

    fireEvent.click(screen.getByRole('button', { name: 'Delete playlist' }))
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.delete', { id: 4 }]))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/playlists'))
  })

  it('lists playlists in the expanded sidebar', async () => {
    await renderAt('/', fakeApi({ 'playlists.list': () => [mix] }).api)
    act(() => useUi.getState().setSidebar(true, false))
    expect(await screen.findByRole('link', { name: 'Mix' })).toBeTruthy()
  })
})
