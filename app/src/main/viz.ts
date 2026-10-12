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
      // A window that can't take the frame is gone (or reloading): stop feeding it. Deleting during Set iteration is safe.
      for (const id of subs) if (!opts.send(id, data)) { subs.delete(id); sync() }
    },
  }
}

/** The slice of webContents that hookSender needs. */
export interface SenderLike {
  id: number
  on(event: string, listener: (...args: any[]) => void): unknown
  once(event: string, listener: (...args: any[]) => void): unknown
}

/**
 * Ties a window's subscription to its webContents' lifetime, once per sender: the subscription ends when the
 * window is destroyed, its renderer dies, or its main frame navigates/reloads (the page re-subscribes on mount).
 */
export function hookSender(wc: SenderLike, hub: { drop(windowId: number): void }, hooked: Set<number>) {
  const id = wc.id
  if (hooked.has(id)) return
  hooked.add(id)
  wc.once('destroyed', () => { hooked.delete(id); hub.drop(id) })
  wc.on('render-process-gone', () => hub.drop(id))
  wc.on('did-start-navigation', (details: { isMainFrame: boolean; isSameDocument: boolean }) => {
    if (details.isMainFrame && !details.isSameDocument) hub.drop(id)
  })
}
