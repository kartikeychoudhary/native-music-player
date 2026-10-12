import type { AlbumDetail } from '@protocol/AlbumDetail'
import type { AlbumRow } from '@protocol/AlbumRow'
import type { ArtistDetail } from '@protocol/ArtistDetail'
import type { ArtistRow } from '@protocol/ArtistRow'
import type { ArtworkInfo } from '@protocol/ArtworkInfo'
import type { ArtworkParams } from '@protocol/ArtworkParams'
import type { CrossfadeParams } from '@protocol/CrossfadeParams'
import type { DeviceInfo } from '@protocol/DeviceInfo'
import type { DeviceParams } from '@protocol/DeviceParams'
import type { DspState } from '@protocol/DspState'
import type { EnabledParams } from '@protocol/EnabledParams'
import type { EngineErrorEvent } from '@protocol/EngineErrorEvent'
import type { EqPreset } from '@protocol/EqPreset'
import type { ExportParams } from '@protocol/ExportParams'
import type { Folder } from '@protocol/Folder'
import type { GenreRow } from '@protocol/GenreRow'
import type { IdParams } from '@protocol/IdParams'
import type { ImportResult } from '@protocol/ImportResult'
import type { LibraryChanged } from '@protocol/LibraryChanged'
import type { ListQuery } from '@protocol/ListQuery'
import type { Lyrics } from '@protocol/Lyrics'
import type { LyricsParams } from '@protocol/LyricsParams'
import type { ModeParams } from '@protocol/ModeParams'
import type { NameParams } from '@protocol/NameParams'
import type { Page } from '@protocol/Page'
import type { PathParams } from '@protocol/PathParams'
import type { PlayParams } from '@protocol/PlayParams'
import type { PlayerState } from '@protocol/PlayerState'
import type { Playlist } from '@protocol/Playlist'
import type { PlaylistAddParams } from '@protocol/PlaylistAddParams'
import type { PlaylistDetail } from '@protocol/PlaylistDetail'
import type { PlaylistRemoveParams } from '@protocol/PlaylistRemoveParams'
import type { PlaylistReorderParams } from '@protocol/PlaylistReorderParams'
import type { QueueChanged } from '@protocol/QueueChanged'
import type { QueueMoveParams } from '@protocol/QueueMoveParams'
import type { QueueRemoveParams } from '@protocol/QueueRemoveParams'
import type { QueueSetParams } from '@protocol/QueueSetParams'
import type { QueueSetPathsParams } from '@protocol/QueueSetPathsParams'
import type { RecentParams } from '@protocol/RecentParams'
import type { RecentResult } from '@protocol/RecentResult'
import type { RenameParams } from '@protocol/RenameParams'
import type { RepeatParams } from '@protocol/RepeatParams'
import type { ReplayGainParams } from '@protocol/ReplayGainParams'
import type { RescanParams } from '@protocol/RescanParams'
import type { ScanProgress } from '@protocol/ScanProgress'
import type { SearchParams } from '@protocol/SearchParams'
import type { SearchResults } from '@protocol/SearchResults'
import type { SeekParams } from '@protocol/SeekParams'
import type { SetEqParams } from '@protocol/SetEqParams'
import type { ShuffleParams } from '@protocol/ShuffleParams'
import type { SignalPath } from '@protocol/SignalPath'
import type { TrackDetail } from '@protocol/TrackDetail'
import type { TrackIdsParams } from '@protocol/TrackIdsParams'
import type { TrackRow } from '@protocol/TrackRow'
import type { VolumeParams } from '@protocol/VolumeParams'
import type { EngineReady } from '@protocol/EngineReady'
import type { Settings } from './settings'

export type EngineStatus = { state: 'starting' | 'ready' | 'restarting' | 'failed' | 'mismatch'; message?: string; engine?: EngineReady }

export interface AppInfo { version: string; variant: 'standard' | 'nofdk'; platform: string; arch: string; licenses: boolean }
type None = Record<string, never>
type M<P, R> = { params: P; result: R }

