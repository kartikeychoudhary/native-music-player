import { useEffect, useState } from 'react'
import { formatTime } from '../lib/format'
import { livePosition, usePlayer } from '../stores/player'
import s from './SeekBar.module.css'

export function SeekBar() {
  const state = usePlayer((p) => p.state)
  const stamp = usePlayer((p) => p.stamp)
  const seek = usePlayer((p) => p.seek)
  const [, tick] = useState(0)
  const [drag, setDrag] = useState<number | null>(null)
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
        aria-label="Seek" disabled={!duration} style={{ ['--pct' as string]: `${pct}%` }}
        onChange={(e) => setDrag(Number(e.target.value))}
        onPointerUp={() => { if (drag != null) { void seek(drag); setDrag(null) } }}
        onKeyUp={() => { if (drag != null) { void seek(drag); setDrag(null) } }}
      />
      <span className={s.time}>{formatTime(duration)}</span>
    </div>
  )
}
