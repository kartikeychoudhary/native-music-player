import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/** Written by app/build-tools/build-info.ts and packaged as resources/build-info.json. */
export interface BuildInfo { variant: 'standard' | 'nofdk'; macSigned: boolean; commit: string | null }

export const DEV_BUILD: BuildInfo = Object.freeze({ variant: 'standard', macSigned: false, commit: null })

export function parseBuildInfo(raw: unknown): BuildInfo {
  if (typeof raw !== 'object' || raw === null) return DEV_BUILD
  const r = raw as Record<string, unknown>
  if ((r.variant !== 'standard' && r.variant !== 'nofdk') || typeof r.macSigned !== 'boolean' || (r.commit !== null && typeof r.commit !== 'string')) return DEV_BUILD
  return { variant: r.variant, macSigned: r.macSigned, commit: r.commit }
}

export function readBuildInfo(o: { isPackaged: boolean; resourcesPath: string; read?: (p: string) => string }): BuildInfo {
  if (!o.isPackaged) return DEV_BUILD
  try { return parseBuildInfo(JSON.parse((o.read ?? ((p) => readFileSync(p, 'utf8')))(join(o.resourcesPath, 'build-info.json')))) } catch { return DEV_BUILD }
}
