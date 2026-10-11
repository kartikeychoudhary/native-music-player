import { S } from '../lib/strings'
import { Dialog } from './Dialog'
import s from './ui.module.css'

export function ConfirmDialog({ title, body, confirm, onConfirm, onCancel }: { title: string; body: string; confirm: string; onConfirm: () => void; onCancel: () => void }) {
  return (
    <Dialog title={title} onClose={onCancel}>
      <p>{body}</p>
      <div className={s.actions}>
        <button className={s.secondary} onClick={onCancel}>{S.cancel}</button>
        <button className={s.primary} onClick={onConfirm}>{confirm}</button>
      </div>
    </Dialog>
  )
}
