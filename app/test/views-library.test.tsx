// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { album, fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

const folder = { id: 1, path: '/music', trackCount: 3, lastScanAt: 1, available: true }

describe('home', () => {
  it('invites the user to add a folder when the library is empty', async () => {
    const { api } = fakeApi()
    await renderAt('/', api)
    fireEvent.click(await screen.findByRole('button', { name: 'Add a music folder' }))
    await vi.waitFor(() => expect(api.openFolder).toHaveBeenCalled())
    expect(screen.getByRole('button', { name: 'Open files' })).toBeTruthy()
  })

  it("doesn't show the empty state until the folder list has loaded", async () => {
    let resolve: (f: unknown[]) => void = () => {}
    const { api } = fakeApi({ 'library.folders': () => new Promise((r) => { resolve = r }) })
    await renderAt('/', api)
    await vi.waitFor(() => expect(api.call).toHaveBeenCalledWith('library.folders', {}))
    expect(screen.queryByRole('button', { name: 'Add a music folder' })).toBeNull()
    await act(async () => { resolve([]) })
    expect(await screen.findByRole('button', { name: 'Add a music folder' })).toBeTruthy()
  })

  it('shows recently added albums and plays recently played tracks', async () => {
    const { api, calls } = fakeApi({
      'library.folders': () => [folder],
      'library.recent': (p) => (p.kind === 'added' ? { albums: [album(5, 'First Light')], tracks: [] } : { albums: [], tracks: p.kind === 'played' ? [track(7, 'Dawn'), track(8, 'Noon')] : [] }),
    })
    await renderAt('/', api)
    expect(await screen.findByText('First Light')).toBeTruthy()
    const played = await screen.findByRole('grid', { name: 'Recently played' })
    fireEvent.doubleClick(within(played).getByText('Noon'))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [7, 8], startIndex: 1 }]))
  })

  it('shows scan progress while scanning', async () => {
    const { api, emit } = fakeApi({ 'library.folders': () => [folder] })
    await renderAt('/', api)
    emit('library.scanProgress', { folderId: 1, phase: 'reading', processed: 40, total: 100, added: 40, updated: 0, missing: 0 })
    expect(await screen.findByText('Scanning 40 / 100')).toBeTruthy()
  })
})

describe('albums', () => {
  it('lists albums and re-queries when the sort changes', async () => {
    const { api, calls } = fakeApi({ 'library.albums': () => ({ items: [album(5, 'First Light'), album(6, 'Zenith', { artworkId: null })], total: 2 }) })
    await renderAt('/albums', api)
    expect(await screen.findByText('Zenith')).toBeTruthy()
    expect(screen.getByText('2 albums')).toBeTruthy()
    fireEvent.change(screen.getByRole('combobox', { name: 'Sort albums' }), { target: { value: 'artist' } })
    await vi.waitFor(() => expect(calls.some(([m, p]) => m === 'library.albums' && p.sort === 'artist')).toBe(true))
  })

  it('opens an album from its card', async () => {
    const { api } = fakeApi({ 'library.albums': () => ({ items: [album(5, 'First Light')], total: 1 }), 'library.album': () => ({ album: album(5, 'First Light'), tracks: [], genres: [] }) })
    const { router } = await renderAt('/albums', api)
    fireEvent.click(await screen.findByText('First Light'))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/albums/5'))
  })
})

describe('album detail', () => {
  const detail = { album: album(5, 'First Light'), tracks: [track(1, 'Dawn'), track(2, 'Noon')], genres: [{ id: 3, name: 'Ambient' }] }

  it('shows the header with format readout and dynamic colour, and plays the album', async () => {
    const { api, calls } = fakeApi({ 'library.album': () => detail, 'library.artwork': () => ({ path: '/a.webp', palette: { dominant: '#112233', vibrant: '#ff0000', muted: '#445566' } }) })
    await renderAt('/albums/5', api)
    expect(await screen.findByRole('heading', { name: 'First Light' })).toBeTruthy()
    expect(screen.getByText('FLAC 24/96')).toBeTruthy()
    await vi.waitFor(() => expect(screen.getByTestId('album-head').getAttribute('style')).toContain('#445566'))
    fireEvent.click(screen.getByRole('button', { name: 'Play album' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [1, 2], startIndex: 0 }]))
    fireEvent.click(screen.getByRole('button', { name: 'Shuffle album' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.setShuffle', { shuffle: true }]))
  })

  it('reports an unknown album', async () => {
    const { api } = fakeApi({ 'library.album': () => { throw new Error('album 99 not found') } })
    await renderAt('/albums/99', api)
    expect(await screen.findByText('album 99 not found')).toBeTruthy()
  })
})

describe('artists and genres', () => {
  it('shows an artist with top tracks and albums, and plays everything', async () => {
    const { api, calls } = fakeApi({
      'library.artist': () => ({ artist: { id: 1, name: 'Alpha', albumCount: 1, trackCount: 2, artworkId: null }, albums: [album(5, 'First Light')], topTracks: [track(1, 'Dawn', { playCount: 4 })] }),
      'library.tracks': () => ({ items: [track(1, 'Dawn'), track(2, 'Noon')], total: 2 }),
    })
    await renderAt('/artists/1', api)
    expect(await screen.findByRole('heading', { name: 'Alpha' })).toBeTruthy()
    expect(within(screen.getByRole('grid', { name: 'Top tracks' })).getByText('Dawn')).toBeTruthy()
    expect(screen.getByText('First Light')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Play artist' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['queue.set', { trackIds: [1, 2], startIndex: 0 }]))
    expect(calls.find(([m]) => m === 'library.tracks')![1]).toMatchObject({ filter: { artistId: 1 }, sort: 'album' })
  })

  it('lists artists', async () => {
    const { api } = fakeApi({ 'library.artists': () => ({ items: [{ id: 1, name: 'Alpha', albumCount: 2, trackCount: 9, artworkId: null }], total: 1 }) })
    await renderAt('/artists', api)
    expect(await screen.findByText('Alpha')).toBeTruthy()
    expect(screen.getByText('2 albums · 9 tracks')).toBeTruthy()
  })

  it('goes from a genre to its albums', async () => {
    const { api, calls } = fakeApi({
      'library.genres': () => ({ items: [{ id: 3, name: 'Ambient', albumCount: 1, trackCount: 2 }], total: 1 }),
      'library.genre': () => ({ id: 3, name: 'Ambient', albumCount: 1, trackCount: 2 }),
      'library.albums': () => ({ items: [album(5, 'First Light')], total: 1 }),
    })
    const { router } = await renderAt('/genres', api)
    fireEvent.click(await screen.findByText('Ambient'))
    await vi.waitFor(() => expect(router.state.location.pathname).toBe('/genres/3'))
    expect(await screen.findByText('First Light')).toBeTruthy()
    expect(calls.some(([m, p]) => m === 'library.albums' && p.filter?.genreId === 3)).toBe(true)
  })

  it('adds the library sections to the sidebar', async () => {
    await renderAt('/albums')
    for (const name of ['Home', 'Albums', 'Artists', 'Genres', 'Signal path']) expect(screen.getByRole('link', { name })).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Albums' }).getAttribute('aria-current')).toBe('page')
  })
})
