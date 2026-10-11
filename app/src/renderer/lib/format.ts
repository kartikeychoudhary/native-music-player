import type { BluetoothInfo } from '@protocol/BluetoothInfo'
import type { SignalPath } from '@protocol/SignalPath'
import type { SourceSummary } from '@protocol/SourceSummary'

export function formatTime(ms: number): string {
  const t = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60
  const ss = String(s).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`
}

export function khz(rate: number): string {
  const k = rate / 1000
  return Number.isInteger(k) ? String(k) : k.toFixed(1)
}

const LABELS: Record<string, string> = { mp3: 'MP3', 'aac-lc': 'AAC', 'he-aac': 'HE-AAC', 'he-aac-v2': 'HE-AACv2', alac: 'ALAC', flac: 'FLAC', vorbis: 'Vorbis', opus: 'Opus', dsd: 'DSD' }
const LOSSLESS = new Set(['flac', 'alac', 'pcm'])

export function codecLabel(src: SourceSummary): string {
  return src.codec === 'pcm' ? src.container.toUpperCase() : (LABELS[src.codec] ?? src.codec.toUpperCase())
}

export function formatBadge(src: SourceSummary): string {
  if (src.codec === 'dsd' && src.dsdRate) return `DSD${Math.round(src.dsdRate / 44100)}`
  if (LOSSLESS.has(src.codec)) return `${codecLabel(src)} ${src.bitDepth ?? 16}/${khz(src.sampleRate)}`
  return src.bitrateKbps ? `${codecLabel(src)} ${src.bitrateKbps}k` : `${codecLabel(src)} ${khz(src.sampleRate)}`
}

export function bitPerfectLabel(sp: Pick<SignalPath, 'bitPerfect' | 'bitPerfectScope'>): string | null {
  if (!sp.bitPerfect) return null
  return sp.bitPerfectScope === 'os' ? 'Bit-perfect to OS' : 'Bit-perfect'
}

export function bluetoothLabel(bt: BluetoothInfo): string {
  if (!bt.codec) return 'Codec not reported by the OS'
  const name = bt.codec.toUpperCase()
  if (bt.bitrateKbps) return `${name} · ${bt.bitrateKbps} kbps`
  if (bt.quality) return `${name} · ${bt.quality}`
  return name
}
