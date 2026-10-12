import { FRAME_SPECTRUM } from '@shared/api'

/** Spectrum subscriptions by window (webContents id). The engine computes the spectrum only while
 * at least one window wants it (spec §3.6), and forgets the subscription when it restarts. */
export function createVizHub(opts: { call: (method: 'viz.subscribe' | 'viz.unsubscribe') => Promise<unknown>; send: (windowId: number, data: Uint8Array) => boolean }) {
  const subs = new Set<number>()
  let engineOn = false
  const sync = () => {
    const want = subs.size > 0
    if (want === engineOn) return
    engineOn = want
    // A failed subscribe (engine not ready) is retried by engineReady(); a failed unsubscribe means the engine is gone anyway.
    opts.call(want ? 'viz.subscribe' : 'viz.unsubscribe').catch(() => { engineOn = false })
  }
  return {
    set(windowId: number, on: boolean) { if (on) subs.add(windowId); else subs.delete(windowId); sync() },
    drop(windowId: number) { subs.delete(windowId); sync() },
    engineReady() { engineOn = false; sync() },
    frame(type: number, data: Uint8Array) {
      if (type !== FRAME_SPECTRUM) return
      for (const id of subs) opts.send(id, data)
    },
  }
}
