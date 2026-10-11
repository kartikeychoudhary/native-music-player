// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { album, fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

const item = (id: number, title: string) => ({ id, path: `/m/${title}.flac`, title, trackId: id * 10, artist: 'Alpha', album: 'Light', artworkId: null, durationMs: 60_000 })

describe('queue view', () => {
  it('shows the queue with the current item and edits it', async () => {
    const { api, calls } = fakeApi({ 'queue.get': () => ({ items: [item(1, 'Dawn'), item(2, 'Noon'), item(3, 'Dusk')], current: 1 }) })
    await renderAt('/queue', api)
    const list = await screen.findByRole('list', { name: 'Queue' })
    expect(within(list).getByText('Noon').closest('[data-current]')!.getAttribute('data-current')).toBe('true')
    act(() => usePlayer.setState({ state: { ...usePlayer.getState().state, status: 'playing', track: { path: '/m/Noon.flac', title: 'Noon', durationMs: 1, trackId: 20, artist: 'Alpha', album: 'Light', artworkId: null } } }))
    expect(screen.getByText('Playing from Light')).toBeTruthy()
    fireEvent.doubleClick(within(list).getByText('Dusk'))
    await vi.waitFor(() => expect(calls).toContainEqual(['player.play', { queueItemId: 3 }]))
    fireEvent.click(within(list).getAllByRole('button', { name: 'Remove from queue' })[0])
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.remove', { itemIds: [1] }]))
    fireEvent.contextMenu(within(list).getByText('Dawn'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move down' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.move', { from: 0, to: 1 }]))
    fireEvent.click(screen.getByRole('button', { name: 'Clear queue' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.clear', {}]))
  })

  it('opens from the player bar', async () => {
    const { router } = await renderAt('/')
    fireEvent.click(screen.getByRole('button', { name: 'Queue' }))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/queue'))
  })
})

describe('search view', () => {
  it('searches as you type and groups the results', async () => {
    const { api, calls } = fakeApi({
      'library.search': () => ({ tracks: [track(1, 'Dawn')], albums: [album(5, 'First Light')], artists: [{ id: 1, name: 'Alpha', albumCount: 1, trackCount: 3, artworkId: null }] }),
    })
    await renderAt('/search', api)
    const input = screen.getByRole('searchbox', { name: 'Search' })
    expect(document.activeElement).toBe(input)
    fireEvent.change(input, { target: { value: 'a' } })
    expect(screen.getByText('Type at least two characters.')).toBeTruthy()
    fireEvent.change(input, { target: { value: 'alp' } })
    await vi.waitFor(() => expect(calls).toContainEqual(['library.search', { text: 'alp', limit: 20 }]))
    expect(await screen.findByRole('heading', { name: 'Artists' })).toBeTruthy()
    expect(screen.getByText('First Light')).toBeTruthy()
    expect(within(screen.getByRole('grid', { name: 'Tracks' })).getByText('Dawn')).toBeTruthy()
    expect(calls.filter(([m]) => m === 'library.search').every(([, p]) => p.text.length >= 2)).toBe(true)
  })
})
