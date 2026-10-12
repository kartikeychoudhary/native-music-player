import { useCallback, useEffect, useRef, useState } from 'react'
import { formatTime } from '../lib/format'
import { livePosition, usePlayer } from '../stores/player'
import s from './SeekBar.module.css'

export function SeekBar() {
  const state = usePlayer((p) => p.state)
  const stamp = usePlayer((p) => p.stamp)
  const [, tick] = useState(0)
  const [drag, setDrag] = useState<number | null>(null)
  const dragRef = useRef<number | null>(null)
  const setPending = (v: number | null) => { dragRef.current = v; setDrag(v) }
  /** Seeks to the dragged position and goes back to following playback. */
  const commit = useCallback(() => {
    const v = dragRef.current
    if (v == null) return
    dragRef.current = null
    setDrag(null)
    void usePlayer.getState().seek(v)
  }, [])
  /** Pointer releases commit on the next tick: the browser fires the range input's trailing change
   * event after pointerup, and handling it after an immediate commit would re-arm the drag. */
  const commitSoon = useCallback(() => { setTimeout(commit, 0) }, [commit])
  const dragging = drag != null
  useEffect(() => {
    if (!dragging) return
    // A mouse released past the slider's end lands on another element, so the slider never sees
    // pointerup: listen on the window while a drag is pending, or the bar freezes on the dragged value.
    const up = () => commitSoon()
    const cancel = () => { dragRef.current = null; setDrag(null) }
    window.addEventListener('pointerup', up, true)
    window.addEventListener('pointercancel', cancel, true)
    return () => { window.removeEventListener('pointerup', up, true); window.removeEventListener('pointercancel', cancel, true) }
  }, [dragging, commitSoon])
  useEffect(() => {
    if (state.status !== 'playing') return
    let raf = 0
    const loop = () => { tick((n) => n + 1); raf = requestAnimationFrame(loop) }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [state.status])
  const duration = state.durationMs ?? 0
  const pos = drag ?? livePosition({ state, stamp }, performance.now())
  const pct = duration > 0 ? Math.min(100, (pos / duration) * 100) : 0
  return (
    <div className={`${s.row} mono`}>
      <span className={s.time}>{formatTime(pos)}</span>
      <input
        className={s.range} type="range" min={0} max={duration || 1} step={100} value={Math.min(pos, duration || 1)}
        aria-label="Seek" aria-valuetext={`${formatTime(pos)} of ${formatTime(duration)}`} disabled={!duration} style={{ ['--pct' as string]: `${pct}%` }}
        onChange={(e) => setPending(Number(e.target.value))}
        onPointerUp={commitSoon}
        onPointerCancel={() => setPending(null)}
        onBlur={commit}
        onKeyUp={commit}
      />
      <span className={s.time}>{formatTime(duration)}</span>
    </div>
  )
}
