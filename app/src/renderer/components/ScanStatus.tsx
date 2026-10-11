import { RefreshCw } from 'lucide-react'
import { S } from '../lib/strings'
import { useLibrary } from '../stores/library'
import s from './ScanStatus.module.css'

/** The running library scan: a progress line with a bar. `compact` shows only the spinning icon. */
export function ScanStatus({ folderId, compact = false, className }: { folderId?: number; compact?: boolean; className?: string }) {
  const scan = useLibrary((l) => l.scan)
  if (!scan || (folderId != null && scan.folderId !== folderId)) return null
  const label = S.scanning(scan.processed, scan.total)
  return (
    <div role="status" className={`${s.status} ${className ?? ''}`} title={label}>
      <span className={s.line}><RefreshCw size={13} className={s.spin} aria-hidden />{compact ? <span className="sr-only">{label}</span> : label}</span>
      {!compact && (
        // Indeterminate while the folder is still being walked (the total isn't known yet).
        <progress className={s.bar} aria-label={label} max={scan.total || undefined} value={scan.total ? scan.processed : undefined} />
      )}
    </div>
  )
}
