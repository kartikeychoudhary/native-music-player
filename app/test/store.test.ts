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
})
