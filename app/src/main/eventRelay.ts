/** Events the renderer must not miss: the engine sends some (e.g. "library unavailable") only once, right after it starts. */
const HELD_EVENTS = new Set(['engine.error', 'library.scanProgress', 'library.changed', 'dsp.changed'])

/**
 * Relays engine events to the renderer. Until `ready()` (the page has loaded and subscribed), and
 * whenever delivery fails, the events in HELD_EVENTS are kept (the most recent `limit`, and only
 * the newest scan progress) and replayed in order on the next `ready()`. Other events are
 * delivered or dropped as they come.
 */
export function createEventRelay(deliver: (event: string, payload: unknown) => boolean, limit = 20) {
  let isReady = false
  let held: [string, unknown][] = []
  const send = (event: string, payload: unknown) => {
    const hold = HELD_EVENTS.has(event)
    if ((isReady || !hold) && (deliver(event, payload) || !hold)) return
    // Each scan progress event supersedes the last, so only the newest is kept.
    if (event === 'library.scanProgress') held = held.filter(([e]) => e !== event)
    held.push([event, payload])
    if (held.length > limit) held.shift()
  }
  return {
    send,
    /** The renderer can receive events: replays the held ones. */
    ready() {
      isReady = true
      const replay = held
      held = []
      for (const [event, payload] of replay) send(event, payload)
    },
    /** The renderer is gone or reloading: hold events again until `ready()`. */
    reset() { isReady = false },
  }
}
