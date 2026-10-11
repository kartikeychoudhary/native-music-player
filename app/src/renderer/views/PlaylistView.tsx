import type { PlaylistItem } from '@protocol/PlaylistItem'
import { useParams } from '@tanstack/react-router'
import { ArrowDown, ArrowUp, FileOutput, Pencil, Play, Shuffle, Trash2, X } from 'lucide-react'
import { useState } from 'react'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DragHandle, SortableList, sortableStyles as ss } from '../components/SortableList'
import { trackMenu } from '../components/trackMenu'
import { formatDuration, formatTime } from '../lib/format'
import { S } from '../lib/strings'
import { useEngineQuery } from '../lib/useEngineQuery'
import { useGo } from '../lib/useGo'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

function arrayMove<T>(a: T[], from: number, to: number): T[] {
  const b = [...a]
  const [x] = b.splice(from, 1)
  b.splice(to, 0, x)
  return b
}

export function PlaylistView() {
  const { playlistId } = useParams({ strict: false }) as { playlistId?: string }
  const id = Number(playlistId)
  const go = useGo()
  const q = useEngineQuery(['playlist', id], (a) => a.call('playlists.get', { id }), ['playlists', 'tracks'])
  // Optimistic edits are tied to the fetched result they were made on, so a refetch (or another playlist) discards them.
  const [edit, setEdit] = useState<{ src: typeof q.data; items: PlaylistItem[] } | null>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)
  if (q.error) return <p className={v.error}>{q.error}</p>
  // useEngineQuery keeps the previous playlist's data while the new one loads.
  if (!q.data || q.data.playlist.id !== id) return null
  const pl = q.data.playlist
  const items = edit?.src === q.data ? edit.items : q.data.items
  const api = usePlayer.getState().api
  const player = usePlayer.getState()
  const ids = items.map((i) => i.track.id)
  const call = async (p: Promise<unknown> | undefined) => { try { await p } catch (e) { useUi.getState().toast('error', e instanceof Error ? e.message : String(e)) } }
  const move = (from: number, to: number) => {
    if (to < 0 || to >= items.length || from === to) return
    setEdit({ src: q.data, items: arrayMove(items, from, to) })
    void call(api?.call('playlists.reorder', { id, from, to }))
  }
  const remove = (itemIds: number[]) => {
    setEdit({ src: q.data, items: items.filter((i) => !itemIds.includes(i.itemId)) })
    void call(api?.call('playlists.removeItems', { id, itemIds }))
  }
  const rename = async () => {
    const name = await useUi.getState().askName({ title: S.renameTitle, initial: pl.name, confirm: S.save })
    if (name) await call(api?.call('playlists.rename', { id, name }))
  }
  const exportM3u = async () => {
    const path = await api?.pickM3uSave(pl.name)
    if (!path) return
    await call(api?.call('playlists.exportM3u', { id, path }).then(() => useUi.getState().toast('info', S.exported)))
  }
  const del = async () => {
    setConfirmDelete(false)
    await call(api?.call('playlists.delete', { id }))
    void useLibrary.getState().refreshPlaylists()
    go('/playlists')
  }
  return (
    <div className={v.view}>
      <header className={v.head} style={{ alignItems: 'center' }}>
        <div>
          <div className={v.kicker}>{S.playlist.toUpperCase()}</div>
          <h1 className={v.title}>{pl.name}</h1>
          <div className={v.meta}>{S.trackCount(pl.trackCount)} · {formatDuration(pl.durationMs)}</div>
          <div className={v.actions} style={{ marginTop: 14 }}>
            <button className={v.primary} aria-label={S.playPlaylist} disabled={!ids.length} onClick={() => void player.playTracks(ids, 0)}><Play size={16} fill="currentColor" /> {S.play}</button>
            <button className={v.ghost} aria-label={S.shufflePlaylist} disabled={!ids.length} onClick={() => void player.playTracks(ids, Math.floor(Math.random() * ids.length), { shuffle: true })}><Shuffle size={16} /> {S.shuffle}</button>
            <button className={v.ghost} onClick={() => void rename()}><Pencil size={16} /> {S.rename}</button>
            <button className={v.ghost} onClick={() => void exportM3u()}><FileOutput size={16} /> {S.exportM3u}</button>
            <button className={v.ghost} aria-label={S.deletePlaylist} onClick={() => setConfirmDelete(true)}><Trash2 size={16} /></button>
          </div>
        </div>
      </header>
      {items.length === 0 ? <p className="muted">{S.nothingYet}</p> : (
        <SortableList label={pl.name} items={items} getId={(i) => i.itemId} onMove={move}
          renderRow={(it, index, handle) => (
            <>
              <DragHandle {...handle} />
              <div className={ss.main} data-dim={it.track.missing || !!it.track.unplayable}
                onDoubleClick={() => void player.playTracks(ids, index)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  useUi.getState().openMenu(e.clientX, e.clientY, trackMenu([it.track], go, [
                    { label: S.moveUp, icon: ArrowUp, disabled: index === 0, onSelect: () => move(index, index - 1) },
                    { label: S.moveDown, icon: ArrowDown, disabled: index === items.length - 1, onSelect: () => move(index, index + 1) },
                    { label: S.removeFromPlaylist, icon: X, onSelect: () => remove([it.itemId]) },
                  ]))
                }}>
                <span>{it.track.missing ? '⚠ ' : ''}{it.track.title}</span>
                <span className="muted">{it.track.artist || S.unknownArtist}</span>
                <span className="mono muted">{it.track.durationMs != null ? formatTime(it.track.durationMs) : ''}</span>
                <span />
              </div>
            </>
          )} />
      )}
      {confirmDelete && <ConfirmDialog title={S.deletePlaylist} body={S.deleteConfirm(pl.name)} confirm={S.delete} onConfirm={() => void del()} onCancel={() => setConfirmDelete(false)} />}
    </div>
  )
}
