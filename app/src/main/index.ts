import { app, BrowserWindow, dialog, ipcMain, net, protocol, shell, webContents } from 'electron'
import { spawn } from 'node:child_process'
import { existsSync, statSync } from 'node:fs'
import { delimiter, isAbsolute, join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { PROTOCOL_VERSION } from '@protocol/ProtocolVersion'
import { AUDIO_EXTENSIONS, isAllowedMethod } from '@shared/api'
import { APP_ID } from './appIdentity'
import { ART_SCHEME, createArtHandler } from './artProtocol'
import { readBuildInfo } from './buildInfo'
import { engineArgs, resolveDataDir, usesSingleInstanceLock } from './engineArgs'
import { enginePath } from './engine/enginePath'
import { EngineSupervisor } from './engine/supervisor'
import { installKind } from './installKind'
import { logDir, logger } from './logs'
import { audioFilesFromArgv, createFileOpener } from './openFiles'
import { safeSend } from './safeSend'
import { SettingsStore } from './settings'
import { createTray } from './tray'
import { createUpdateController, latestPublishedVersion, releaseUrl, updateMode } from './updates'
import { createVizHub, hookSender } from './viz'
import { WindowManager } from './windows'

// One Windows identity for the taskbar, notifications and (via the engine) the media flyout.
if (process.platform === 'win32') app.setAppUserModelId(APP_ID)

protocol.registerSchemesAsPrivileged([{ scheme: ART_SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true } }])

// Tests point everything (engine data, settings, logs) at a throwaway folder. Unpackaged builds only.
if (!app.isPackaged && process.env.AURORA_DATA_DIR) app.setPath('userData', process.env.AURORA_DATA_DIR)

// One Aurora at a time: a second launch focuses the running window instead of starting a second
// engine on the same library. Unpackaged runs with their own AURORA_DATA_DIR skip the lock, so
// parallel e2e runs (each with a distinct data dir) don't block each other.
const primary = !usesSingleInstanceLock({ isPackaged: app.isPackaged, env: process.env }) || app.requestSingleInstanceLock()
if (!primary) app.quit()

let windows: WindowManager
let settings: SettingsStore
let quitting = false

/** Test-only hooks; ignored in packaged builds. */
const testEnv = (name: string): string | undefined => (app.isPackaged ? undefined : process.env[name])

const binary = () => enginePath({ isPackaged: app.isPackaged, resourcesPath: process.resourcesPath, appPath: app.getAppPath(), platform: process.platform, arch: process.arch, env: process.env })
const dataDir = () => resolveDataDir({ isPackaged: app.isPackaged, env: process.env, userData: app.getPath('userData') })

// Only an NSIS install has a Start-menu shortcut carrying APP_ID for the engine's id to resolve against.
const kind = installKind({ isPackaged: app.isPackaged, platform: process.platform, execPath: process.execPath, env: process.env })

const supervisor = new EngineSupervisor(() => spawn(binary(), engineArgs(dataDir(), { appUserModelId: kind === 'nsis' ? APP_ID : undefined }), { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }), { protocolVersion: PROTOCOL_VERSION })

// Spectrum subscriptions are refcounted across windows; the engine only has one on/off switch.
const viz = createVizHub({
  call: (method) => supervisor.call(method, {}),
  send: (id, data) => {
    const wc = webContents.fromId(id)
    return wc ? safeSend({ isDestroyed: () => wc.isDestroyed(), webContents: wc }, 'aurora:spectrum', data) : false
  },
})
const hookedSenders = new Set<number>()

// Audio files the OS hands us: on the command line at start-up (double-click, Open with), through a second
// launch's argv, or (macOS) an open-file event. Everything funnels through openFromOs; paths that arrive
// before whenReady has built the opener wait in pendingOpen.
const isFile = (p: string) => statSync(p, { throwIfNoEntry: false })?.isFile() === true
const fromArgv = (argv: readonly string[], cwd: string) => audioFilesFromArgv(argv, { isPackaged: app.isPackaged, cwd, exists: isFile })
/** The audio files this process was launched with (the update resumer needs to know whether there were any). */
const startupFiles = fromArgv(process.argv, process.cwd())
let opener: ReturnType<typeof createFileOpener> | undefined
let pendingOpen: string[] = []
const openFromOs = (paths: string[]) => { if (opener) opener.open(paths); else pendingOpen.push(...paths) }

