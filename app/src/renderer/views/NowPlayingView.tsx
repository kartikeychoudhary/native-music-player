import { useEngineQuery } from '../lib/useEngineQuery'
import { Artwork } from '../components/Artwork'
import { FormatBadges } from '../components/FormatBadges'
import { LyricsView } from '../components/LyricsView'
import { SignalPathPanel } from '../components/SignalPathPanel'
import { Spectrum } from '../components/Spectrum'
import { formatTime } from '../lib/format'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import { useUi, type NowPlayingTab } from '../stores/ui'
import { QueueView } from './QueueView'
import s from './NowPlaying.module.css'

const TABS: NowPlayingTab[] = ['lyrics', 'queue', 'signal', 'credits']

function Credits({ trackId }: { trackId: number }) {
  const q = useEngineQuery(['track-info', trackId], (a) => a.call('library.track', { id: trackId }), [])
  const d = q.data
  if (!d) return null
  const t = d.track
  const rows: [string, string | null | undefined][] = [
    [S.credits.composer, d.composer], [S.credits.albumArtist, t.albumArtist], [S.credits.album, t.album], [S.credits.year, t.year?.toString()],
    [S.credits.genre, d.genres.map((g) => g.name).join(', ') || t.genre], [S.credits.track, t.trackNo ? `${t.trackNo}${d.trackTotal ? ` / ${d.trackTotal}` : ''}` : null],
    [S.credits.disc, t.discNo ? `${t.discNo}${d.discTotal ? ` / ${d.discTotal}` : ''}` : null],
    [S.credits.format, [t.codec?.toUpperCase(), t.bitDepth && `${t.bitDepth}-bit`, t.sampleRate && `${t.sampleRate / 1000} kHz`, t.bitrateKbps && `${t.bitrateKbps} kbps`].filter(Boolean).join(' · ')],
    [S.credits.file, t.path], [S.credits.mbRecording, d.musicbrainz.recording], [S.credits.mbRelease, d.musicbrainz.release],
  ]
  return (
    <dl className={s.credits}>
      {rows.filter(([, v]) => v).map(([k, v]) => <div key={k}><dt className="muted">{k}</dt><dd className={k === S.credits.format || k === S.credits.file ? 'mono' : undefined}>{v}</dd></div>)}
    </dl>
  )
}

export function NowPlayingView() {
  const state = usePlayer((p) => p.state)
  const signal = usePlayer((p) => p.signal)
  const tab = useUi((u) => u.nowPlayingTab)
  const t = state.status !== 'stopped' ? state.track : null
  const artId = t?.artworkId ?? null
  const art = useEngineQuery(['artwork', artId], async (a) => (artId == null ? null : a.call('library.artwork', { id: artId, size: 300 })), [])
  const muted = art.data?.palette?.muted
  if (!t) return <div className={s.view} data-testid="now-playing"><p className="muted">{S.nothingPlaying}</p></div>
  return (
    <div className={s.view} data-testid="now-playing" style={muted ? { background: `linear-gradient(180deg, ${muted} 0%, var(--bg) 75%)` } : undefined}>
      <section className={s.left}>
        <Artwork id={artId} size={1000} px={360} alt={t.title} className={s.art} />
        <h1 className={s.title}>{t.title}</h1>
        <div className={s.sub}>{[t.artist, t.album].filter(Boolean).join(' · ')}</div>
        <div className={s.badges}><FormatBadges signal={signal} /><span className="mono muted">{formatTime(state.durationMs ?? 0)}</span></div>
        <Spectrum className={s.spectrum} height={96} />
      </section>
      <section className={s.right}>
        <div role="tablist" aria-label={S.nowPlaying} className={s.tabs}>
          {TABS.map((k) => (
            <button key={k} role="tab" id={`np-tab-${k}`} aria-selected={tab === k} aria-controls="np-panel" onClick={() => useUi.getState().setNowPlayingTab(k)}>{S.tabs[k]}</button>
          ))}
        </div>
        <div role="tabpanel" id="np-panel" aria-labelledby={`np-tab-${tab}`} className={s.panel}>
          {/* Keyed so each track gets fresh query state: useEngineQuery keeps the previous key's data while loading. */}
          {tab === 'lyrics' && <LyricsView key={t.trackId ?? 'none'} trackId={t.trackId ?? null} />}
          {tab === 'queue' && <QueueView />}
          {tab === 'signal' && <SignalPathPanel signal={signal} />}
          {tab === 'credits' && (t.trackId != null ? <Credits trackId={t.trackId} /> : <p className="muted">{S.lyricsLibraryOnly}</p>)}
        </div>
      </section>
    </div>
  )
}
