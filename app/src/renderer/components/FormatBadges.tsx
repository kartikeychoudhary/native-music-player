import type { SignalPath } from '@protocol/SignalPath'
import { bitPerfectLabel, bluetoothLabel, formatBadge } from '../lib/format'
import s from './FormatBadges.module.css'

export function FormatBadges({ signal }: { signal: SignalPath | null }) {
  if (!signal) return null
  const bp = bitPerfectLabel(signal)
  return (
    <div className={s.row}>
      {bp && <span className={`${s.badge} ${s.ok}`} data-testid="bitperfect-badge">{bp}</span>}
      <span className={s.badge} data-testid="format-badge">{formatBadge(signal.source)}</span>
      {signal.bluetooth?.codec && <span className={s.badge}>{bluetoothLabel(signal.bluetooth)}</span>}
    </div>
  )
}
