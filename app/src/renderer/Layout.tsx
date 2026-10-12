import { Outlet, useRouter, useRouterState } from '@tanstack/react-router'
import { useCallback, useEffect } from 'react'
import { ContextMenu } from './components/ContextMenu'
import { EngineBanner } from './components/EngineBanner'
import { PlayerBar } from './components/PlayerBar'
import { PromptDialog } from './components/PromptDialog'
import { Sidebar, SidebarPlaylists } from './components/Sidebar'
import { Toasts } from './components/Toasts'
import { TrackInfoDialog } from './components/TrackInfoDialog'
import { applyAccent, tintFor } from './lib/accent'
import { useEngineQuery } from './lib/useEngineQuery'
import { useGo } from './lib/useGo'
import { handleShortcut } from './lib/shortcuts'
import { S } from './lib/strings'
import { usePlayer } from './stores/player'
import { useUi } from './stores/ui'
import s from './Layout.module.css'

/** Follows the playing track's palette when the accent-tint setting is on (spec §5.5). */
function useAccentTint() {
  const enabled = useUi((u) => u.accentTint)
  const artId = usePlayer((p) => (p.state.status !== 'stopped' ? p.state.track?.artworkId ?? null : null))
  const art = useEngineQuery(['accent', enabled ? artId : null], async (a) => (enabled && artId != null ? a.call('library.artwork', { id: artId, size: 64 }) : null), [])
  useEffect(() => { applyAccent(tintFor(art.data?.palette?.vibrant, enabled)) }, [art.data, enabled])
}

export function Layout() {
  const api = usePlayer((p) => p.api)
  const expanded = useUi((u) => u.sidebarExpanded)
  const pathname = useRouterState({ select: (r) => r.location.pathname })
  const error = usePlayer((p) => p.error)
  const go = useGo()
  const router = useRouter()
  const toggleGo = useCallback((path: string) => {
    // Ctrl/Cmd+L on Now Playing returns to where the user was.
    if (path === '/now-playing' && router.state.location.pathname === '/now-playing') router.history.back()
    else go(path)
  }, [go, router])
  useAccentTint()

  useEffect(() => { void api?.setSettings({ lastView: pathname }).catch(() => {}) }, [api, pathname])
  useEffect(() => {
    if (!error) return
    useUi.getState().toast('error', `${error.message}${error.path ? ` · ${error.path.split(/[\\/]/).pop()}` : ''}`)
    usePlayer.getState().dismissError()
  }, [error])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { handleShortcut(e, usePlayer.getState(), performance.now(), toggleGo) }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [toggleGo])

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
