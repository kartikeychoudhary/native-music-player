import { Outlet, useRouterState } from '@tanstack/react-router'
import { useEffect } from 'react'
import { ContextMenu } from './components/ContextMenu'
import { EngineBanner } from './components/EngineBanner'
import { PlayerBar } from './components/PlayerBar'
import { PromptDialog } from './components/PromptDialog'
import { Sidebar, SidebarPlaylists } from './components/Sidebar'
import { Toasts } from './components/Toasts'
import { TrackInfoDialog } from './components/TrackInfoDialog'
import { handleShortcut } from './lib/shortcuts'
import { S } from './lib/strings'
import { usePlayer } from './stores/player'
import { useUi } from './stores/ui'
import s from './Layout.module.css'

export function Layout() {
  const api = usePlayer((p) => p.api)
  const expanded = useUi((u) => u.sidebarExpanded)
  const pathname = useRouterState({ select: (r) => r.location.pathname })
  const error = usePlayer((p) => p.error)

  useEffect(() => { void api?.setSettings({ lastView: pathname }).catch(() => {}) }, [api, pathname])
  useEffect(() => {
    if (!error) return
    useUi.getState().toast('error', `${error.message}${error.path ? ` · ${error.path.split(/[\\/]/).pop()}` : ''}`)
    usePlayer.getState().dismissError()
  }, [error])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { handleShortcut(e, usePlayer.getState(), performance.now()) }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <div className={s.app} data-platform={api?.platform} data-expanded={expanded}>
      <div className={`${s.titlebar} drag`}><span className="mono">{S.app}</span></div>
      <Sidebar><SidebarPlaylists /></Sidebar>
      <main className={s.main}>
        <EngineBanner />
        <Outlet />
      </main>
      <PlayerBar />
      <ContextMenu />
      <Toasts />
      <PromptDialog />
      <TrackInfoDialog />
    </div>
  )
}
