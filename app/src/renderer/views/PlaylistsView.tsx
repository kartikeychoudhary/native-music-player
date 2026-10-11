import type { Playlist } from '@protocol/Playlist'
import { FileInput, Plus } from 'lucide-react'
import { formatDuration } from '../lib/format'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export async function newPlaylist(go?: (p: string) => void) {
  const name = await useUi.getState().askName({ title: S.newPlaylistTitle, confirm: S.create })
  if (!name) return
  const p = await useLibrary.getState().createPlaylist(name)
  if (p && go) go(`/playlists/${p.id}`)
}

async function importM3u(go: (p: string) => void) {
  const api = usePlayer.getState().api
  const path = await api?.pickM3u()
  if (!api || !path) return
  try {
    const r = await api.call('playlists.importM3u', { path })
    useUi.getState().toast('info', S.imported(r.playlist.name, r.skipped))
    void useLibrary.getState().refreshPlaylists()
    go(`/playlists/${r.playlist.id}`)
  } catch (e) {
    useUi.getState().toast('error', e instanceof Error ? e.message : String(e))
  }
}

export function PlaylistTile({ p }: { p: Playlist }) {
  const go = useGo()
  return (
    <button className={v.tile} onClick={() => go(`/playlists/${p.id}`)}>
      <span className={v.tileName}>{p.name}</span>
      <span className={v.meta}>{S.trackCount(p.trackCount)} · {formatDuration(p.durationMs)}</span>
    </button>
  )
}

export function PlaylistsView() {
  const playlists = useLibrary((l) => l.playlists)
  const go = useGo()
  return (
    <div className={v.view}>
      <div className={v.toolbar}>
        <h1 className={v.title}>{S.playlistsTitle}</h1>
        <div className={v.actions}>
          <button className={v.primary} onClick={() => void newPlaylist()}><Plus size={16} /> {S.newPlaylistButton}</button>
          <button className={v.ghost} onClick={() => void importM3u(go)}><FileInput size={16} /> {S.importM3u}</button>
        </div>
      </div>
      {playlists.length === 0 ? <p className="muted">{S.nothingYet}</p> : (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 14 }}>
          {playlists.map((p) => <PlaylistTile key={p.id} p={p} />)}
        </div>
      )}
    </div>
  )
}
