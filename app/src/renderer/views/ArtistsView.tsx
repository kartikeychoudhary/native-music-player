import type { ArtistRow } from '@protocol/ArtistRow'
import { VirtualGrid } from '../components/AlbumGrid'
import { Artwork } from '../components/Artwork'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { usePaged } from '../lib/usePaged'
import g from '../components/AlbumGrid.module.css'
import v from './views.module.css'

function ArtistCard({ a }: { a: ArtistRow }) {
  const go = useGo()
  return (
    <div className={g.card} role="listitem" onClick={() => go(`/artists/${a.id}`)}>
      <div className={g.cover}><Artwork id={a.artworkId} size={300} px={180} alt="" /></div>
      <a href={`#/artists/${a.id}`} className={g.title} style={{ color: 'inherit', textDecoration: 'none' }} onClick={(e) => { e.preventDefault(); go(`/artists/${a.id}`) }}>{a.name}</a>
      <div className={g.sub}>{S.albumCount(a.albumCount)} · {S.trackCount(a.trackCount)}</div>
    </div>
  )
}

export function ArtistsView() {
  const paged = usePaged(['artists'], (a, offset, limit) => a.call('library.artists', { offset, limit }))
  return (
    <div className={v.view}>
      <div>
        <h1 className={v.title}>{S.artistsTitle}</h1>
        <div className={v.meta}>{paged.total} {S.artistsTitle.toLowerCase()}</div>
      </div>
      <VirtualGrid label={S.artistsTitle} count={paged.total} row={paged.row} onRange={paged.onRange} render={(a) => <ArtistCard a={a} />} keyOf={(a) => a.id} height={240} />
    </div>
  )
}
