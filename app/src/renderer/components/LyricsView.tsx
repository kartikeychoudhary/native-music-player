import { useEffect, useRef, useState } from 'react'
import { activeLine, activeWord } from '../lib/lyrics'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { livePosition, usePlayer } from '../stores/player'
import s from './LyricsView.module.css'

const reducedMotion = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches

export function LyricsView({ trackId }: { trackId: number | null }) {
  const status = usePlayer((p) => p.state.status)
  const state = usePlayer((p) => p.state)
  const stamp = usePlayer((p) => p.stamp)
  const q = useEngineQuery(['lyrics', trackId], (a) => (trackId == null ? Promise.resolve(null) : a.call('lyrics.get', { trackId })), ['tracks'])
  const [, tick] = useState(0)
  const currentRef = useRef<HTMLLIElement | null>(null)
  useEffect(() => {
    if (status !== 'playing') return
    const id = setInterval(() => tick((n) => n + 1), 100)
    return () => clearInterval(id)
  }, [status])
  const lyrics = q.data
  const ms = livePosition({ state, stamp }, performance.now())
  const idx = lyrics?.synced ? activeLine(lyrics.lines, ms) : -1
  useEffect(() => { currentRef.current?.scrollIntoView?.({ block: 'center', behavior: reducedMotion() ? 'auto' : 'smooth' }) }, [idx])
  if (status === 'stopped' || !state.track) return <p className={`${s.message} muted`}>{S.nothingPlaying}</p>
  if (trackId == null) return <p className={`${s.message} muted`}>{S.lyricsLibraryOnly}</p>
  if (q.loading && !lyrics) return null
  if (!lyrics || lyrics.lines.length === 0) return <p className={`${s.message} muted`}>{S.noLyrics}</p>
  const seek = usePlayer.getState().seek
  return (
    <ol className={s.lyrics} aria-label={S.lyrics} data-synced={lyrics.synced || undefined}>
      {lyrics.lines.map((line, i) => {
        const current = i === idx
        const word = current && line.words ? activeWord(line.words, ms) : -1
        return (
          <li key={i} ref={current ? currentRef : undefined} aria-current={current ? 'true' : undefined} className={line.text ? undefined : s.gap}>
            {lyrics.synced && line.tMs != null ? (
              <button className={s.line} onClick={() => void seek(line.tMs!)}>
                {line.words ? line.words.map((w, j) => <span key={j} data-sung={current && j <= word ? '' : undefined}>{w.text}</span>) : line.text}
              </button>
            ) : <span className={s.line}>{line.text}</span>}
          </li>
        )
      })}
    </ol>
  )
}
