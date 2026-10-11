import { app, BrowserWindow, dialog, ipcMain, net, protocol, screen, shell } from 'electron'
import { spawn } from 'node:child_process'
import { delimiter, isAbsolute, join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { PROTOCOL_VERSION } from '@protocol/ProtocolVersion'
import { AUDIO_EXTENSIONS, isAllowedMethod } from '@shared/api'
import { ART_SCHEME, createArtHandler } from './artProtocol'
import { engineArgs, resolveDataDir, usesSingleInstanceLock } from './engineArgs'
import { enginePath } from './engine/enginePath'
import { EngineSupervisor } from './engine/supervisor'
import { createEventRelay } from './eventRelay'
import { logDir, logger } from './logs'
import { shouldBlockNavigation } from './navigation'
import { safeSend } from './safeSend'
import { boundsVisible, SettingsStore } from './settings'

protocol.registerSchemesAsPrivileged([{ scheme: ART_SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true } }])

// Tests point everything (engine data, settings, logs) at a throwaway folder. Unpackaged builds only.
if (!app.isPackaged && process.env.AURORA_DATA_DIR) app.setPath('userData', process.env.AURORA_DATA_DIR)

// One Aurora at a time: a second launch focuses the running window instead of starting a second
// engine on the same library. Unpackaged runs with their own AURORA_DATA_DIR skip the lock, so
// parallel e2e runs (each with a distinct data dir) don't block each other.
const primary = !usesSingleInstanceLock({ isPackaged: app.isPackaged, env: process.env }) || app.requestSingleInstanceLock()
if (!primary) app.quit()

let win: BrowserWindow | null = null
let settings: SettingsStore
let quitting = false

/** Test-only hooks; ignored in packaged builds. */
const testEnv = (name: string): string | undefined => (app.isPackaged ? undefined : process.env[name])

const binary = () => enginePath({ isPackaged: app.isPackaged, resourcesPath: process.resourcesPath, appPath: app.getAppPath(), platform: process.platform, arch: process.arch, env: process.env })
const dataDir = () => resolveDataDir({ isPackaged: app.isPackaged, env: process.env, userData: app.getPath('userData') })

const supervisor = new EngineSupervisor(() => spawn(binary(), engineArgs(dataDir()), { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }), { protocolVersion: PROTOCOL_VERSION })

// Holds library notices (sent once, right after the engine starts) until the page has loaded.
const relay = createEventRelay((event, payload) => safeSend(win, 'aurora:event', event, payload))
const send = relay.send

function saveBounds() {
  if (!win || win.isDestroyed()) return
  const b = win.getNormalBounds()
  settings.update({ windowBounds: { ...b, maximized: win.isMaximized() } })
}

function createWindow() {
  const mac = process.platform === 'darwin'
  const saved = settings.get().windowBounds
  const visible = saved && boundsVisible(saved, screen.getAllDisplays().map((d) => d.workArea))
  win = new BrowserWindow({
    width: 1280, height: 800, ...(visible ? { x: saved.x, y: saved.y, width: saved.width, height: saved.height } : {}),
    minWidth: 940, minHeight: 600, show: false,
    backgroundColor: '#17151A',
    titleBarStyle: mac ? 'hiddenInset' : 'hidden',
    titleBarOverlay: mac ? undefined : { color: '#100E12', symbolColor: '#EFE9E1', height: 36 },
    webPreferences: { preload: join(__dirname, '../preload/index.js'), contextIsolation: true, nodeIntegration: false, sandbox: true },
  })
  if (visible && saved.maximized) win.maximize()
  win.once('ready-to-show', () => win?.show())
  win.on('close', saveBounds)
  win.on('closed', () => { win = null; relay.reset() })
  win.webContents.on('did-start-loading', relay.reset)
  win.webContents.on('did-finish-load', relay.ready)
  const devUrl = process.env.ELECTRON_RENDERER_URL
  const page = join(__dirname, '../renderer/index.html')
  const appUrl = devUrl ?? pathToFileURL(page).href
  // The window only ever shows the app: no navigating away (e.g. to a dropped file), no new windows.
  win.webContents.on('will-navigate', (e) => { if (shouldBlockNavigation(e.url, appUrl)) e.preventDefault() })
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  if (devUrl) win.loadURL(devUrl)
  else win.loadFile(page)
}

