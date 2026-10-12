import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import '@fontsource/inter/700.css'
import '@fontsource/inter/800.css'
import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/700.css'
import './styles/tokens.css'
import './styles/global.css'
import { RouterProvider } from '@tanstack/react-router'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { installDropGuard } from './lib/dropGuard'
import { MiniApp } from './mini/MiniApp'
import { createAppRouter } from './router'
import { useDsp } from './stores/dsp'
import { useLibrary } from './stores/library'
import { usePlayer } from './stores/player'
import { useUi } from './stores/ui'
import { useUpdates } from './stores/updates'

installDropGuard(document)
const api = window.aurora
usePlayer.getState().connect(api)
if (window.location.hash.startsWith('#/mini')) {
  // The mini-player needs only the player store; the library store stays with the main window.
  createRoot(document.getElementById('root')!).render(<StrictMode><MiniApp api={api} /></StrictMode>)
} else {
  const router = createAppRouter()
  useLibrary.getState().connect(api)
  useDsp.getState().connect(api)
  useUpdates.getState().init(api)
  void api.getSettings().then((s) => {
    useUi.getState().applySettings(s)
    const hash = window.location.hash
    if ((!hash || hash === '#/' || hash === '#') && s.lastView !== '/') router.history.replace(s.lastView)
  }).catch(() => {})

  createRoot(document.getElementById('root')!).render(<StrictMode><RouterProvider router={router} /></StrictMode>)
}
