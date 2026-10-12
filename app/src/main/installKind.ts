import { existsSync } from 'node:fs'
import path from 'node:path'

/** How this copy of Aurora was installed; decides the update mode and the Windows identity. */
export type InstallKind = 'dev' | 'nsis' | 'win-zip' | 'mac' | 'appimage' | 'linux-package'

export function installKind(o: { isPackaged: boolean; platform: string; execPath: string; env: Record<string, string | undefined>; exists?: (p: string) => boolean }): InstallKind {
  if (!o.isPackaged) return 'dev'
  const exists = o.exists ?? existsSync
  // Pick the path flavour from the modelled platform, not the host: both are pure string functions.
  const p = o.platform === 'win32' ? path.win32 : path.posix
  if (o.platform === 'win32') return exists(p.join(p.dirname(o.execPath), 'Uninstall Aurora.exe')) ? 'nsis' : 'win-zip'
  if (o.platform === 'darwin') return 'mac'
  return o.env.APPIMAGE ? 'appimage' : 'linux-package'
}
