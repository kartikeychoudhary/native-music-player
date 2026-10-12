// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { SignalPathPanel } from '../src/renderer/components/SignalPathPanel'

const base = {
  source: { container: 'm4a', codec: 'aac-lc', bitrateKbps: 320, sampleRate: 44100, bitDepth: null, channels: 2, dsdRate: null },
  decoder: { name: 'symphonia', gapless: { delay: 2112, padding: 1024 } },
  stages: [
    { kind: 'channels', active: false, detail: 'unchanged' },
    { kind: 'resampler', active: false, detail: 'rate matched' },
    { kind: 'volume', active: true, detail: '82%' },
  ],
  output: { deviceId: 'd', name: 'WH-1000XM5', transport: 'bluetooth', mode: 'shared', sampleRate: 44100, format: 'f32' },
  bluetooth: { codec: null, quality: null, bitrateKbps: null },
  bitPerfect: false, bitPerfectScope: null, underruns: 0, notes: ['Exclusive mode unavailable — using processed mode'],
} as any

afterEach(cleanup)

describe('SignalPathPanel', () => {
  it('draws the chain with inactive stages marked and honest Bluetooth text', () => {
    render(<SignalPathPanel signal={base} />)
    expect(screen.getByText('M4A · AAC')).toBeTruthy()
    expect(screen.getByText('rate matched').closest('[data-active]')?.getAttribute('data-active')).toBe('false')
    expect(screen.getByText('Codec not reported by the OS')).toBeTruthy()
    expect(screen.getByTestId('signal-output').textContent).toContain('WH-1000XM5')
    expect(screen.getByText('Exclusive mode unavailable — using processed mode')).toBeTruthy()
  })

  it('shows LDAC and the OS-scoped badge', () => {
    render(<SignalPathPanel signal={{ ...base, bitPerfect: true, bitPerfectScope: 'os', bluetooth: { codec: 'ldac', quality: 'hq', bitrateKbps: 990 } }} />)
    expect(screen.getByText('LDAC · 990 kbps')).toBeTruthy()
    expect(screen.getByText('Bit-perfect to OS')).toBeTruthy()
  })

  it('titles the DSP stages by name', () => {
    const stages = [
      { kind: 'replaygain', active: true, detail: '-6.2 dB (track)' },
      { kind: 'eq', active: true, detail: 'Flat' },
      { kind: 'crossfade', active: true, detail: '4 s' },
    ]
    render(<SignalPathPanel signal={{ ...base, stages }} />)
    for (const title of ['ReplayGain', 'EQ', 'Crossfade']) expect(screen.getByRole('heading', { level: 5, name: title })).toBeTruthy()
    expect(screen.queryByRole('heading', { level: 5, name: 'REPLAYGAIN' })).toBeNull()
    expect(screen.queryByRole('heading', { level: 5, name: 'CROSSFADE' })).toBeNull()
  })

  it('renders an empty state with no signal', () => {
    render(<SignalPathPanel signal={null} />)
    expect(screen.getByText(/play something/i)).toBeTruthy()
  })
})
