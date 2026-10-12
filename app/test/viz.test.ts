import { describe, expect, it, vi } from 'vitest'
import { EventEmitter } from 'node:events'
import { createVizHub, hookSender } from '../src/main/viz'

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

  it('drops a window whose send fails, and unsubscribes when it was the last one', async () => {
    const call = vi.fn().mockResolvedValue(null)
    const send = vi.fn((id: number) => id !== 1)
    const hub = createVizHub({ call, send })
    hub.set(1, true)
    hub.set(2, true)
    hub.frame(1, new Uint8Array(64)) // window 1 is dead: dropped, window 2 still served
    expect(send.mock.calls.map((c) => c[0])).toEqual([1, 2])
    send.mockClear()
    hub.frame(1, new Uint8Array(64))
    expect(send.mock.calls.map((c) => c[0])).toEqual([2])
    expect(call.mock.calls).toEqual([['viz.subscribe']])
    send.mockImplementation(() => false)
    hub.frame(1, new Uint8Array(64)) // last subscriber fails
    expect(call.mock.calls).toEqual([['viz.subscribe'], ['viz.unsubscribe']])
  })

  it('hookSender registers one destroyed listener however often it is called, and drops on navigation or crash', () => {
    const drop = vi.fn()
    const hooked = new Set<number>()
    const wc = Object.assign(new EventEmitter(), { id: 7 })
    hookSender(wc, { drop }, hooked)
    hookSender(wc, { drop }, hooked)
    expect(wc.listenerCount('destroyed')).toBe(1)
    wc.emit('did-start-navigation', { isMainFrame: false, isSameDocument: false })
    wc.emit('did-start-navigation', { isMainFrame: true, isSameDocument: true })
    expect(drop).not.toHaveBeenCalled() // subframes and in-page navigations keep the subscription
    wc.emit('did-start-navigation', { isMainFrame: true, isSameDocument: false })
    expect(drop).toHaveBeenCalledTimes(1)
    wc.emit('render-process-gone')
    expect(drop).toHaveBeenCalledTimes(2)
    wc.emit('destroyed')
    expect(drop).toHaveBeenCalledTimes(3)
    expect(hooked.has(7)).toBe(false) // a new webContents reusing the id would be hooked afresh
  })
})
