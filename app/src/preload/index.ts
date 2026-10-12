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
  openFolder: () => ipcRenderer.invoke('aurora:openFolder'),
  pickM3u: () => ipcRenderer.invoke('aurora:pickM3u'),
  pickM3uSave: (defaultName) => ipcRenderer.invoke('aurora:pickM3uSave', defaultName),
  showItemInFolder: (path) => ipcRenderer.invoke('aurora:showItemInFolder', path),
  getSettings: () => ipcRenderer.invoke('aurora:getSettings'),
  setSettings: (patch) => ipcRenderer.invoke('aurora:setSettings', patch),
  pathsForFiles: (files) => files.map((f) => webUtils.getPathForFile(f)).filter(Boolean),
  status: () => ipcRenderer.invoke('aurora:status'),
  restartEngine: () => ipcRenderer.invoke('aurora:restartEngine'),
  openLogs: () => ipcRenderer.invoke('aurora:openLogs'),
  toggleMini: () => ipcRenderer.invoke('aurora:toggleMini'),
  showMain: () => ipcRenderer.invoke('aurora:showMain'),
  setViz: (on) => ipcRenderer.send('aurora:viz', on === true),
  onSpectrum: (cb) => {
    const handler = (_e: IpcRendererEvent, bins: Uint8Array) => cb(bins)
    ipcRenderer.on('aurora:spectrum', handler)
    return () => { ipcRenderer.removeListener('aurora:spectrum', handler) }
  },
  platform: process.platform,
}

contextBridge.exposeInMainWorld('aurora', api)
