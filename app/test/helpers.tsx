import { render } from '@testing-library/react'
import { act } from 'react'
import { createMemoryHistory, RouterProvider } from '@tanstack/react-router'
import { vi } from 'vitest'
import type { AuroraApi, Methods } from '../src/shared/api'
import { createAppRouter } from '../src/renderer/router'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { DEFAULT_SETTINGS } from '../src/shared/settings'

type Handler = (params: any) => any
const DEFAULTS: Partial<Record<keyof Methods, Handler>> = {
  'player.state': () => ({ status: 'stopped', queueIndex: null, queueItemId: null, track: null, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null, shuffle: false, repeat: 'off' }),
  'queue.get': () => ({ items: [], current: null }),
  'devices.list': () => [],
  'library.folders': () => [],
  'playlists.list': () => [],
  'library.recent': () => ({ albums: [], tracks: [] }),
  'library.albums': () => ({ items: [], total: 0 }),
  'library.artists': () => ({ items: [], total: 0 }),
  'library.genres': () => ({ items: [], total: 0 }),
  'library.tracks': () => ({ items: [], total: 0 }),
  'library.search': () => ({ tracks: [], albums: [], artists: [] }),
}

export function fakeApi(overrides: Partial<Record<keyof Methods, Handler>> = {}) {
  const handlers = new Map<string, Set<(p: any) => void>>()
  const calls: [string, any][] = []
  const api = {
    call: vi.fn(async (m: string, p: any) => { calls.push([m, p]); const h = overrides[m as keyof Methods] ?? DEFAULTS[m as keyof Methods]; return h ? h(p) : null }),
    on: vi.fn((e: string, cb: (p: any) => void) => { if (!handlers.has(e)) handlers.set(e, new Set()); handlers.get(e)!.add(cb); return () => handlers.get(e)!.delete(cb) }),
    openFiles: vi.fn(async () => []),
    openFolder: vi.fn(async (): Promise<string | null> => null),
    pickM3u: vi.fn(async (): Promise<string | null> => null),
    pickM3uSave: vi.fn(async (): Promise<string | null> => null),
    showItemInFolder: vi.fn(async () => {}),
    getSettings: vi.fn(async () => DEFAULT_SETTINGS),
    setSettings: vi.fn(async (p: any) => ({ ...DEFAULT_SETTINGS, ...p })),
    pathsForFiles: vi.fn(() => []),
    status: vi.fn(async () => ({ state: 'ready' })),
    restartEngine: vi.fn(async () => {}),
    openLogs: vi.fn(async () => {}),
    platform: 'linux',
  }
  const emit = (e: string, p: unknown) => act(() => { handlers.get(e)?.forEach((cb) => cb(p)) })
  return { api: api as unknown as AuroraApi & typeof api, emit, calls }
}

export function resetStores() {
  usePlayer.setState(usePlayer.getInitialState())
  useLibrary.setState(useLibrary.getInitialState())
  useUi.setState(useUi.getInitialState())
}

/** Renders the real route tree at `path` with the stores connected to `api`. */
export async function renderAt(path: string, api: AuroraApi = fakeApi().api) {
  resetStores()
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }))
  usePlayer.getState().connect(api)
  useLibrary.getState().connect(api)
  await act(async () => { render(<RouterProvider router={router} />) })
  return { router }
}
