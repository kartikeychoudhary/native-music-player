import { useNavigate, useRouter, useRouterState } from '@tanstack/react-router'
import { Heart, ListMusic, Mic2, PictureInPicture2, Volume2, VolumeX } from 'lucide-react'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import { Artwork } from './Artwork'
import { DevicePicker } from './DevicePicker'
import { FormatBadges } from './FormatBadges'
import { SeekBar } from './SeekBar'
import { Transport } from './Transport'
import { playlistSubmenu } from './trackMenu'
import s from './PlayerBar.module.css'

export function PlayerBar() {
  const state = usePlayer((p) => p.state)
  const signal = usePlayer((p) => p.signal)
  const bitPerfect = state.mode === 'bitPerfect'
  const navigate = useNavigate()
  const router = useRouter()
  const onQueue = useRouterState({ select: (r) => r.location.pathname === '/queue' })
  const onNowPlaying = useRouterState({ select: (r) => r.location.pathname === '/now-playing' })
  const npTab = useUi((u) => u.nowPlayingTab)
  const onLyrics = onNowPlaying && npTab === 'lyrics'
  const openNowPlaying = () => void navigate({ to: '/now-playing' })
  const { setVolume, setMode } = usePlayer.getState()
  const t = state.track
  const sub = t ? (t.artist ?? t.album ?? t.path.split(/[\\/]/).slice(-2, -1)[0]) : 'Open files to start'
  // The heart lists the playlists directly instead of opening a one-item "Add to playlist" submenu first.
  const like = (e: React.MouseEvent) => {
    const id = t?.trackId
    if (id == null) return
    const r = e.currentTarget.getBoundingClientRect()
    useUi.getState().openMenu(r.left, r.top - 8, playlistSubmenu(async () => [id]).items ?? [])
  }
  return (
    <footer className={s.bar}>
      <div className={s.now}>
        <button className={s.artButton} aria-label={S.openNowPlaying} onClick={openNowPlaying}><Artwork id={t?.artworkId ?? null} size={64} px={46} alt={t?.title ?? ''} /></button>
        <div className={s.meta}>
          <button className={`${s.title} ${s.linkish}`} data-testid="now-title" onClick={openNowPlaying}>{t?.title ?? S.nothingPlaying}</button>
          <div className={`${s.sub} muted`}>{sub}</div>
        </div>
        <button className={s.icon} aria-label={S.addCurrentToPlaylist} disabled={t?.trackId == null} onClick={like}><Heart size={16} /></button>
      </div>
      <div className={s.center}>
        <Transport />
        <SeekBar />
      </div>
      <div className={s.right}>
        <span className={s.badges}><FormatBadges signal={signal} /></span>
        <button className={s.icon} aria-label="Queue" aria-pressed={onQueue} onClick={() => (onQueue ? (router.history.canGoBack() ? router.history.back() : void navigate({ to: '/' })) : void navigate({ to: '/queue' }))}><ListMusic size={16} /></button>
        <button className={s.icon} aria-label={S.tabs.lyrics} aria-pressed={onLyrics} onClick={() => {
          if (onLyrics) { if (router.history.canGoBack()) router.history.back(); else void navigate({ to: '/' }) }
          else { useUi.getState().setNowPlayingTab('lyrics'); openNowPlaying() }
        }}><Mic2 size={16} /></button>
        <button className={s.icon} aria-label={S.miniPlayer} onClick={() => void usePlayer.getState().api?.toggleMini()}><PictureInPicture2 size={16} /></button>
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
