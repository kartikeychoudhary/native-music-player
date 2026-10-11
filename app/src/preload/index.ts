import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron'
import type { AuroraApi } from '../shared/api'

const api: AuroraApi = {
  call: (method, params) => ipcRenderer.invoke('aurora:call', method, params),
  on: (event, cb) => {
    const handler = (_e: IpcRendererEvent, name: string, payload: unknown) => { if (name === event) cb(payload as never) }
    ipcRenderer.on('aurora:event', handler)
    return () => { ipcRenderer.removeListener('aurora:event', handler) }
  },
  openFiles: () => ipcRenderer.invoke('aurora:openFiles'),
  pathsForFiles: (files) => files.map((f) => webUtils.getPathForFile(f)).filter(Boolean),
  status: () => ipcRenderer.invoke('aurora:status'),
  restartEngine: () => ipcRenderer.invoke('aurora:restartEngine'),
  openLogs: () => ipcRenderer.invoke('aurora:openLogs'),
  platform: process.platform,
}

contextBridge.exposeInMainWorld('aurora', api)
