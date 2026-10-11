import type { ArtistRow } from '@protocol/ArtistRow'
import { Search } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { AlbumCard } from '../components/AlbumGrid'
import { TrackTable } from '../components/TrackTable'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useGo } from '../lib/useGo'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

function ArtistChip({ a }: { a: ArtistRow }) {
  const go = useGo()
  return (
    <button className={v.tile} onClick={() => go(`/artists/${a.id}`)}>
      <span className={v.tileName}>{a.name}</span>
      <span className={v.meta}>{S.albumCount(a.albumCount)} · {S.trackCount(a.trackCount)}</span>
    </button>
  )
}

export function SearchView() {
  const text = useUi((u) => u.searchText)
  const [debounced, setDebounced] = useState(text)
  const input = useRef<HTMLInputElement>(null)
  useEffect(() => { input.current?.focus() }, [])
  useEffect(() => { const t = setTimeout(() => setDebounced(text.trim()), 150); return () => clearTimeout(t) }, [text])
  const ok = debounced.length >= 2
  const q = useEngineQuery(['search', ok ? debounced : ''], (a) => (ok ? a.call('library.search', { text: debounced, limit: 20 }) : Promise.resolve(null)))
  const r = ok ? q.data : null
  const empty = r && !r.tracks.length && !r.albums.length && !r.artists.length
  return (
    <div className={v.view}>
      <label style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '0 12px', height: 44, borderRadius: 6, background: 'var(--raised)', border: '1px solid var(--border)' }}>
        <Search size={18} className="muted" aria-hidden />
        <input ref={input} type="search" aria-label={S.searchTitle} placeholder={S.searchPlaceholder} value={text}
          onChange={(e) => useUi.getState().setSearchText(e.target.value)}
          style={{ flex: 1, background: 'transparent', border: 0, color: 'var(--text)', font: 'inherit', fontSize: 15, outline: 'none' }} />
      </label>
      {text.trim().length < 2 && <p className="muted">{S.searchHint}</p>}
      {empty && <p className="muted">{S.noResults}</p>}
      {!!r?.artists.length && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.artistsTitle}</h2>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 10 }}>{r.artists.slice(0, 8).map((a) => <ArtistChip key={a.id} a={a} />)}</div>
        </section>
      )}
      {!!r?.albums.length && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.albumsTitle}</h2>
          <div className={v.row} role="list" aria-label={S.albumsTitle}>{r.albums.map((a) => <AlbumCard key={a.id} a={a} />)}</div>
        </section>
      )}
      {!!r?.tracks.length && (
        <section className={v.section}>
          <h2 className={v.sectionTitle}>{S.tracksTitle}</h2>
          <TrackTable label={S.tracksTitle} count={r.tracks.length} row={(i) => r.tracks[i]} columns={['title', 'artist', 'album', 'format', 'duration']}
            onPlay={(i) => void usePlayer.getState().playTracks(r.tracks.map((t) => t.id), i)} />
        </section>
      )}
    </div>
  )
}
