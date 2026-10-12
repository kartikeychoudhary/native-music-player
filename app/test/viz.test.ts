import { describe, expect, it, vi } from 'vitest'
import { createVizHub } from '../src/main/viz'

describe('viz hub', () => {
  it('viz_refcount_tracks_windows_and_engine_restarts', async () => {
    const call = vi.fn().mockResolvedValue(null)
    const send = vi.fn().mockReturnValue(true)
    const hub = createVizHub({ call, send })
    hub.set(1, true)
    hub.set(2, true)
    expect(call.mock.calls).toEqual([['viz.subscribe']])
    hub.frame(1, new Uint8Array(64))
    expect(send.mock.calls.map((c) => c[0]).sort()).toEqual([1, 2])
    send.mockClear()
    hub.frame(2, new Uint8Array(4))
    expect(send).not.toHaveBeenCalled() // only spectrum frames are forwarded
    hub.set(1, false)
    hub.frame(1, new Uint8Array(64))
    expect(send.mock.calls.map((c) => c[0])).toEqual([2])
    hub.drop(2) // window closed
    expect(call.mock.calls).toEqual([['viz.subscribe'], ['viz.unsubscribe']])
    hub.engineReady()
    expect(call).toHaveBeenCalledTimes(2) // nobody wants frames: no re-subscribe
    hub.set(3, true)
    hub.engineReady() // engine restarted: it forgot the subscription
    expect(call.mock.calls.slice(2)).toEqual([['viz.subscribe'], ['viz.subscribe']])
  })

  it('retries after a failed subscribe once the engine is ready', async () => {
    const call = vi.fn().mockRejectedValueOnce(new Error('engine not ready')).mockResolvedValue(null)
    const hub = createVizHub({ call, send: vi.fn() })
    hub.set(1, true)
    await Promise.resolve(); await Promise.resolve()
    hub.engineReady()
    expect(call.mock.calls).toEqual([['viz.subscribe'], ['viz.subscribe']])
  })
})
