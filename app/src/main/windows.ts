import { BrowserWindow, screen } from 'electron'
import { pathToFileURL } from 'node:url'
import type { Point } from '@shared/settings'
import { createEventRelay } from './eventRelay'
import { shouldBlockNavigation } from './navigation'
import { safeSend } from './safeSend'
import { boundsVisible, type SettingsStore } from './settings'

type Rect = { x: number; y: number; width: number; height: number }
export const MINI_SIZE = { width: 340, height: 110 }
const MARGIN = 24

/** Where the mini-player opens: its saved spot when the whole window fits on a display, else bottom-right of the first one. */
export function miniPosition(saved: Point | null, workAreas: Rect[], size = MINI_SIZE): Point {
  const fits = (a: Rect) => saved && saved.x >= a.x && saved.y >= a.y && saved.x + size.width <= a.x + a.width && saved.y + size.height <= a.y + a.height
  if (saved && workAreas.some(fits)) return saved
  const a = workAreas[0] ?? { x: 0, y: 0, width: 1280, height: 800 }
  return { x: a.x + a.width - size.width - MARGIN, y: a.y + a.height - size.height - MARGIN }
}

type Managed = { win: BrowserWindow; relay: ReturnType<typeof createEventRelay> }

export class WindowManager {
  private mainW: Managed | null = null
  private miniW: Managed | null = null

  /**
   * `onMainClosed` runs when the main window closes (index.ts quits there on Windows/Linux, spec §5.2).
   * `onVisibilityChange` runs when the main window shows, hides, minimizes, restores or closes, and when the mini-player opens or closes (the tray's labels follow it).
   */
  constructor(private o: { settings: SettingsStore; preload: string; page: string; devUrl?: string; onMainClosed?: () => void; onVisibilityChange?: () => void }) {}

  private changed() { this.o.onVisibilityChange?.() }

  get main(): BrowserWindow | null { return this.mainW?.win ?? null }
  get miniOpen(): boolean { return !!this.miniW }

  /** Delivers an engine event to every open window. */
  send(event: string, payload: unknown) {
    for (const m of [this.mainW, this.miniW]) m?.relay.send(event, payload)
  }

  private webPreferences() {
    return { preload: this.o.preload, contextIsolation: true, nodeIntegration: false, sandbox: true }
  }

  private load(win: BrowserWindow, hash: string) {
    const appUrl = this.o.devUrl ?? pathToFileURL(this.o.page).href
    // A window only ever shows the app: no navigating away (e.g. to a dropped file), no new windows.
    win.webContents.on('will-navigate', (e) => { if (shouldBlockNavigation(e.url, appUrl)) e.preventDefault() })
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    if (this.o.devUrl) void win.loadURL(`${this.o.devUrl}${hash ? `#${hash}` : ''}`)
    else void win.loadFile(this.o.page, hash ? { hash } : undefined)
  }

  /** Holds events (e.g. library notices sent once, right after the engine starts) until the page has loaded. */
  private manage(win: BrowserWindow, onClosed: () => void): Managed {
    const relay = createEventRelay((event, payload) => safeSend(win, 'aurora:event', event, payload))
    win.on('closed', () => { relay.reset(); onClosed() })
    win.webContents.on('did-start-loading', relay.reset)
    win.webContents.on('did-finish-load', relay.ready)
    return { win, relay }
  }

  createMain() {
    const mac = process.platform === 'darwin'
    const saved = this.o.settings.get().windowBounds
    const visible = saved && boundsVisible(saved, screen.getAllDisplays().map((d) => d.workArea))
    const win = new BrowserWindow({
      width: 1280, height: 800, ...(visible ? { x: saved.x, y: saved.y, width: saved.width, height: saved.height } : {}),
      minWidth: 940, minHeight: 600, show: false, backgroundColor: '#17151A',
      titleBarStyle: mac ? 'hiddenInset' : 'hidden',
      titleBarOverlay: mac ? undefined : { color: '#100E12', symbolColor: '#EFE9E1', height: 36 },
      webPreferences: this.webPreferences(),
    })
    if (visible && saved.maximized) win.maximize()
    win.once('ready-to-show', () => win.show())
    win.on('close', () => this.saveMainBounds())
    win.on('show', () => this.changed())
    win.on('hide', () => this.changed())
    win.on('minimize', () => this.changed())
    win.on('restore', () => this.changed())
    this.mainW = this.manage(win, () => { this.mainW = null; this.changed(); this.o.onMainClosed?.() })
    this.load(win, '')
  }

  saveMainBounds() {
    const win = this.main
    if (!win || win.isDestroyed()) return
    this.o.settings.update({ windowBounds: { ...win.getNormalBounds(), maximized: win.isMaximized() } })
  }

  toggleMini() {
    if (this.miniW) { this.miniW.win.close(); return }
    const pos = miniPosition(this.o.settings.get().miniBounds, screen.getAllDisplays().map((d) => d.workArea))
    const win = new BrowserWindow({
      ...MINI_SIZE, ...pos, frame: false, resizable: false, maximizable: false, minimizable: false, fullscreenable: false,
      alwaysOnTop: true, show: false, backgroundColor: '#100E12', title: 'Aurora mini-player', webPreferences: this.webPreferences(),
    })
    win.once('ready-to-show', () => win.show())
    win.on('moved', () => { const [x, y] = win.getPosition(); this.o.settings.update({ miniBounds: { x, y } }) })
    this.miniW = this.manage(win, () => { this.miniW = null; this.changed() })
    this.changed()
    this.load(win, '/mini')
  }

  showMain() {
    if (!this.mainW) { this.createMain(); return }
    const win = this.mainW.win
    if (win.isMinimized()) win.restore()
    win.show()
    win.focus()
  }

  toggleMainVisible() {
    const win = this.main
    if (win && win.isVisible() && !win.isMinimized()) win.hide()
    else this.showMain()
  }
}