app.on('second-instance', () => {
  if (!win) return
  if (win.isMinimized()) win.restore()
  win.show()
  win.focus()
})

app.whenReady().then(() => {
  if (!primary) return
  settings = new SettingsStore(join(app.getPath('userData'), 'settings.json'))
  const engineLog = logger('engine.log')
  supervisor.on('log', engineLog)
  supervisor.on('notification', (m: string, p: unknown) => send(m, p))
  supervisor.on('status', (s) => { engineLog(`[status] ${JSON.stringify(s)}`); send('engine.status', s) })

  protocol.handle(ART_SCHEME, createArtHandler(
    (method, params) => supervisor.call(method, params) as never,
    (path) => net.fetch(pathToFileURL(path).toString()),
  ))

  ipcMain.handle('aurora:call', (_e, method: unknown, params: unknown) => {
    if (!isAllowedMethod(method)) throw new Error(`method not allowed: ${String(method)}`)
    return supervisor.call(method, params ?? {})
  })
  ipcMain.handle('aurora:openFiles', async () => {
    const test = testEnv('AURORA_TEST_OPEN_FILES')
    if (test) return test.split(delimiter).filter(Boolean)
    if (!win) return []
    const r = await dialog.showOpenDialog(win, { properties: ['openFile', 'multiSelections'], filters: [{ name: 'Audio', extensions: [...AUDIO_EXTENSIONS] }] })
    return r.canceled ? [] : r.filePaths
  })
  ipcMain.handle('aurora:openFolder', async () => {
    const test = testEnv('AURORA_TEST_OPEN_FOLDER')
    if (test) return test
    if (!win) return null
    const r = await dialog.showOpenDialog(win, { properties: ['openDirectory'] })
    return r.canceled ? null : r.filePaths[0] ?? null
  })
  ipcMain.handle('aurora:pickM3u', async () => {
    const test = testEnv('AURORA_TEST_OPEN_M3U')
    if (test) return test
    if (!win) return null
    const r = await dialog.showOpenDialog(win, { properties: ['openFile'], filters: [{ name: 'Playlists', extensions: ['m3u8', 'm3u'] }] })
    return r.canceled ? null : r.filePaths[0] ?? null
  })
  ipcMain.handle('aurora:pickM3uSave', async (_e, defaultName: unknown) => {
    const test = testEnv('AURORA_TEST_SAVE_M3U')
    if (test) return test
    if (!win) return null
    const name = typeof defaultName === 'string' ? defaultName.replace(/[\\/:*?"<>|]/g, '_') : 'Playlist'
    const r = await dialog.showSaveDialog(win, { defaultPath: `${name}.m3u8`, filters: [{ name: 'Playlists', extensions: ['m3u8'] }] })
    return r.canceled ? null : r.filePath ?? null
  })
  ipcMain.handle('aurora:showItemInFolder', (_e, path: unknown) => {
    if (typeof path === 'string' && isAbsolute(path)) shell.showItemInFolder(path)
  })
  ipcMain.handle('aurora:getSettings', () => settings.get())
  ipcMain.handle('aurora:setSettings', (_e, patch: unknown) => settings.update(patch))
  ipcMain.handle('aurora:status', () => supervisor.status)
  ipcMain.handle('aurora:restartEngine', () => supervisor.restart())
  ipcMain.handle('aurora:openLogs', async () => { await shell.openPath(logDir()) })

  supervisor.start()
  createWindow()
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow() })
})

// Quit only after the engine has shut down cleanly (it writes session.json on the way out).
app.on('before-quit', (e) => {
  if (quitting || !primary) return
  e.preventDefault()
  quitting = true
  saveBounds()
  settings?.flush()
  void supervisor.stop().finally(() => app.quit())
})
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
