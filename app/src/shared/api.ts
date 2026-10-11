import type { DeviceInfo } from '@protocol/DeviceInfo'
import type { DeviceParams } from '@protocol/DeviceParams'
import type { EngineErrorEvent } from '@protocol/EngineErrorEvent'
import type { ModeParams } from '@protocol/ModeParams'
import type { PlayParams } from '@protocol/PlayParams'
import type { PlayerState } from '@protocol/PlayerState'
import type { QueueChanged } from '@protocol/QueueChanged'
import type { QueueSetPathsParams } from '@protocol/QueueSetPathsParams'
import type { SeekParams } from '@protocol/SeekParams'
import type { SignalPath } from '@protocol/SignalPath'
import type { VolumeParams } from '@protocol/VolumeParams'

export type EngineStatus = { state: 'starting' | 'ready' | 'restarting' | 'failed' | 'mismatch'; message?: string }
type None = Record<string, never>

export type Methods = {
  'player.play': { params: PlayParams; result: null }
  'player.pause': { params: None; result: null }
  'player.toggle': { params: None; result: null }
  'player.stop': { params: None; result: null }
  'player.next': { params: None; result: null }
  'player.prev': { params: None; result: null }
  'player.seek': { params: SeekParams; result: null }
  'player.setVolume': { params: VolumeParams; result: null }
  'player.setMode': { params: ModeParams; result: null }
  'player.setDevice': { params: DeviceParams; result: null }
  'player.state': { params: None; result: PlayerState }
  'queue.setPaths': { params: QueueSetPathsParams; result: null }
  'queue.get': { params: None; result: QueueChanged }
  'devices.list': { params: None; result: DeviceInfo[] }
}

export type Events = {
  'player.state': PlayerState
  'player.signalPath': SignalPath
  'queue.changed': QueueChanged
  'devices.changed': DeviceInfo[]
  'engine.error': EngineErrorEvent
  'engine.status': EngineStatus
}

export const ALLOWED_METHODS = [
  'player.play', 'player.pause', 'player.toggle', 'player.stop', 'player.next', 'player.prev', 'player.seek',
  'player.setVolume', 'player.setMode', 'player.setDevice', 'player.state', 'queue.setPaths', 'queue.get', 'devices.list',
] as const satisfies readonly (keyof Methods)[]

export function isAllowedMethod(m: unknown): m is keyof Methods {
  return typeof m === 'string' && (ALLOWED_METHODS as readonly string[]).includes(m)
}

export const AUDIO_EXTENSIONS = ['mp3', 'm4a', 'm4b', 'mp4', 'aac', 'alac', 'flac', 'wav', 'aif', 'aiff', 'aifc', 'ogg', 'oga', 'opus', 'dsf', 'dff'] as const

export interface AuroraApi {
  call<M extends keyof Methods>(method: M, params: Methods[M]['params']): Promise<Methods[M]['result']>
  on<E extends keyof Events>(event: E, cb: (payload: Events[E]) => void): () => void
  openFiles(): Promise<string[]>
  pathsForFiles(files: File[]): string[]
  status(): Promise<EngineStatus>
  restartEngine(): Promise<void>
  openLogs(): Promise<void>
  platform: string
}
