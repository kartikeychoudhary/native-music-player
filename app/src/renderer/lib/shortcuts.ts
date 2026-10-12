import { livePosition, type PlayerStore } from '../stores/player'
import { useUi } from '../stores/ui'
import { S } from './strings'

// Controls that own their own keyboard behaviour; global shortcuts must not steal keys from them.
const INTERACTIVE = 'button, [role=switch], [role=slider], [role=grid], [role=listbox], [role=menu], [role=dialog], a, input, select, textarea, [contenteditable="true"]'
const SEEK_STEP_MS = 5000
const VOLUME_STEP = 0.05

/** Listed in Settings (spec §5.6). Ctrl/Cmd+M (mini-player) is listed; M3 adds Ctrl/Cmd+L (lyrics). */
export const SHORTCUTS: { keys: string; action: string }[] = S.shortcutList

/** Applies a global shortcut. Returns true when the event was handled. `go` navigates (Layout passes it). */
export function handleShortcut(e: KeyboardEvent, store: PlayerStore, now: number, go?: (path: string) => void): boolean {
  if (e.defaultPrevented || e.altKey) return false
  if (e.ctrlKey || e.metaKey) {
    if (e.key === 'm' || e.key === 'M') { e.preventDefault(); void store.api?.toggleMini(); return true }
    if (e.key === 'l' || e.key === 'L') {
      if (!go) return false
      e.preventDefault()
      useUi.getState().setNowPlayingTab('lyrics')
      go('/now-playing')
      return true
    }
    const target = e.key === 'f' || e.key === 'F' ? '/search' : e.key === ',' ? '/settings' : null
    if (!target || !go) return false
    e.preventDefault()
    go(target)
    return true
  }
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
  if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
    if (store.state.mode === 'bitPerfect') return false
    e.preventDefault()
    const v = store.state.volume + (e.key === 'ArrowUp' ? VOLUME_STEP : -VOLUME_STEP)
    void store.setVolume(Math.round(Math.min(1, Math.max(0, v)) * 100) / 100)
    return true
  }
  return false
}
