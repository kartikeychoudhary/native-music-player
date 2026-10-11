import { useNavigate } from '@tanstack/react-router'
import { Disc3, ListMusic, Volume2, VolumeX } from 'lucide-react'
import { usePlayer } from '../stores/player'
import { DevicePicker } from './DevicePicker'
import { FormatBadges } from './FormatBadges'
import { SeekBar } from './SeekBar'
import { Transport } from './Transport'
import s from './PlayerBar.module.css'

export function PlayerBar() {
  const state = usePlayer((p) => p.state)
  const signal = usePlayer((p) => p.signal)
  const bitPerfect = state.mode === 'bitPerfect'
  const navigate = useNavigate()
  const { setVolume, setMode } = usePlayer.getState()
  return (
    <footer className={s.bar}>
      <div className={s.now}>
        <div className={s.art} aria-hidden><Disc3 size={22} /></div>
        <div className={s.meta}>
          <div className={s.title} data-testid="now-title">{state.track?.title ?? 'Nothing playing'}</div>
          <div className="muted">{state.track ? state.track.path.split(/[\\/]/).slice(-2, -1)[0] : 'Open files to start'}</div>
        </div>
      </div>
      <div className={s.center}>
        <Transport />
        <SeekBar />
      </div>
      <div className={s.right}>
        <FormatBadges signal={signal} />
        <button className={s.icon} aria-label="Queue" onClick={() => void navigate({ to: '/queue' })}><ListMusic size={16} /></button>
        <DevicePicker />
        <button
          role="switch" aria-checked={bitPerfect} aria-label="Bit-perfect" className={s.switch}
          onClick={() => void setMode(bitPerfect ? 'processed' : 'bitPerfect')}
          title={bitPerfect ? 'Bit-perfect: exclusive, untouched output' : 'Processed: shared output with volume'}
        >
          <span className="mono">BP</span>
        </button>
        <span className={s.vol}>
          {state.volume === 0 ? <VolumeX size={16} aria-hidden /> : <Volume2 size={16} aria-hidden />}
          <input
            type="range" min={0} max={1} step={0.01} aria-label="Volume" value={bitPerfect ? 1 : state.volume}
            disabled={bitPerfect} title={bitPerfect ? 'Bit-perfect: use your system or DAC volume' : `${Math.round(state.volume * 100)}%`}
            onChange={(e) => void setVolume(Number(e.target.value))}
          />
        </span>
      </div>
    </footer>
  )
}
