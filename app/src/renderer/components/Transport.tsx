import { Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward } from 'lucide-react'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import s from './Transport.module.css'

// Actions are read from the store at click time so tests can swap them with setState.
const actions = () => usePlayer.getState()

export function Transport() {
  const playing = usePlayer((p) => p.state.status === 'playing')
  const shuffle = usePlayer((p) => p.state.shuffle)
  const repeat = usePlayer((p) => p.state.repeat)
  return (
    <div className={s.row}>
      <button aria-label="Shuffle" aria-pressed={shuffle} className={shuffle ? s.on : undefined} onClick={() => void actions().setShuffle(!shuffle)}><Shuffle size={16} /></button>
      <button aria-label="Previous" onClick={() => void actions().prev()}><SkipBack size={18} /></button>
      <button className={s.play} aria-label={playing ? 'Pause' : 'Play'} onClick={() => void actions().toggle()}>
        {playing ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}
      </button>
      <button aria-label="Next" onClick={() => void actions().next()}><SkipForward size={18} /></button>
      <button aria-label={S.repeatLabel(repeat)} className={repeat !== 'off' ? s.on : undefined} onClick={() => void actions().cycleRepeat()}>
        {repeat === 'one' ? <Repeat1 size={16} /> : <Repeat size={16} />}
      </button>
    </div>
  )
}
