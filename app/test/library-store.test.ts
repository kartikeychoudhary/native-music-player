import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useLibrary } from '../src/renderer/stores/library'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, resetStores } from './helpers'

describe('library store', () => {
  beforeEach(resetStores)

  it('loads folders and playlists once the engine is ready', async () => {
    const { api } = fakeApi({ 'library.folders': () => [{ id: 1, path: '/m', trackCount: 3, lastScanAt: 1, available: true }], 'playlists.list': () => [{ id: 4, name: 'Mix', trackCount: 0, durationMs: 0, updatedAt: 0 }] })
    useLibrary.getState().connect(api)
    await vi.waitFor(() => expect(useLibrary.getState().folders).toHaveLength(1))
    expect(useLibrary.getState().playlists[0].name).toBe('Mix')
  })

  it('bumps scope versions and reloads what changed', async () => {
    const { api, emit, calls } = fakeApi()
    useLibrary.getState().connect(api)
    await vi.waitFor(() => expect(calls.some(([m]) => m === 'playlists.list')).toBe(true))
    calls.length = 0
    emit('library.changed', { scopes: ['playlists', 'tracks'] })
    expect(useLibrary.getState().versions.playlists).toBe(1)
    expect(useLibrary.getState().versions.tracks).toBe(1)
    expect(useLibrary.getState().versions.folders).toBe(0)
    await vi.waitFor(() => expect(calls.some(([m]) => m === 'playlists.list')).toBe(true))
  })

  it('tracks scan progress', () => {
    const { api, emit } = fakeApi()
    useLibrary.getState().connect(api)
    emit('library.scanProgress', { folderId: 1, phase: 'reading', processed: 5, total: 10, added: 5, updated: 0, missing: 0 })
    expect(useLibrary.getState().scan?.processed).toBe(5)
  })

  it('adds the picked folder, and does nothing when the picker is cancelled', async () => {
    const { api, calls } = fakeApi({ 'library.addFolder': (p) => ({ id: 2, path: p.path, trackCount: 0, lastScanAt: null, available: true }) })
    useLibrary.getState().connect(api)
    await useLibrary.getState().addFolder()
    expect(calls.some(([m]) => m === 'library.addFolder')).toBe(false)
    api.openFolder.mockResolvedValueOnce('/music')
    await useLibrary.getState().addFolder()
    expect(calls).toContainEqual(['library.addFolder', { path: '/music' }])
  })

  it('reports failures as error toasts', async () => {
    const { api } = fakeApi({ 'library.addFolder': () => { throw new Error('/x overlaps the library folder /x/y') } })
    useLibrary.getState().connect(api)
    api.openFolder.mockResolvedValueOnce('/x')
    await useLibrary.getState().addFolder()
    expect(useUi.getState().toasts.at(-1)).toMatchObject({ kind: 'error', text: '/x overlaps the library folder /x/y' })
  })
})
