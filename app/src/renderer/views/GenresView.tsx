import type { GenreRow } from '@protocol/GenreRow'
import { VirtualGrid } from '../components/AlbumGrid'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { usePaged } from '../lib/usePaged'
import v from './views.module.css'

function GenreTile({ g }: { g: GenreRow }) {
  const go = useGo()
  return (
    <button className={v.tile} role="listitem" onClick={() => go(`/genres/${g.id}`)}>
      <span className={v.tileName}>{g.name}</span>
      <span className={v.meta}>{S.albumCount(g.albumCount)} · {S.trackCount(g.trackCount)}</span>
    </button>
  )
}

export function GenresView() {
  const paged = usePaged(['genres'], (a, offset, limit) => a.call('library.genres', { offset, limit }))
  return (
    <div className={v.view}>
      <h1 className={v.title}>{S.genresTitle}</h1>
      <VirtualGrid label={S.genresTitle} count={paged.total} row={paged.row} onRange={paged.onRange} render={(g) => <GenreTile g={g} />} keyOf={(g) => g.id} height={96} />
    </div>
  )
}
