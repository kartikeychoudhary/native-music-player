import type { AlbumRow } from '@protocol/AlbumRow'
import type { TrackRow } from '@protocol/TrackRow'
import { FolderOpen, Info, ListEnd, ListPlus, ListStart, Play, User, Disc3 } from 'lucide-react'
import { S } from '../lib/strings'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi, type MenuItem } from '../stores/ui'

async function newPlaylistWith(trackIds: () => Promise<number[]>) {
  const name = await useUi.getState().askName({ title: S.newPlaylistTitle, confirm: S.create })
  if (!name) return
  const lib = useLibrary.getState()
  const p = await lib.createPlaylist(name)
  if (!p) return
  await addAndToast(p.id, p.name, trackIds)
}

async function addAndToast(id: number, name: string, trackIds: () => Promise<number[]>) {
  const ids = await trackIds()
  if (ids.length && (await useLibrary.getState().addToPlaylist(id, ids))) useUi.getState().toast('info', S.addedToPlaylist(name))
}

/** "Add to playlist ▸" for tracks resolved lazily (an album menu fetches its tracks only when used). */
export function playlistSubmenu(trackIds: () => Promise<number[]>): MenuItem {
  const playlists = useLibrary.getState().playlists
  return {
    label: S.addToPlaylist, icon: ListPlus,
    items: [
      { label: S.newPlaylist, onSelect: () => void newPlaylistWith(trackIds) },
      ...playlists.map((p) => ({
        label: p.name,
        onSelect: () => void addAndToast(p.id, p.name, trackIds),
      })),
    ],
  }
}

export function trackMenu(tracks: TrackRow[], go: (path: string) => void, extra: MenuItem[] = []): MenuItem[] {
  const ids = tracks.map((t) => t.id)
  const one = tracks.length === 1 ? tracks[0] : null
  const player = usePlayer.getState()
  return [
    { label: S.playNext, icon: ListStart, onSelect: () => void player.playNext(ids) },
    { label: S.addToQueue, icon: ListEnd, onSelect: () => void player.addToQueue(ids) },
    playlistSubmenu(async () => ids),
    ...extra,
    { label: '', separator: true },
    { label: S.goToAlbum, icon: Disc3, disabled: one?.albumId == null, onSelect: () => one?.albumId != null && go(`/albums/${one.albumId}`) },
    { label: S.goToArtist, icon: User, disabled: one?.artistId == null, onSelect: () => one?.artistId != null && go(`/artists/${one.artistId}`) },
    { label: S.showInFolder, icon: FolderOpen, disabled: !one || one.missing, onSelect: () => one && void player.api?.showItemInFolder(one.path) },
    { label: S.trackInfo, icon: Info, disabled: !one, onSelect: () => one && useUi.getState().showTrackInfo(one.id) },
  ]
}

/** An album's track ids; an engine failure becomes an error toast and an empty list. */
export async function albumTrackIds(id: number): Promise<number[]> {
  const api = usePlayer.getState().api
  if (!api) return []
  try {
    return (await api.call('library.album', { id })).tracks.map((t) => t.id)
  } catch (e) {
    useUi.getState().toast('error', e instanceof Error ? e.message : String(e))
    return []
  }
}

export async function playAlbum(id: number): Promise<void> {
  await usePlayer.getState().playTracks(await albumTrackIds(id))
}

export function albumMenu(album: AlbumRow, go: (path: string) => void): MenuItem[] {
  const player = usePlayer.getState()
  return [
    { label: S.play, icon: Play, onSelect: () => void playAlbum(album.id) },
    { label: S.playNext, icon: ListStart, onSelect: () => void albumTrackIds(album.id).then((ids) => player.playNext(ids)) },
    { label: S.addToQueue, icon: ListEnd, onSelect: () => void albumTrackIds(album.id).then((ids) => player.addToQueue(ids)) },
    playlistSubmenu(() => albumTrackIds(album.id)),
    { label: '', separator: true },
    { label: S.goToArtist, icon: User, disabled: album.artistId == null, onSelect: () => album.artistId != null && go(`/artists/${album.artistId}`) },
  ]
}
