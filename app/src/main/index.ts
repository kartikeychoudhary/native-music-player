import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron'
import { spawn } from 'node:child_process'
import { delimiter, join } from 'node:path'
import { PROTOCOL_VERSION } from '@protocol/ProtocolVersion'
import { AUDIO_EXTENSIONS, isAllowedMethod } from '@shared/api'
import { enginePath } from './engine/enginePath'
import { EngineSupervisor } from './engine/supervisor'
import { logDir, logger } from './logs'

let win: BrowserWindow | null = null

const binary = () => enginePath({ isPackaged: app.isPackaged, resourcesPath: process.resourcesPath, appPath: app.getAppPath(), platform: process.platform, arch: process.arch, env: process.env })

const supervisor = new EngineSupervisor(() => spawn(binary(), ['rpc'], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }), { protocolVersion: PROTOCOL_VERSION })

function send(event: string, payload: unknown) {
  if (win && !win.isDestroyed()) win.webContents.send('aurora:event', event, payload)
}

function createWindow() {
  const mac = process.platform === 'darwin'
  win = new BrowserWindow({
    width: 1280, height: 800, minWidth: 940, minHeight: 600, show: false,
    backgroundColor: '#17151A',
    titleBarStyle: mac ? 'hiddenInset' : 'hidden',
    titleBarOverlay: mac ? undefined : { color: '#100E12', symbolColor: '#EFE9E1', height: 36 },
    webPreferences: { preload: join(__dirname, '../preload/index.js'), contextIsolation: true, nodeIntegration: false, sandbox: true },
  })
  win.once('ready-to-show', () => win?.show())
  if (process.env.ELECTRON_RENDERER_URL) win.loadURL(process.env.ELECTRON_RENDERER_URL)
  else win.loadFile(join(__dirname, '../renderer/index.html'))
}

app.whenReady().then(() => {
  const engineLog = logger('engine.log')
  supervisor.on('log', engineLog)
  supervisor.on('notification', (m: string, p: unknown) => send(m, p))
  supervisor.on('status', (s) => { send('engine.status', s); engineLog(`[status] ${JSON.stringify(s)}`) })

  ipcMain.handle('aurora:call', (_e, method: unknown, params: unknown) => {
    if (!isAllowedMethod(method)) throw new Error(`method not allowed: ${String(method)}`)
    return supervisor.call(method, params ?? {})
  })
  ipcMain.handle('aurora:openFiles', async () => {
    const test = process.env.AURORA_TEST_OPEN_FILES
    if (test) return test.split(delimiter).filter(Boolean)
    const r = await dialog.showOpenDialog(win!, { properties: ['openFile', 'multiSelections'], filters: [{ name: 'Audio', extensions: [...AUDIO_EXTENSIONS] }] })
    return r.canceled ? [] : r.filePaths
  })
  ipcMain.handle('aurora:status', () => supervisor.status)
  ipcMain.handle('aurora:restartEngine', () => supervisor.restart())
  ipcMain.handle('aurora:openLogs', async () => { await shell.openPath(logDir()) })

  supervisor.start()
  createWindow()
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow() })
})

app.on('before-quit', () => { void supervisor.stop() })
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
