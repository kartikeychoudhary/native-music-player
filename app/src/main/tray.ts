import { Menu, Tray, type MenuItemConstructorOptions } from 'electron'
import { trayIcon } from './trayIcon'

export type TrayState = { playing: boolean; mainVisible: boolean; miniOpen: boolean; title: string | null; artist: string | null }
export type TrayActions = { toggle(): void; next(): void; prev(): void; toggleMain(): void; toggleMini(): void; quit(): void }

export function trayMenuTemplate(s: TrayState, a: TrayActions): MenuItemConstructorOptions[] {
  return [
    { label: s.playing ? 'Pause' : 'Play', click: () => a.toggle() },
    { label: 'Next', click: () => a.next() },
    { label: 'Previous', click: () => a.prev() },
    { type: 'separator' },
    { label: s.mainVisible ? 'Hide Aurora' : 'Show Aurora', click: () => a.toggleMain() },
    { label: s.miniOpen ? 'Close mini-player' : 'Mini-player', click: () => a.toggleMini() },
    { type: 'separator' },
    { label: 'Quit Aurora', click: () => a.quit() },
  ]
}

export function trayTooltip(s: TrayState): string {
  if (!s.title) return 'Aurora'
  return s.artist ? `${s.title} — ${s.artist}` : s.title
}

/** The tray icon (spec §5.2). Call `update` whenever player state or window visibility changes. */
export function createTray(actions: TrayActions) {
  const tray = new Tray(trayIcon())
  let state: TrayState = { playing: false, mainVisible: true, miniOpen: false, title: null, artist: null }
  let shown = ''
  const render = () => {
    const key = JSON.stringify(state)
    if (key === shown) return
    shown = key
    tray.setToolTip(trayTooltip(state))
    tray.setContextMenu(Menu.buildFromTemplate(trayMenuTemplate(state, actions)))
  }
  if (process.platform !== 'darwin') tray.on('click', () => actions.toggleMain())
  render()
  return {
    update(patch: Partial<TrayState>) { state = { ...state, ...patch }; render() },
    destroy() { tray.destroy() },
  }
}
