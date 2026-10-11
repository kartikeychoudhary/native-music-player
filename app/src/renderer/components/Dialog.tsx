import { useEffect, useId, useRef } from 'react'
import s from './ui.module.css'

/** Modal dialog: Escape and backdrop click close it; focus moves in on open and back on close. */
export function Dialog({ title, onClose, labelledBy, children }: { title: string; onClose: () => void; labelledBy?: string; children: React.ReactNode }) {
  const id = useId()
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null
    const first = ref.current?.querySelector<HTMLElement>('input, textarea, select, button')
    first?.focus()
    return () => opener?.focus?.()
  }, [])
  return (
    <div className={s.backdrop} onMouseDown={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <div ref={ref} role="dialog" aria-modal="true" aria-labelledby={labelledBy ?? id} className={s.dialog}
        onKeyDown={(e) => { e.stopPropagation(); if (e.key === 'Escape') { e.preventDefault(); onClose() } }}>
        <h2 id={id}>{title}</h2>
        {children}
      </div>
    </div>
  )
}
