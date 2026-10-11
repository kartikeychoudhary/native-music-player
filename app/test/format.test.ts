import { describe, expect, it } from 'vitest'
import { bitPerfectLabel, bluetoothLabel, formatBadge, formatTime, khz } from '../src/renderer/lib/format'

const src = (o: object) => ({ container: 'flac', codec: 'flac', bitrateKbps: 2304, sampleRate: 96000, bitDepth: 24, channels: 2, dsdRate: null, ...o }) as any

describe('format', () => {
  it('times', () => {
    expect(formatTime(0)).toBe('0:00')
    expect(formatTime(61_999)).toBe('1:01')
    expect(formatTime(3_723_000)).toBe('1:02:03')
  })
  it('rates', () => { expect(khz(44100)).toBe('44.1'); expect(khz(96000)).toBe('96'); expect(khz(352800)).toBe('352.8') })
  it('badges', () => {
    expect(formatBadge(src({}))).toBe('FLAC 24/96')
    expect(formatBadge(src({ codec: 'mp3', container: 'mp3', bitDepth: null, sampleRate: 44100, bitrateKbps: 320 }))).toBe('MP3 320k')
    expect(formatBadge(src({ codec: 'he-aac', container: 'm4a', bitDepth: 16, sampleRate: 48000, bitrateKbps: 64 }))).toBe('HE-AAC 64k')
    expect(formatBadge(src({ codec: 'pcm', container: 'wav', bitDepth: 16, sampleRate: 44100 }))).toBe('WAV 16/44.1')
    expect(formatBadge(src({ codec: 'dsd', container: 'dsf', dsdRate: 2822400, sampleRate: 88200, bitDepth: null }))).toBe('DSD64')
  })
  it('bit-perfect labels follow the Bluetooth honesty rule', () => {
    expect(bitPerfectLabel({ bitPerfect: true, bitPerfectScope: 'device' } as any)).toBe('Bit-perfect')
    expect(bitPerfectLabel({ bitPerfect: true, bitPerfectScope: 'os' } as any)).toBe('Bit-perfect to OS')
    expect(bitPerfectLabel({ bitPerfect: false, bitPerfectScope: null } as any)).toBeNull()
  })
  it('bluetooth labels never guess', () => {
    expect(bluetoothLabel({ codec: 'ldac', quality: 'hq', bitrateKbps: 990 })).toBe('LDAC · 990 kbps')
    expect(bluetoothLabel({ codec: 'ldac', quality: 'auto', bitrateKbps: null })).toBe('LDAC · auto')
    expect(bluetoothLabel({ codec: 'aac', quality: null, bitrateKbps: null })).toBe('AAC')
    expect(bluetoothLabel({ codec: null, quality: null, bitrateKbps: null })).toBe('Codec not reported by the OS')
  })
})
