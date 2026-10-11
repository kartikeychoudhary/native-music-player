import { create } from 'zustand'
import type { DeviceInfo } from '@protocol/DeviceInfo'
import type { EngineErrorEvent } from '@protocol/EngineErrorEvent'
import type { OutputMode } from '@protocol/OutputMode'
import type { PlayerState } from '@protocol/PlayerState'
import type { QueueChanged } from '@protocol/QueueChanged'
import type { SignalPath } from '@protocol/SignalPath'
import type { AuroraApi, EngineStatus } from '@shared/api'

export interface PlayerStore {
  api: AuroraApi | null
  state: PlayerState
  stamp: number
  signal: SignalPath | null
  queue: QueueChanged
  devices: DeviceInfo[]
  engine: EngineStatus
  error: EngineErrorEvent | null
  panel: 'queue' | 'signal'
  connect(api: AuroraApi): () => void
  openFiles(): Promise<void>
  playPaths(paths: string[]): Promise<void>
  playItem(id: number): Promise<void>
  toggle(): Promise<void>
  next(): Promise<void>
  prev(): Promise<void>
  seek(ms: number): Promise<void>
  setVolume(v: number): Promise<void>
  setMode(m: OutputMode): Promise<void>
  setDevice(id: string | null): Promise<void>
  setPanel(p: 'queue' | 'signal'): void
  dismissError(): void
}

const initialState: PlayerState = { status: 'stopped', queueIndex: null, queueItemId: null, track: null, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null, shuffle: false, repeat: 'off' }
const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now())

export function livePosition(s: Pick<PlayerStore, 'state' | 'stamp'>, at: number): number {
  const base = s.state.positionMs
  if (s.state.status !== 'playing') return base
  const p = base + Math.max(0, at - s.stamp)
  return s.state.durationMs != null ? Math.min(p, s.state.durationMs) : p
}

const trackKey = (s: PlayerState) => `${s.queueIndex ?? ''}|${s.track?.path ?? ''}`

export const usePlayer = create<PlayerStore>()((set, get) => {
  const call: AuroraApi['call'] = (method, params) => {
    const api = get().api
    return api ? api.call(method, params) : Promise.reject(new Error('not connected'))
  }
  // Runs a request; a rejection becomes a visible `error` (the IPC layer only carries the message) and resolves false.
  const safe = async (p: Promise<unknown>): Promise<boolean> => {
    try {
      await p
      return true
    } catch (e) {
      set({ error: { code: -32000, name: 'REQUEST_FAILED', message: e instanceof Error ? e.message : String(e), path: null } })
      return false
    }
  }

  return {
    api: null,
    state: initialState,
    stamp: 0,
    signal: null,
    queue: { items: [], current: null },
    devices: [],
    engine: { state: 'starting' },
    error: null,
    panel: 'queue',

    connect(api) {
      set({ api })
      // The signal path describes one open stream for one track; it carries no track identity, so it
      // is dropped when the track changes, unless it arrived since the last state (on a gapless
      // transition the engine sends the new track's signal path just before its state).
      let lastKey = trackKey(get().state)
      let signalSinceState = false
      const applyState = (state: PlayerState) => {
        const key = trackKey(state)
        const stale = state.status === 'stopped' || (key !== lastKey && !signalSinceState)
        lastKey = key
        signalSinceState = false
        set(stale ? { state, stamp: now(), signal: null } : { state, stamp: now() })
      }
      const refresh = async () => {
        try {
          const [state, queue, devices] = await Promise.all([api.call('player.state', {}), api.call('queue.get', {}), api.call('devices.list', {})])
          // After an engine (re)start any signal path we hold belongs to a stream that no longer exists.
          lastKey = trackKey(state)
          signalSinceState = false
          set({ state, stamp: now(), queue, devices, signal: null })
        } catch { /* the engine isn't ready yet; engine.status will trigger another refresh */ }
      }
      const offs = [
        api.on('player.state', applyState),
        api.on('player.signalPath', (signal) => { signalSinceState = true; set({ signal }) }),
        api.on('queue.changed', (queue) => set({ queue, signal: null })),
        api.on('devices.changed', (devices) => set({ devices })),
        api.on('engine.error', (error) => set({ error })),
        api.on('engine.status', (engine) => { set({ engine }); if (engine.state === 'ready') void refresh() }),
      ]
      void api.status().then((engine) => { set({ engine }); if (engine.state === 'ready') void refresh() }).catch(() => {})
      return () => offs.forEach((off) => off())
    },

    async openFiles() {
      const api = get().api
      if (!api) return
      let paths: string[]
      try {
        paths = await api.openFiles()
      } catch (e) {
        await safe(Promise.reject(e))
        return
      }
      if (paths.length) await get().playPaths(paths)
    },
    async playPaths(paths) {
      if (!(await safe(call('queue.setPaths', { paths, startIndex: 0 })))) return
      set({ error: null })
      await safe(call('player.play', {}))
    },
    async playItem(id) {
      if (await safe(call('player.play', { queueItemId: id }))) set({ error: null })
    },
    async toggle() { await safe(call('player.toggle', {})) },
    async next() { await safe(call('player.next', {})) },
    async prev() { await safe(call('player.prev', {})) },
    async seek(ms) {
      set((s) => ({ state: { ...s.state, positionMs: ms }, stamp: now() }))
      await safe(call('player.seek', { ms: Math.round(ms) }))
    },
    async setVolume(volume) {
      set((s) => ({ state: { ...s.state, volume } }))
      await safe(call('player.setVolume', { volume }))
    },
    async setMode(mode) { await safe(call('player.setMode', { mode })) },
    async setDevice(id) { await safe(call('player.setDevice', { id })) },
    setPanel: (panel) => set({ panel }),
    dismissError: () => set({ error: null }),
  }
})
