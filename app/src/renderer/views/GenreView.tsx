import { useParams } from '@tanstack/react-router'
import { AlbumGrid } from '../components/AlbumGrid'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { usePaged } from '../lib/usePaged'
import v from './views.module.css'

export function GenreView() {
  const { genreId } = useParams({ strict: false }) as { genreId?: string }
  const id = Number(genreId)
  const genre = useEngineQuery(['genre', id], (a) => a.call('library.genre', { id }))
  const paged = usePaged(['genre-albums', id], (a, offset, limit) => a.call('library.albums', { filter: { genreId: id }, offset, limit }))
  if (genre.error) return <p className={v.error}>{genre.error}</p>
  // useEngineQuery and usePaged keep the previous genre's data while the new one loads.
  if (!genre.data || genre.data.id !== id) return null
  return (
    <div className={v.view}>
      <div>
        <div className={v.kicker}>{S.genre.toUpperCase()}</div>
        <h1 className={v.title}>{genre.data.name}</h1>
        <div className={v.meta}>{S.albumCount(paged.total)}</div>
      </div>
      <AlbumGrid label={genre.data.name} count={paged.total} row={paged.row} onRange={paged.onRange} />
    </div>
  )
}
