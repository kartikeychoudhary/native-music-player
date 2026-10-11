import { codecLabel, formatTime, khz, trackFormat } from '../lib/format'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useUi } from '../stores/ui'
import { Dialog } from './Dialog'
import s from './ui.module.css'

const db = (v: number | null) => (v == null ? '—' : `${v.toFixed(2)} dB`)
const mb = (bytes: number) => `${(bytes / 1_048_576).toFixed(1)} MB`
const date = (ms: number | null) => (ms == null ? '—' : new Date(ms).toLocaleString())

export function TrackInfoDialog() {
  const id = useUi((u) => u.trackInfo)
  if (id == null) return null
  return <TrackInfoBody key={id} id={id} />
}

function TrackInfoBody({ id }: { id: number }) {
  const close = () => useUi.getState().showTrackInfo(null)
  const q = useEngineQuery(['track-info', id], (a) => a.call('library.track', { id }), [])
  const d = q.data
  if (!d) return q.error ? <Dialog title={S.trackInfo} onClose={close}><p>{q.error}</p></Dialog> : null
  const t = d.track
  const rows: [string, React.ReactNode, boolean?][] = [
    [S.info.artists, d.artists.map((a) => a.name).join(', ') || S.unknownArtist],
    [S.info.album, t.album || S.unknownAlbum],
    [S.info.albumArtist, t.albumArtist ?? '—'],
    [S.info.trackDisc, `${t.trackNo ?? '—'} / ${d.trackTotal ?? '—'} · ${t.discNo ?? 1} / ${d.discTotal ?? 1}`],
    [S.info.year, t.year ?? '—'],
    [S.info.genres, d.genres.map((g) => g.name).join(', ') || '—'],
    [S.info.composer, d.composer ?? '—'],
    [S.info.duration, t.durationMs != null ? formatTime(t.durationMs) : '—', true],
    [S.info.format, t.codec ? `${trackFormat(t)} (${codecLabel({ codec: t.codec, container: t.container } as never)}, ${t.container.toUpperCase()})` : '—', true],
    [S.info.bitrate, t.bitrateKbps ? `${t.bitrateKbps} kbps` : '—', true],
    [S.info.sampleRate, t.sampleRate ? `${khz(t.sampleRate)} kHz` : '—', true],
    [S.info.bitDepth, t.bitDepth ? `${t.bitDepth}-bit` : '—', true],
    [S.info.channels, t.channels ?? '—', true],
    [S.info.gapless, d.gapless ? `${d.gapless.delay} / ${d.gapless.padding}` : '—', true],
    [S.info.replayGain, `${db(d.replayGain.trackGain)} / ${db(d.replayGain.albumGain)}`, true],
    [S.info.musicbrainz, d.musicbrainz.recording ?? '—', true],
    [S.info.lyrics, d.hasLyrics ? S.info.yes : S.info.no],
    [S.info.path, t.path, true],
    [S.info.size, mb(d.sizeBytes), true],
    [S.info.modified, date(d.mtime)],
    [S.info.added, date(t.addedAt * 1000)],
    [S.info.plays, t.playCount, true],
    [S.info.lastPlayed, date(t.lastPlayedAt == null ? null : t.lastPlayedAt * 1000)],
    [S.info.status, t.missing ? S.missingFile : t.unplayable ? S.unplayable(t.unplayable) : '—'],
  ]
  return (
    <Dialog title={t.title} onClose={close}>
      <dl style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: '6px 16px', margin: 0, userSelect: 'text' }}>
        {rows.map(([k, v, mono]) => (
          <div key={k} style={{ display: 'contents' }}>
            <dt className="muted">{k}</dt>
            <dd className={mono ? 'mono' : undefined} style={{ margin: 0, wordBreak: 'break-all' }}>{v}</dd>
          </div>
        ))}
      </dl>
      <div className={s.actions}><button className={s.secondary} onClick={close}>{S.info.close}</button></div>
    </Dialog>
  )
}
