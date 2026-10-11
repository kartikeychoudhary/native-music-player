import { useState } from 'react'
import { S } from '../lib/strings'
import { useUi } from '../stores/ui'
import { Dialog } from './Dialog'
import s from './ui.module.css'

/** Text prompt driven by `useUi().askName` (new playlist, rename). */
export function PromptDialog() {
  const prompt = useUi((u) => u.prompt)
  if (!prompt) return null
  return <PromptBody key={prompt.title + prompt.initial} />
}

function PromptBody() {
  const prompt = useUi((u) => u.prompt)!
  const [value, setValue] = useState(prompt.initial)
  const ok = value.trim().length > 0
  return (
    <Dialog title={prompt.title} onClose={() => prompt.resolve(null)}>
      <form onSubmit={(e) => { e.preventDefault(); if (ok) prompt.resolve(value.trim()) }}>
        <input className={s.field} value={value} maxLength={200} aria-label={prompt.title} onChange={(e) => setValue(e.target.value)} />
        <div className={s.actions}>
          <button type="button" className={s.secondary} onClick={() => prompt.resolve(null)}>{S.cancel}</button>
          <button type="submit" className={s.primary} disabled={!ok}>{prompt.confirm}</button>
        </div>
      </form>
    </Dialog>
  )
}
