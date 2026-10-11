import { useParams } from '@tanstack/react-router'
import { Play } from 'lucide-react'
import { AlbumCard } from '../components/AlbumGrid'
import { TrackTable } from '../components/TrackTable'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

const PAGE = 1000 // the engine's per-request maximum

/** Every track of the artist ordered by album, paging past the engine's request limit; a failure becomes a toast. */
async function playAll(artistId: number) {
  const api = usePlayer.getState().api
  if (!api) return
  try {
    const ids: number[] = []
    for (;;) {
      const page = await api.call('library.tracks', { filter: { artistId }, sort: 'album', offset: ids.length, limit: PAGE })
      ids.push(...page.items.map((t) => t.id))
      if (!page.items.length || ids.length >= page.total) break
    }
    await usePlayer.getState().playTracks(ids)
  } catch (e) {
    useUi.getState().toast('error', e instanceof Error ? e.message : String(e))
  }
}

export function ArtistView() {
  const { artistId } = useParams({ strict: false }) as { artistId?: string }
  const id = Number(artistId)
  const q = useEngineQuery(['artist', id], (a) => a.call('library.artist', { id }), ['tracks', 'history'])
  if (q.error) return <p className={v.error}>{q.error}</p>
  // useEngineQuery keeps the previous artist's data while the new one loads.
  if (!q.data || q.data.artist.id !== id) return null
  const { artist, albums, topTracks } = q.data
  return (
    <div className={v.view}>
      <header className={v.head} style={{ alignItems: 'center' }}>
        <div>
          <div className={v.kicker}>{S.artist.toUpperCase()}</div>
          <h1 className={v.title}>{artist.name}</h1>
          <div className={v.meta}>{S.albumCount(artist.albumCount)} · {S.trackCount(artist.trackCount)}</div>
          <div className={v.actions} style={{ marginTop: 14 }}>
            <button className={v.primary} aria-label={S.playArtist} onClick={() => void playAll(id)}><Play size={16} fill="currentColor" /> {S.play}</button>
          </div>
        </div>
      </header>
      {topTracks.length > 0 && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.topTracks}</h2>
          <TrackTable label={S.topTracks} count={topTracks.length} row={(i) => topTracks[i]} columns={['title', 'album', 'plays', 'duration']}
            onPlay={(i) => void usePlayer.getState().playTracks(topTracks.map((t) => t.id), i)} />
        </section>
      )}
      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.albumsTitle}</h2>
        <div className={v.row} role="list" aria-label={S.albumsTitle} style={{ flexWrap: 'wrap' }}>{albums.map((a) => <AlbumCard key={a.id} a={a} />)}</div>
      </section>
    </div>
  )
}
