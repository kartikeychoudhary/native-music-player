import { useEffect } from 'react'
import type { AuroraApi } from '@shared/api'
import { EngineBanner } from './components/EngineBanner'
import { Home } from './components/Home'
import { IconRail } from './components/IconRail'
import { PlayerBar } from './components/PlayerBar'
import { SignalPathPanel } from './components/SignalPathPanel'
import { usePlayer } from './stores/player'
import s from './App.module.css'

export function App({ api }: { api: AuroraApi }) {
  const panel = usePlayer((p) => p.panel)
  const signal = usePlayer((p) => p.signal)
  useEffect(() => usePlayer.getState().connect(api), [api])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === 'INPUT' || (e.target as HTMLElement)?.tagName === 'SELECT') return
      const p = usePlayer.getState()
      if (e.code === 'Space') { e.preventDefault(); void p.toggle() }
      else if (e.key === 'ArrowRight' && e.shiftKey) void p.next()
      else if (e.key === 'ArrowLeft' && e.shiftKey) void p.prev()
      else if (e.key === 'ArrowRight') void p.seek(p.state.positionMs + 5000)
      else if (e.key === 'ArrowLeft') void p.seek(Math.max(0, p.state.positionMs - 5000))
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  return (
    <div className={s.app} data-platform={api.platform}>
      <div className={`${s.titlebar} drag`}><span className="mono">aurora</span></div>
      <IconRail />
      <main className={s.main}>
        <EngineBanner />
        {panel === 'signal' ? <SignalPathPanel signal={signal} /> : <Home />}
      </main>
      <PlayerBar />
    </div>
  )
}
