import { beforeEach, describe, expect, it, vi } from 'vitest'
import { livePosition, usePlayer } from '../src/renderer/stores/player'

function fakeApi() {
  const handlers = new Map<string, (p: any) => void>()
  const api = {
    call: vi.fn(async (m: string) => (m === 'player.state' ? { status: 'stopped', queueIndex: null, track: null, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null } : m === 'devices.list' ? [] : m === 'queue.get' ? { items: [], current: null } : null)),
    on: vi.fn((e: string, cb: any) => { handlers.set(e, cb); return () => handlers.delete(e) }),
    openFiles: vi.fn(async () => ['/m/a.flac', '/m/b.flac']),
    pathsForFiles: vi.fn(() => []),
    status: vi.fn(async () => ({ state: 'ready' })),
    restartEngine: vi.fn(), openLogs: vi.fn(), platform: 'linux',
  }
  return { api, emit: (e: string, p: any) => handlers.get(e)?.(p) }
}

describe('player store', () => {
  beforeEach(() => usePlayer.setState(usePlayer.getInitialState()))

  it('applies engine events', async () => {
    const { api, emit } = fakeApi()
    usePlayer.getState().connect(api as any)
    emit('player.state', { status: 'playing', positionMs: 1500, volume: 0.5, mode: 'processed' })
    expect(usePlayer.getState().state.status).toBe('playing')
    emit('player.signalPath', { bitPerfect: true })
    expect(usePlayer.getState().signal).toEqual({ bitPerfect: true })
    emit('engine.error', { code: -32003, name: 'UNSUPPORTED_FORMAT', message: 'x', path: '/a' })
    expect(usePlayer.getState().error?.name).toBe('UNSUPPORTED_FORMAT')
  })

  describe('signal path', () => {
    const track = (path: string, queueIndex = 0) => ({ status: 'playing', queueIndex, track: { path, title: 't', durationMs: null }, positionMs: 0, durationMs: null, volume: 1, mode: 'processed', deviceId: null })

    it('clears on queue.changed', () => {
      const { api, emit } = fakeApi()
      usePlayer.getState().connect(api as any)
      emit('player.signalPath', { bitPerfect: true })
      emit('queue.changed', { items: [], current: null })
      expect(usePlayer.getState().signal).toBeNull()
    })

    it('clears on a refresh after the engine becomes ready', async () => {
      const { api, emit } = fakeApi()
      usePlayer.getState().connect(api as any)
      await vi.waitFor(() => expect(api.call).toHaveBeenCalledWith('player.state', {}))
      emit('player.signalPath', { bitPerfect: true })
      emit('engine.status', { state: 'ready' })
      await vi.waitFor(() => expect(usePlayer.getState().signal).toBeNull())
    })

    it('keeps a signal path that arrives just before the new track state (gapless)', () => {
      const { api, emit } = fakeApi()
      usePlayer.getState().connect(api as any)
      emit('player.state', track('/m/a.flac', 0))
      emit('player.signalPath', { bitPerfect: true })
      emit('player.signalPath', { bitPerfect: false })
      emit('player.state', track('/m/b.flac', 1))
      expect(usePlayer.getState().signal).toEqual({ bitPerfect: false })
    })

    it('clears when the track changes and survives same-track state updates', () => {
      const { api, emit } = fakeApi()
      usePlayer.getState().connect(api as any)
      emit('player.state', track('/m/a.flac'))
      emit('player.signalPath', { bitPerfect: true })
      emit('player.state', { ...track('/m/a.flac'), positionMs: 500 })
      expect(usePlayer.getState().signal).toEqual({ bitPerfect: true })
      emit('player.state', track('/m/b.flac', 1))
      expect(usePlayer.getState().signal).toBeNull()
    })

    it('clears when the queue index changes on the same path', () => {
      const { api, emit } = fakeApi()
      usePlayer.getState().connect(api as any)
      emit('player.state', track('/m/a.flac', 0))
      emit('player.signalPath', { bitPerfect: true })
      emit('player.state', { ...track('/m/a.flac', 0), positionMs: 250 })
      emit('player.state', track('/m/a.flac', 1))
      expect(usePlayer.getState().signal).toBeNull()
    })

    it('clears when playback stops', () => {
      const { api, emit } = fakeApi()
      usePlayer.getState().connect(api as any)
      emit('player.state', track('/m/a.flac'))
      emit('player.signalPath', { bitPerfect: true })
      emit('player.state', { ...track('/m/a.flac'), status: 'stopped', queueIndex: null, track: null })
      expect(usePlayer.getState().signal).toBeNull()
    })
  })

  it('openFiles sets the queue and plays', async () => {
    const { api } = fakeApi()
    usePlayer.getState().connect(api as any)
    await usePlayer.getState().openFiles()
    expect(api.call).toHaveBeenCalledWith('queue.setPaths', { paths: ['/m/a.flac', '/m/b.flac'], startIndex: 0 })
    expect(api.call).toHaveBeenCalledWith('player.play', {})
  })

  it('interpolates position only while playing', () => {
    expect(livePosition({ state: { status: 'playing', positionMs: 1000 } as any, stamp: 100 }, 350)).toBe(1250)
    expect(livePosition({ state: { status: 'paused', positionMs: 1000 } as any, stamp: 100 }, 350)).toBe(1000)
    expect(livePosition({ state: { status: 'playing', positionMs: 1000, durationMs: 1100 } as any, stamp: 0 }, 5000)).toBe(1100)
  })

  it('surfaces a failed queue.setPaths as an error and does not start playback', async () => {
    const { api } = fakeApi()
    const call = api.call
    api.call = vi.fn(async (m: string) => { if (m === 'queue.setPaths') throw new Error('bad path'); return call(m) }) as any
    usePlayer.getState().connect(api as any)
    await usePlayer.getState().playPaths(['/m/a.flac'])
    expect(usePlayer.getState().error).toMatchObject({ code: -32000, name: 'REQUEST_FAILED', message: 'bad path', path: null })
    expect(api.call).not.toHaveBeenCalledWith('player.play', expect.anything())
  })

  it('surfaces a failed open dialog without an unhandled rejection', async () => {
    const { api } = fakeApi()
    api.openFiles = vi.fn(async () => { throw new Error('dialog failed') })
    usePlayer.getState().connect(api as any)
    await expect(usePlayer.getState().openFiles()).resolves.toBeUndefined()
    expect(usePlayer.getState().error?.message).toBe('dialog failed')
  })

  it('surfaces failed setMode / setDevice', async () => {
    const { api } = fakeApi()
    api.call = vi.fn(async () => { throw new Error('nope') }) as any
    usePlayer.getState().connect(api as any)
    await usePlayer.getState().setMode('bitPerfect')
    expect(usePlayer.getState().error?.name).toBe('REQUEST_FAILED')
    usePlayer.getState().dismissError()
    await usePlayer.getState().setDevice('x')
    expect(usePlayer.getState().error?.message).toBe('nope')
  })

  it('clears a stale error after a successful playPaths', async () => {
    const { api } = fakeApi()
    usePlayer.getState().connect(api as any)
    usePlayer.setState({ error: { code: -32000, name: 'REQUEST_FAILED', message: 'old', path: null } })
    await usePlayer.getState().playPaths(['/m/a.flac'])
    expect(usePlayer.getState().error).toBeNull()
  })
})
