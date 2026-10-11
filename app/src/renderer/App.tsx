import { useEffect } from 'react'
import type { AuroraApi } from '@shared/api'
import { EngineBanner } from './components/EngineBanner'
import { Home } from './components/Home'
import { IconRail } from './components/IconRail'
import { PlayerBar } from './components/PlayerBar'
import { SignalPathPanel } from './components/SignalPathPanel'
import { handleShortcut } from './lib/shortcuts'
import { usePlayer } from './stores/player'
import s from './App.module.css'

export function App({ api }: { api: AuroraApi }) {
  const panel = usePlayer((p) => p.panel)
  const signal = usePlayer((p) => p.signal)
  useEffect(() => usePlayer.getState().connect(api), [api])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { handleShortcut(e, usePlayer.getState(), performance.now()) }
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
