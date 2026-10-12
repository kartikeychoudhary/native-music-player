import type { SignalPath } from '@protocol/SignalPath'
import { bitPerfectLabel, bluetoothLabel, codecLabel, khz } from '../lib/format'
import s from './SignalPathPanel.module.css'

const STAGE_TITLE: Record<string, string> = { dsd: 'DSD', channels: 'Channels', resampler: 'Resampler', volume: 'Volume', dither: 'Dither', replaygain: 'ReplayGain', eq: 'EQ', crossfade: 'Crossfade' }

function Node({ title, active = true, children, testId }: { title: string; active?: boolean; children: React.ReactNode; testId?: string }) {
  return (
    <div className={s.node} data-active={String(active)} data-testid={testId}>
      <h5>{title}</h5>
      {children}
    </div>
  )
}

export function SignalPathPanel({ signal }: { signal: SignalPath | null }) {
  if (!signal) return <section className={s.panel}><p className="muted">Play something to see its signal path.</p></section>
  const { source: src, output: out } = signal
  const bp = bitPerfectLabel(signal)
  return (
    <section className={s.panel} aria-label="Signal path details">
      <header className={s.head}>
        <h2>Signal path</h2>
        <span className={`${s.badge} ${bp ? s.ok : ''}`}>{bp ?? 'Processed'}</span>
        {signal.underruns > 0 && <span className={`${s.badge} ${s.warn}`}>{signal.underruns} underruns</span>}
      </header>
      <div className={`${s.chain} mono`}>
        <Node title="SOURCE">
          <b>{`${src.container.toUpperCase()} · ${codecLabel(src)}`}</b>
          <span>{src.dsdRate ? `${(src.dsdRate / 1e6).toFixed(4)} MHz · 1-bit` : `${khz(src.sampleRate)} kHz${src.bitDepth ? ` · ${src.bitDepth}-bit` : ''}`}</span>
          <span>{src.channels} ch{src.bitrateKbps ? ` · ${src.bitrateKbps} kbps` : ''}</span>
        </Node>
        <span className={s.arrow} aria-hidden>→</span>
        <Node title="DECODER">
          <b>{signal.decoder.name}</b>
          <span>{signal.decoder.gapless ? `gapless −${signal.decoder.gapless.delay} / −${signal.decoder.gapless.padding}` : 'gapless: n/a'}</span>
        </Node>
        {signal.stages.map((st) => (
          <span key={st.kind} className={s.pair}>
            <span className={s.arrow} aria-hidden>→</span>
            <Node title={STAGE_TITLE[st.kind] ?? st.kind.toUpperCase()} active={st.active}><span>{st.detail}</span></Node>
          </span>
        ))}
        <span className={s.arrow} aria-hidden>→</span>
        <Node title="OUTPUT" testId="signal-output">
          <b>{out.name}</b>
          <span>{out.mode} · {khz(out.sampleRate)} kHz</span>
          <span>{out.format} · {out.transport}</span>
        </Node>
        {signal.bluetooth && (
          <>
            <span className={s.arrow} aria-hidden>→</span>
            <Node title="BLUETOOTH">
              <b className={signal.bluetooth.codec ? s.accent : undefined}>{bluetoothLabel(signal.bluetooth)}</b>
              {signal.bluetooth.codec && <span>negotiated by the OS</span>}
            </Node>
          </>
        )}
      </div>
      {signal.notes.length > 0 && <ul className={s.notes}>{signal.notes.map((n) => <li key={n}>{n}</li>)}</ul>}
    </section>
  )
}
