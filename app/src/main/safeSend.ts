export type SendTarget = {
  isDestroyed(): boolean
  webContents: { isDestroyed(): boolean; send(channel: string, ...args: unknown[]): void }
}

/** Sends to a renderer without ever throwing; returns whether the message was handed to webContents. */
export function safeSend(target: SendTarget | null, channel: string, ...args: unknown[]): boolean {
  if (!target || target.isDestroyed() || target.webContents.isDestroyed()) return false
  try {
    target.webContents.send(channel, ...args)
    return true
  } catch {
    return false // e.g. "Render frame was disposed" during reload/crash/shutdown
  }
}