export type Methods = {
  'player.play': M<PlayParams, null>
  'player.pause': M<None, null>
  'player.toggle': M<None, null>
  'player.stop': M<None, null>
  'player.next': M<None, null>
  'player.prev': M<None, null>
  'player.seek': M<SeekParams, null>
  'player.setVolume': M<VolumeParams, null>
  'player.setMode': M<ModeParams, null>
  'player.setDevice': M<DeviceParams, null>
  'player.state': M<None, PlayerState>
  'queue.setPaths': M<QueueSetPathsParams, null>
  'queue.set': M<QueueSetParams, null>
  'queue.add': M<TrackIdsParams, null>
  'queue.insertNext': M<TrackIdsParams, null>
  'queue.move': M<QueueMoveParams, null>
  'queue.remove': M<QueueRemoveParams, null>
  'queue.clear': M<None, null>
  'queue.setShuffle': M<ShuffleParams, null>
  'queue.setRepeat': M<RepeatParams, null>
  'queue.get': M<None, QueueChanged>
  'devices.list': M<None, DeviceInfo[]>
  'library.folders': M<None, Folder[]>
  'library.addFolder': M<PathParams, Folder>
  'library.removeFolder': M<IdParams, null>
  'library.rescan': M<RescanParams, null>
  'library.albums': M<ListQuery, Page<AlbumRow>>
  'library.artists': M<ListQuery, Page<ArtistRow>>
  'library.genres': M<ListQuery, Page<GenreRow>>
  'library.tracks': M<ListQuery, Page<TrackRow>>
  'library.album': M<IdParams, AlbumDetail>
  'library.artist': M<IdParams, ArtistDetail>
  'library.genre': M<IdParams, GenreRow>
  'library.track': M<IdParams, TrackDetail>
  'library.search': M<SearchParams, SearchResults>
  'library.artwork': M<ArtworkParams, ArtworkInfo>
  'library.recent': M<RecentParams, RecentResult>
  'playlists.list': M<None, Playlist[]>
  'playlists.get': M<IdParams, PlaylistDetail>
  'playlists.create': M<NameParams, Playlist>
  'playlists.rename': M<RenameParams, Playlist>
  'playlists.delete': M<IdParams, null>
  'playlists.addTracks': M<PlaylistAddParams, Playlist>
  'playlists.removeItems': M<PlaylistRemoveParams, Playlist>
  'playlists.reorder': M<PlaylistReorderParams, Playlist>
  'playlists.importM3u': M<PathParams, ImportResult>
  'playlists.exportM3u': M<ExportParams, null>
  'dsp.get': M<None, DspState>
  'dsp.presets': M<None, EqPreset[]>
  'dsp.setEq': M<SetEqParams, DspState>
  'dsp.setEqEnabled': M<EnabledParams, DspState>
  'dsp.savePreset': M<NameParams, EqPreset>
  'dsp.deletePreset': M<NameParams, DspState>
  'dsp.setCrossfade': M<CrossfadeParams, DspState>
  'dsp.setReplayGain': M<ReplayGainParams, DspState>
  'lyrics.get': M<LyricsParams, Lyrics | null>
}

export type Events = {
  'player.state': PlayerState
  'player.signalPath': SignalPath
  'queue.changed': QueueChanged
  'devices.changed': DeviceInfo[]
  'engine.error': EngineErrorEvent
  'engine.status': EngineStatus
  'library.scanProgress': ScanProgress
  'library.changed': LibraryChanged
  'dsp.changed': DspState
}

export const ALLOWED_METHODS = [
  'player.play', 'player.pause', 'player.toggle', 'player.stop', 'player.next', 'player.prev', 'player.seek',
  'player.setVolume', 'player.setMode', 'player.setDevice', 'player.state',
  'queue.setPaths', 'queue.set', 'queue.add', 'queue.insertNext', 'queue.move', 'queue.remove', 'queue.clear',
  'queue.setShuffle', 'queue.setRepeat', 'queue.get', 'devices.list',
  'library.folders', 'library.addFolder', 'library.removeFolder', 'library.rescan', 'library.albums', 'library.artists',
  'library.genres', 'library.tracks', 'library.album', 'library.artist', 'library.genre', 'library.track', 'library.search',
  'library.artwork', 'library.recent',
  'playlists.list', 'playlists.get', 'playlists.create', 'playlists.rename', 'playlists.delete', 'playlists.addTracks',
  'playlists.removeItems', 'playlists.reorder', 'playlists.importM3u', 'playlists.exportM3u',
  'dsp.get', 'dsp.presets', 'dsp.setEq', 'dsp.setEqEnabled', 'dsp.savePreset', 'dsp.deletePreset', 'dsp.setCrossfade', 'dsp.setReplayGain',
  'lyrics.get',
] as const satisfies readonly (keyof Methods)[]

// Compile-time check that every typed method is allowed (and nothing else is).
type Unlisted = Exclude<keyof Methods, (typeof ALLOWED_METHODS)[number]>
const everyMethodAllowed: [Unlisted] extends [never] ? true : Unlisted = true
void everyMethodAllowed

export function isAllowedMethod(m: unknown): m is keyof Methods {
  return typeof m === 'string' && (ALLOWED_METHODS as readonly string[]).includes(m)
}

export const AUDIO_EXTENSIONS = ['mp3', 'm4a', 'm4b', 'mp4', 'aac', 'flac', 'wav', 'aif', 'aiff', 'aifc', 'ogg', 'oga', 'opus', 'dsf', 'dff'] as const

/** Binary frame kinds from the engine (spec §4.1). */
export const FRAME_SPECTRUM = 1

export interface AuroraApi {
  call<K extends keyof Methods>(method: K, params: Methods[K]['params']): Promise<Methods[K]['result']>
  on<E extends keyof Events>(event: E, cb: (payload: Events[E]) => void): () => void
  openFiles(): Promise<string[]>
  openFolder(): Promise<string | null>
  pickM3u(): Promise<string | null>
  pickM3uSave(defaultName: string): Promise<string | null>
  showItemInFolder(path: string): Promise<void>
  getSettings(): Promise<Settings>
  setSettings(patch: Partial<Settings>): Promise<Settings>
  pathsForFiles(files: File[]): string[]
  status(): Promise<EngineStatus>
  restartEngine(): Promise<void>
  openLogs(): Promise<void>
  appInfo(): Promise<AppInfo>
  openLicenses(): Promise<void>
  toggleMini(): Promise<void>
  showMain(): Promise<void>
  /** Turns this window's spectrum subscription on or off (main refcounts across windows). */
  setViz(on: boolean): void
  /** 64 spectrum bins (0-255) at up to 30 fps while subscribed. */
  onSpectrum(cb: (bins: Uint8Array) => void): () => void
  platform: string
}
