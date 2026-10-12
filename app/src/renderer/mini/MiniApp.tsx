import { Maximize2, Pause, Play, SkipBack, SkipForward, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import type { AuroraApi } from '@shared/api'
import { Artwork } from '../components/Artwork'
import { S } from '../lib/strings'
import { livePosition, usePlayer } from '../stores/player'
import s from './Mini.module.css'

/** The 340×110 always-on-top mini-player window (spec §5.2), rendered at `#/mini` without the app layout. */
export function MiniApp({ api }: { api: AuroraApi }) {
  const state = usePlayer((p) => p.state)
  const stamp = usePlayer((p) => p.stamp)
  const [, tick] = useState(0)
  const playing = state.status === 'playing'
  useEffect(() => {
    if (!playing) return
    const id = setInterval(() => tick((n) => n + 1), 250)
    return () => clearInterval(id)
  }, [playing])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === 'm' || e.key === 'M')) { e.preventDefault(); void api.toggleMini() }
      else if (e.code === 'Space' && !(e.target as Element | null)?.closest?.('button')) { e.preventDefault(); void usePlayer.getState().toggle() }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [api])
  const t = state.status !== 'stopped' ? state.track : null
  const dur = state.durationMs ?? 0
  const pct = dur > 0 ? Math.min(100, (livePosition({ state, stamp }, performance.now()) / dur) * 100) : 0
  const p = () => usePlayer.getState()
  return (
    <div className={s.mini}>
      <Artwork id={t?.artworkId ?? null} size={64} px={78} alt={t?.title ?? ''} className={s.art} />
      <div className={s.body}>
        <div className={s.top}>
          <div className={s.meta}>
            <div className={s.title}>{t?.title ?? S.nothingPlaying}</div>
            <div className={`${s.sub} muted`}>{t?.artist ?? ''}</div>
          </div>
          <div className={s.win}>
            <button aria-label={S.openAurora} onClick={() => void api.showMain()}><Maximize2 size={13} /></button>
            <button aria-label={S.closeMini} onClick={() => void api.toggleMini()}><X size={14} /></button>
          </div>
        </div>
        <div className={s.controls}>
          <button aria-label="Previous" onClick={() => void p().prev()}><SkipBack size={16} /></button>
          <button className={s.play} aria-label={playing ? 'Pause' : 'Play'} onClick={() => void p().toggle()}>
            {playing ? <Pause size={16} fill="currentColor" /> : <Play size={16} fill="currentColor" />}
          </button>
          <button aria-label="Next" onClick={() => void p().next()}><SkipForward size={16} /></button>
        </div>
        <progress className={s.progress} max={100} value={pct} aria-label="Progress" />
      </div>
    </div>
  )
}
