import { X } from 'lucide-react'
import { S } from '../lib/strings'
import { useUi } from '../stores/ui'
import s from './ui.module.css'

export function Toasts() {
  const toasts = useUi((u) => u.toasts)
  const dismiss = useUi((u) => u.dismissToast)
  return (
    <div className={s.toasts} aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={s.toast} data-kind={t.kind} role="status">
          <span style={{ flex: 1 }}>{t.text}</span>
          <button aria-label={S.dismiss} onClick={() => dismiss(t.id)} className="muted"><X size={14} /></button>
        </div>
      ))}
    </div>
  )
}
