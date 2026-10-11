import { Pause, Play, Repeat, Shuffle, SkipBack, SkipForward } from 'lucide-react'
import { usePlayer } from '../stores/player'
import s from './Transport.module.css'

// Actions are read from the store at click time so tests can swap them with setState.
const actions = () => usePlayer.getState()

export function Transport() {
  const playing = usePlayer((p) => p.state.status === 'playing')
  return (
    <div className={s.row}>
      <button aria-label="Shuffle" title="Shuffle arrives with the library (M2)" disabled><Shuffle size={16} /></button>
      <button aria-label="Previous" onClick={() => void actions().prev()}><SkipBack size={18} /></button>
      <button className={s.play} aria-label={playing ? 'Pause' : 'Play'} onClick={() => void actions().toggle()}>
        {playing ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}
      </button>
      <button aria-label="Next" onClick={() => void actions().next()}><SkipForward size={18} /></button>
      <button aria-label="Repeat" title="Repeat arrives with the queue (M2)" disabled><Repeat size={16} /></button>
    </div>
  )
}
