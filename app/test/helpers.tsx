import { render } from '@testing-library/react'
import { act } from 'react'
import { createMemoryHistory, createRootRoute, createRouter, RouterProvider } from '@tanstack/react-router'
import { vi } from 'vitest'
import type { AuroraApi, Methods } from '../src/shared/api'
import { createAppRouter } from '../src/renderer/router'
import { useDsp } from '../src/renderer/stores/dsp'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { DEFAULT_SETTINGS } from '../src/shared/settings'

export const DSP_DEFAULT = { eq: { enabled: false, bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], preamp: 0, preset: 'Flat' }, crossfadeSecs: 0, replayGain: 'off' }
export const PRESETS = [
  { name: 'Flat', bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], preamp: 0, builtin: true },
  { name: 'Bass', bands: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0], preamp: 0, builtin: true },
  { name: 'Mine', bands: [1, 1, 1, 1, 1, 1, 1, 1, 1, 1], preamp: -1, builtin: false },
]

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
  'dsp.get': () => DSP_DEFAULT,
  'dsp.presets': () => PRESETS,
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
    toggleMini: vi.fn().mockResolvedValue(undefined),
    showMain: vi.fn().mockResolvedValue(undefined),
    setViz: vi.fn(),
    onSpectrum: vi.fn((_cb: (bins: Uint8Array) => void) => () => {}),
    platform: 'linux',
  }
  const emit = (e: string, p: unknown) => act(() => { handlers.get(e)?.forEach((cb) => cb(p)) })
  return { api: api as unknown as AuroraApi & typeof api, emit, calls }
}

export function resetStores() {
  usePlayer.setState(usePlayer.getInitialState())
  useLibrary.setState(useLibrary.getInitialState())
  useUi.setState(useUi.getInitialState())
  useDsp.setState(useDsp.getInitialState())
}

/** Renders the real route tree at `path` with the stores connected to `api`. */
export async function renderAt(path: string, api: AuroraApi = fakeApi().api) {
  resetStores()
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }))
  usePlayer.getState().connect(api)
  useLibrary.getState().connect(api)
  useDsp.getState().connect(api)
  await act(async () => { render(<RouterProvider router={router} />) })
  return { router }
}

export const track = (id: number, title: string, extra: Record<string, unknown> = {}) => ({
  id, path: `/m/${title}.flac`, title, artist: 'Alpha', artistId: 1, album: 'Light', albumId: 2, albumArtist: null, trackNo: id, discNo: 1,
  year: 2001, genre: null, durationMs: 61_000, codec: 'flac', container: 'flac', bitrateKbps: null, sampleRate: 44100, bitDepth: 16,
  channels: 2, artworkId: null, missing: false, unplayable: null, playCount: 0, addedAt: 0, lastPlayedAt: null, ...extra,
}) as never

export const album = (id: number, title: string, extra: Record<string, unknown> = {}) => ({
  id, title, artist: 'Alpha', artistId: 1, year: 2001, trackCount: 2, durationMs: 122_000, artworkId: 5, addedAt: 0,
  codec: 'flac', maxSampleRate: 96000, maxBitDepth: 24, ...extra,
}) as never

/** Renders `ui` alone inside a router (components that navigate need router context). */
export async function renderInRouter(ui: React.ReactNode) {
  const root = createRootRoute({ component: () => <>{ui}</> })
  const router = createRouter({ routeTree: root, history: createMemoryHistory({ initialEntries: ['/'] }) })
  await act(async () => { render(<RouterProvider router={router as never} />) })
  return { router }
}
