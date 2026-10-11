import { FolderOpen, FolderPlus } from 'lucide-react'
import { AlbumCard } from '../components/AlbumGrid'
import { TrackTable } from '../components/TrackTable'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import v from './views.module.css'

function RecentTracks({ kind, title }: { kind: 'played' | 'mostPlayed'; title: string }) {
  const q = useEngineQuery(['recent', kind], (a) => a.call('library.recent', { kind, limit: 8 }), ['tracks', 'history'])
  const tracks = q.data?.tracks ?? []
  if (!tracks.length) return null
  return (
    <section className={v.section}>
      <h2 className={v.sectionTitle}>{title}</h2>
      <TrackTable label={title} count={tracks.length} row={(i) => tracks[i]} columns={kind === 'mostPlayed' ? ['title', 'artist', 'album', 'plays', 'duration'] : ['title', 'artist', 'album', 'duration']}
        onPlay={(i) => void usePlayer.getState().playTracks(tracks.map((t) => t.id), i)} />
    </section>
  )
}

export function HomeView() {
  const folders = useLibrary((l) => l.folders)
  const scan = useLibrary((l) => l.scan)
  const { openFiles, playPaths, api } = usePlayer.getState()
  const added = useEngineQuery(['recent', 'added'], (a) => a.call('library.recent', { kind: 'added', limit: 12 }))
  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const paths = api?.pathsForFiles([...e.dataTransfer.files]) ?? []
    if (paths.length) void playPaths(paths)
  }
  const scanning = scan && scan.phase !== 'done'
  return (
    <div className={v.view} onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <div className={v.toolbar}>
        <h1 className={v.title}>{S.homeTitle}</h1>
        {scanning && <span className={v.meta} role="status">{S.scanning(scan.processed, scan.total)}</span>}
      </div>
      {folders.length === 0 ? (
        <div className={v.empty}>
          <h2>{S.emptyLibrary}</h2>
          <p className="muted">{S.emptyLibraryHint}</p>
          <div className={v.actions}>
            <button className={v.primary} onClick={() => void useLibrary.getState().addFolder()}><FolderPlus size={16} /> {S.addFolder}</button>
            <button className={v.ghost} onClick={() => void openFiles()}><FolderOpen size={16} /> {S.openFiles}</button>
          </div>
          <p className="muted">{S.dropHint}</p>
        </div>
      ) : (
        <>
          {!!added.data?.albums.length && (
            <section className={v.section}>
              <h2 className={v.sectionTitle}>{S.recentlyAdded}</h2>
              <div className={v.row} role="list" aria-label={S.recentlyAdded}>{added.data.albums.map((a) => <AlbumCard key={a.id} a={a} />)}</div>
            </section>
          )}
          <RecentTracks kind="played" title={S.recentlyPlayed} />
          <RecentTracks kind="mostPlayed" title={S.mostPlayed} />
        </>
      )}
    </div>
  )
}