app.on('second-instance', (_e, argv, workingDirectory) => {
  if (windows) windows.showMain()
  openFromOs(fromArgv(argv, workingDirectory))
})
// macOS delivers "Open with" and Dock drops here, never through argv. Must be registered before ready.
app.on('open-file', (e, path) => {
  e.preventDefault()
  openFromOs(audioFilesFromArgv([process.execPath, path], { isPackaged: true, cwd: process.cwd(), exists: isFile }))
})

app.whenReady().then(() => {
  if (!primary) return
  settings = new SettingsStore(join(app.getPath('userData'), 'settings.json'))
  windows = new WindowManager({
    settings, preload: join(__dirname, '../preload/index.js'), page: join(__dirname, '../renderer/index.html'), devUrl: process.env.ELECTRON_RENDERER_URL,
    // Closing the main window quits on Windows/Linux even while the mini-player is open (spec §5.2).
    onMainClosed: () => { if (process.platform !== 'darwin') app.quit() },
    onVisibilityChange: () => syncWindows(),
  })
  const callQuiet = (m: 'player.toggle' | 'player.next' | 'player.prev') => { supervisor.call(m, {}).catch(() => {}) }
  const tray = createTray({
    toggle: () => callQuiet('player.toggle'), next: () => callQuiet('player.next'), prev: () => callQuiet('player.prev'),
    toggleMain: () => windows.toggleMainVisible(),
    toggleMini: () => windows.toggleMini(),
    quit: () => app.quit(),
  })
  // Deferred a tick: a window's isVisible() settles just after its show/hide event.
  const syncWindows = () => setTimeout(() => {
    const main = windows.main
    tray.update({ mainVisible: !!main && !main.isDestroyed() && main.isVisible() && !main.isMinimized(), miniOpen: windows.miniOpen })
  }, 0)
  supervisor.on('notification', (m: string, p: unknown) => {
    if (m !== 'player.state') return
    const s = p as { status: string; track: { title: string; artist: string | null } | null }
    const stopped = s.status === 'stopped'
    tray.update({ playing: s.status === 'playing', title: stopped ? null : s.track?.title ?? null, artist: stopped ? null : s.track?.artist ?? null })
  })
  const engineLog = logger('engine.log')
  supervisor.on('log', engineLog)
  supervisor.on('notification', (m: string, p: unknown) => windows.send(m, p))
  supervisor.on('status', (s) => { engineLog(`[status] ${JSON.stringify(s)}`); windows.send('engine.status', s) })

  supervisor.on('frame', (t: number, d: Uint8Array) => viz.frame(t, d))
  supervisor.on('status', (s) => { if (s.state === 'ready') viz.engineReady() })

  const fileOpener = createFileOpener({ call: (m, p) => supervisor.call(m, p), onOpen: () => windows.showMain(), onError: (e) => engineLog(`[open files] ${String(e)}`) })
  opener = fileOpener
  supervisor.on('status', (s) => { if (s.state === 'ready') fileOpener.engineReady(); else fileOpener.engineLost() })
  fileOpener.open(startupFiles)
  fileOpener.open(pendingOpen)
  pendingOpen = []

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
    if (!windows.main) return []
    const r = await dialog.showOpenDialog(windows.main, { properties: ['openFile', 'multiSelections'], filters: [{ name: 'Audio', extensions: [...AUDIO_EXTENSIONS] }] })
    return r.canceled ? [] : r.filePaths
  })
  ipcMain.handle('aurora:openFolder', async () => {
    const test = testEnv('AURORA_TEST_OPEN_FOLDER')
    if (test) return test
    if (!windows.main) return null
    const r = await dialog.showOpenDialog(windows.main, { properties: ['openDirectory'] })
    return r.canceled ? null : r.filePaths[0] ?? null
  })
  ipcMain.handle('aurora:pickM3u', async () => {
    const test = testEnv('AURORA_TEST_OPEN_M3U')
    if (test) return test
    if (!windows.main) return null
    const r = await dialog.showOpenDialog(windows.main, { properties: ['openFile'], filters: [{ name: 'Playlists', extensions: ['m3u8', 'm3u'] }] })
    return r.canceled ? null : r.filePaths[0] ?? null
  })
  ipcMain.handle('aurora:pickM3uSave', async (_e, defaultName: unknown) => {
    const test = testEnv('AURORA_TEST_SAVE_M3U')
    if (test) return test
    if (!windows.main) return null
    const name = typeof defaultName === 'string' ? defaultName.replace(/[\\/:*?"<>|]/g, '_') : 'Playlist'
    const r = await dialog.showSaveDialog(windows.main, { defaultPath: `${name}.m3u8`, filters: [{ name: 'Playlists', extensions: ['m3u8'] }] })
    return r.canceled ? null : r.filePath ?? null
  })
  ipcMain.handle('aurora:showItemInFolder', (_e, path: unknown) => {
    if (typeof path === 'string' && isAbsolute(path)) shell.showItemInFolder(path)
  })
  ipcMain.handle('aurora:getSettings', () => settings.get())
  ipcMain.handle('aurora:setSettings', (_e, patch: unknown) => settings.update(patch))
  ipcMain.handle('aurora:status', () => supervisor.status)
  ipcMain.handle('aurora:restartEngine', () => supervisor.restart())
  ipcMain.on('aurora:viz', (e, on: unknown) => {
    const id = e.sender.id
    viz.set(id, on === true)
    hookSender(e.sender, viz, hookedSenders)
  })
  ipcMain.handle('aurora:toggleMini', () => { windows.toggleMini(); syncWindows() })
  ipcMain.handle('aurora:showMain', () => { windows.showMain(); syncWindows() })
  ipcMain.handle('aurora:openLogs', async () => { await shell.openPath(logDir()) })
  const build = readBuildInfo({ isPackaged: app.isPackaged, resourcesPath: process.resourcesPath })
  const licensesPath = () => (app.isPackaged ? join(process.resourcesPath, 'THIRD_PARTY_LICENSES.txt') : join(app.getAppPath(), 'build/generated/THIRD_PARTY_LICENSES.txt'))
  ipcMain.handle('aurora:openLicenses', async () => { if (existsSync(licensesPath())) await shell.openPath(licensesPath()) })

  supervisor.start()

  const mode = updateMode(kind, build)
  const updates = createUpdateController({
    mode, currentVersion: app.getVersion(), enabled: () => settings.get().autoUpdate,
    autoUpdater: () => { const { autoUpdater } = require('electron-updater') as typeof import('electron-updater'); autoUpdater.logger = { info: engineLog, warn: engineLog, error: engineLog, debug: () => {} } as never; return autoUpdater },
    latestVersion: () => latestPublishedVersion(net.fetch as unknown as typeof fetch),
    beforeInstall: async () => { quitting = true; windows.saveMainBounds(); settings.flush(); await supervisor.stop() },
    quit: () => app.quit(),
    onState: (s) => windows.send('update.state', s),
    setTimer: (fn, ms) => setTimeout(fn, ms), setRepeat: (fn, ms) => setInterval(fn, ms),
  })
  updates.start()
  ipcMain.handle('aurora:updateState', () => updates.state)
  ipcMain.handle('aurora:checkUpdates', () => updates.check())
  ipcMain.handle('aurora:installUpdate', () => updates.install())
  ipcMain.handle('aurora:openRelease', async () => { const s = updates.state; if ('version' in s) await shell.openExternal(releaseUrl(s.version)) })
  ipcMain.handle('aurora:appInfo', () => ({ version: app.getVersion(), variant: build.variant, platform: process.platform, arch: process.arch, licenses: existsSync(licensesPath()), updateMode: mode }))

  windows.createMain()
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) windows.createMain() })
})

// Quit only after the engine has shut down cleanly (it writes session.json on the way out).
app.on('before-quit', (e) => {
  if (quitting || !primary) return
  e.preventDefault()
  quitting = true
  windows?.saveMainBounds()
  settings?.flush()
  void supervisor.stop().finally(() => app.quit())
})
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
