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
import { createAppRouter } from './router'
import { useLibrary } from './stores/library'
import { usePlayer } from './stores/player'
import { useUi } from './stores/ui'

installDropGuard(document)
const api = window.aurora
const router = createAppRouter()
usePlayer.getState().connect(api)
useLibrary.getState().connect(api)
void api.getSettings().then((s) => {
  useUi.getState().applySettings(s)
  const hash = window.location.hash
  if ((!hash || hash === '#/' || hash === '#') && s.lastView !== '/') router.history.replace(s.lastView)
}).catch(() => {})

createRoot(document.getElementById('root')!).render(<StrictMode><RouterProvider router={router} /></StrictMode>)
