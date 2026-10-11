import { beforeEach, describe, expect, it, vi } from 'vitest'
import { trackMenu } from '../src/renderer/components/trackMenu'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, resetStores, track } from './helpers'

describe('trackMenu', () => {
  beforeEach(resetStores)

  it('offers queue, playlist, navigation and info actions', () => {
    useLibrary.setState({ playlists: [{ id: 9, name: 'Mix', trackCount: 0, durationMs: 0, updatedAt: 0 }] })
    const go = vi.fn()
    const items = trackMenu([track(1, 'Dawn')], go)
    expect(items.filter((i) => !i.separator).map((i) => i.label)).toEqual(['Play next', 'Add to queue', 'Add to playlist', 'Go to album', 'Go to artist', 'Show in folder', 'Track info'])
    expect(items[2].items!.map((i) => i.label)).toEqual(['New playlist…', 'Mix'])
    items.find((i) => i.label === 'Go to album')!.onSelect!()
    expect(go).toHaveBeenCalledWith('/albums/2')
    items.find((i) => i.label === 'Track info')!.onSelect!()
    expect(useUi.getState().trackInfo).toBe(1)
  })

  it('disables single-track actions for a multi-selection', () => {
    const items = trackMenu([track(1, 'Dawn'), track(2, 'Noon')], vi.fn())
    for (const label of ['Go to album', 'Go to artist', 'Show in folder', 'Track info']) expect(items.find((i) => i.label === label)!.disabled).toBe(true)
  })

  it('creates a playlist from the selection', async () => {
    const { api, calls } = fakeApi({ 'playlists.create': (p) => ({ id: 3, name: p.name, trackCount: 0, durationMs: 0, updatedAt: 0 }) })
    usePlayer.setState({ api }); useLibrary.setState({ api })
    const items = trackMenu([track(1, 'Dawn'), track(2, 'Noon')], vi.fn())
    items[2].items![0].onSelect!()
    useUi.getState().prompt!.resolve('Road trip')
    await vi.waitFor(() => expect(calls).toContainEqual(['playlists.addTracks', { id: 3, trackIds: [1, 2] }]))
  })

  it('announces success only when the playlist add worked', async () => {
    useLibrary.setState({ playlists: [{ id: 9, name: 'Mix', trackCount: 0, durationMs: 0, updatedAt: 0 }] })
    const toasts = () => useUi.getState().toasts.map((t) => t.text)

    const ok = fakeApi({ 'playlists.addTracks': () => ({ id: 9, name: 'Mix', trackCount: 1, durationMs: 0, updatedAt: 0 }) })
    usePlayer.setState({ api: ok.api }); useLibrary.setState({ api: ok.api })
    trackMenu([track(1, 'Dawn')], vi.fn())[2].items![1].onSelect!()
    await vi.waitFor(() => expect(toasts()).toContain('Added to Mix'))

    useUi.setState({ toasts: [] })
    const bad = fakeApi({ 'playlists.addTracks': () => { throw new Error('nope') } })
    usePlayer.setState({ api: bad.api }); useLibrary.setState({ api: bad.api })
    trackMenu([track(1, 'Dawn')], vi.fn())[2].items![1].onSelect!()
    await vi.waitFor(() => expect(toasts()).toContain('nope'))
    expect(toasts()).not.toContain('Added to Mix')
  })
})
