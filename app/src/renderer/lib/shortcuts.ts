import { livePosition, type PlayerStore } from '../stores/player'

// Controls that own their own keyboard behaviour; global shortcuts must not steal keys from them.
const INTERACTIVE = 'button, [role=switch], [role=slider], a, input, select, textarea, [contenteditable="true"]'
const SEEK_STEP_MS = 5000

/** Applies a global playback shortcut. Returns true when the event was handled. */
export function handleShortcut(e: KeyboardEvent, store: PlayerStore, now: number): boolean {
  if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return false
  if ((e.target as Element | null)?.closest?.(INTERACTIVE)) return false
  if (e.code === 'Space') {
    e.preventDefault()
    void store.toggle()
    return true
  }
  if (e.key === 'ArrowRight' && e.shiftKey) { void store.next(); return true }
  if (e.key === 'ArrowLeft' && e.shiftKey) { void store.prev(); return true }
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
    const dir = e.key === 'ArrowRight' ? 1 : -1
    const max = store.state.durationMs ?? Infinity
    void store.seek(Math.min(max, Math.max(0, livePosition(store, now) + dir * SEEK_STEP_MS)))
    return true
  }
  return false
}
