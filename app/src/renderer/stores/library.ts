import { create } from 'zustand'
import type { Folder } from '@protocol/Folder'
import type { LibraryScope } from '@protocol/LibraryScope'
import type { Playlist } from '@protocol/Playlist'
import type { ScanProgress } from '@protocol/ScanProgress'
import type { AuroraApi } from '@shared/api'
import { useUi } from './ui'

export interface LibraryStore {
  api: AuroraApi | null
  versions: Record<LibraryScope, number>
  folders: Folder[]
  /** False until the first folder list arrives, so views don't flash an empty library at startup. */
  foldersLoaded: boolean
  /** The running scan; cleared when it ends or its folder leaves the library. */
  scan: ScanProgress | null
  playlists: Playlist[]
  connect(api: AuroraApi): () => void
  refreshFolders(): Promise<void>
  refreshPlaylists(): Promise<void>
  addFolder(): Promise<void>
  removeFolder(id: number): Promise<void>
  rescan(id?: number): Promise<void>
  createPlaylist(name: string): Promise<Playlist | null>
  /** Resolves true when the tracks were added; a failure is toasted here and resolves false. */
  addToPlaylist(playlistId: number, trackIds: number[]): Promise<boolean>
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

export const useLibrary = create<LibraryStore>()((set, get) => {
  /** Runs an engine request; failures become error toasts and resolve `null`. */
  const run = async <T,>(p: (api: AuroraApi) => Promise<T>): Promise<T | null> => {
    const api = get().api
    if (!api) return null
    try { return await p(api) } catch (e) { useUi.getState().toast('error', message(e)); return null }
  }
  return {
    api: null,
    versions: { tracks: 0, folders: 0, playlists: 0, history: 0 },
    folders: [],
    foldersLoaded: false,
    scan: null,
    playlists: [],
    connect(api) {
      set({ api })
      const load = () => { void get().refreshFolders(); void get().refreshPlaylists() }
      const offs = [
        api.on('library.changed', ({ scopes }) => {
          set((s) => {
            const versions = { ...s.versions }
            for (const sc of scopes) versions[sc] += 1
            return { versions }
          })
          if (scopes.includes('folders') || scopes.includes('tracks')) void get().refreshFolders()
          if (scopes.includes('playlists')) void get().refreshPlaylists()
        }),
        api.on('library.scanProgress', (scan) => set({ scan: scan.phase === 'done' ? null : scan })),
        api.on('engine.status', (s) => { if (s.state === 'ready') load() }),
      ]
      void api.status().then((s) => { if (s.state === 'ready') load() }).catch(() => {})
      return () => offs.forEach((off) => off())
    },
    async refreshFolders() {
      const f = await run((a) => a.call('library.folders', {}))
      if (!f) return
      // A scan of a folder that's gone can't still be running (its end may never be reported).
      set(({ scan }) => ({ folders: f, foldersLoaded: true, scan: scan && f.some((x) => x.id === scan.folderId) ? scan : null }))
    },
    async refreshPlaylists() { const p = await run((a) => a.call('playlists.list', {})); if (p) set({ playlists: p }) },
    async addFolder() {
      const api = get().api
      const path = await api?.openFolder().catch(() => null)
      if (!path) return
      if (await run((a) => a.call('library.addFolder', { path }))) await get().refreshFolders()
    },
    async removeFolder(id) { await run((a) => a.call('library.removeFolder', { id })); await get().refreshFolders() },
    async rescan(id) { await run((a) => a.call('library.rescan', id == null ? {} : { folderId: id })) },
    async createPlaylist(name) {
      const p = await run((a) => a.call('playlists.create', { name }))
      if (p) await get().refreshPlaylists()
      return p
    },
    async addToPlaylist(playlistId, trackIds) {
      const api = get().api
      if (!api) return false
      try { await api.call('playlists.addTracks', { id: playlistId, trackIds }); return true } catch (e) { useUi.getState().toast('error', message(e)); return false }
    },
  }
})
