import { join } from 'node:path'

export function enginePath(o: { isPackaged: boolean; resourcesPath: string; appPath: string; platform: string; arch: string; env: Record<string, string | undefined> }): string {
  if (o.env.AURORA_ENGINE_PATH) return o.env.AURORA_ENGINE_PATH
  const exe = o.platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'
  if (o.isPackaged) return join(o.resourcesPath, 'bin', `${o.platform}-${o.arch}`, exe)
  return join(o.appPath, '..', 'engine', 'target', o.env.AURORA_ENGINE_PROFILE ?? 'debug', exe)
}
