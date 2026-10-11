import type { SortKey } from '@protocol/SortKey'
import { ArrowDownWideNarrow, ArrowUpNarrowWide } from 'lucide-react'
import { useState } from 'react'
import { AlbumGrid } from '../components/AlbumGrid'
import { S } from '../lib/strings'
import { usePaged } from '../lib/usePaged'
import v from './views.module.css'

const SORTS: SortKey[] = ['title', 'artist', 'year', 'addedAt']

export function AlbumsView() {
  const [sort, setSort] = useState<SortKey>('title')
  const [desc, setDesc] = useState(false)
  const paged = usePaged(['albums', sort, desc], (a, offset, limit) => a.call('library.albums', { sort, desc, offset, limit }))
  return (
    <div className={v.view}>
      <div className={v.toolbar}>
        <div>
          <h1 className={v.title}>{S.albumsTitle}</h1>
          <div className={v.meta}>{S.albumCount(paged.total)}</div>
        </div>
        <div className={v.actions}>
          <select className={v.select} aria-label={S.sortAlbums} value={sort}
            onChange={(e) => { const k = e.target.value as SortKey; setSort(k); setDesc(k === 'year' || k === 'addedAt') }}>
            {SORTS.map((k) => <option key={k} value={k}>{S.sortBy[k as keyof typeof S.sortBy]}</option>)}
          </select>
          <button className={v.ghost} aria-label={desc ? S.descending : S.ascending} title={desc ? S.descending : S.ascending} onClick={() => setDesc(!desc)}>
            {desc ? <ArrowDownWideNarrow size={16} /> : <ArrowUpNarrowWide size={16} />}
          </button>
        </div>
      </div>
      <AlbumGrid label={S.albumsTitle} count={paged.total} row={paged.row} onRange={paged.onRange} />
    </div>
  )
}
