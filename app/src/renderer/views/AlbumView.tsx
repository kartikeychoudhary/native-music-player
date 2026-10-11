import { useParams } from '@tanstack/react-router'
import { ListEnd, MoreHorizontal, Play, Shuffle } from 'lucide-react'
import { Artwork } from '../components/Artwork'
import { TrackTable, type Column } from '../components/TrackTable'
import { albumMenu } from '../components/trackMenu'
import { albumFormat, formatDuration } from '../lib/format'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useGo } from '../lib/useGo'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export function AlbumView() {
  const { albumId } = useParams({ strict: false }) as { albumId?: string }
  const id = Number(albumId)
  const go = useGo()
  const q = useEngineQuery(['album', id], (a) => a.call('library.album', { id }))
  const artId = q.data?.album.artworkId ?? null
  // Tagged with its artwork id so the previous album's palette is never applied while the next one loads.
  const art = useEngineQuery(['artwork', artId], async (a) => ({ id: artId, info: artId == null ? null : await a.call('library.artwork', { id: artId, size: 300 }) }), [])
  if (q.error) return <p className={v.error}>{q.error}</p>
  // useEngineQuery keeps the previous album's data while the new one loads.
  if (!q.data || q.data.album.id !== id) return null
  const { album: al, tracks } = q.data
  const ids = tracks.map((t) => t.id)
  const muted = art.data?.id === al.artworkId ? art.data?.info?.palette?.muted : undefined
  const various = tracks.some((t) => t.artist !== al.artist)
  const columns: Column[] = various ? ['index', 'title', 'artist', 'format', 'duration'] : ['index', 'title', 'format', 'duration']
  const player = usePlayer.getState()
  return (
    <div className={v.view}>
      <header data-testid="album-head" className={v.head}
        style={muted ? { background: `linear-gradient(180deg, color-mix(in srgb, ${muted} 55%, var(--bg)) 0%, var(--bg) 100%)` } : undefined}>
        <Artwork id={al.artworkId} size={1000} px={220} alt={al.title} />
        <div>
          <div className={v.kicker}>{S.album.toUpperCase()}</div>
          <h1 className={v.title}>{al.title || S.unknownAlbum}</h1>
          <div className={v.meta}>
            {al.artistId != null ? <a href={`#/artists/${al.artistId}`} onClick={(e) => { e.preventDefault(); go(`/artists/${al.artistId}`) }}>{al.artist}</a> : (al.artist || S.unknownArtist)}
            {al.year ? ` · ${al.year}` : ''} · {S.trackCount(al.trackCount)} · {formatDuration(al.durationMs)}
          </div>
          <div className={v.meta} style={{ marginTop: 4 }}>{albumFormat(al)}</div>
          <div className={v.actions} style={{ marginTop: 16 }}>
            <button className={v.primary} aria-label={S.playAlbum} onClick={() => void player.playTracks(ids, 0)}><Play size={16} fill="currentColor" /> {S.play}</button>
            <button className={v.ghost} aria-label={S.shuffleAlbum} onClick={() => void player.playTracks(ids, Math.floor(Math.random() * ids.length), { shuffle: true })}><Shuffle size={16} /> {S.shuffle}</button>
            <button className={v.ghost} onClick={() => void player.addToQueue(ids)}><ListEnd size={16} /> {S.addToQueue}</button>
            <button className={v.ghost} aria-label="More" onClick={(e) => { const r = e.currentTarget.getBoundingClientRect(); useUi.getState().openMenu(r.left, r.bottom + 4, albumMenu(al, go)) }}><MoreHorizontal size={16} /></button>
          </div>
        </div>
      </header>
      <TrackTable label={al.title || S.unknownAlbum} count={tracks.length} row={(i) => tracks[i]} columns={columns} onPlay={(i) => void player.playTracks(ids, i)} />
    </div>
  )
}
