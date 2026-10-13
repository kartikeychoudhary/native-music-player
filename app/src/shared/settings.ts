/** UI preferences owned by Electron (spec §5.2). */
export interface Bounds { x: number; y: number; width: number; height: number; maximized: boolean }
export interface Point { x: number; y: number }
export interface Settings { windowBounds: Bounds | null; sidebarExpanded: boolean; lastView: string; miniBounds: Point | null; accentTint: boolean; autoUpdate: boolean; resumeAfterUpdate: boolean }

export const DEFAULT_SETTINGS: Settings = { windowBounds: null, sidebarExpanded: false, lastView: '/', miniBounds: null, accentTint: false, autoUpdate: true, resumeAfterUpdate: false }

const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

function bounds(v: unknown): Bounds | null | undefined {
  if (v === null) return null
  if (typeof v !== 'object' || v === null) return undefined
  const b = v as Record<string, unknown>
  if (![b.x, b.y, b.width, b.height].every(num) || (b.width as number) < 200 || (b.height as number) < 200) return undefined
  return { x: b.x as number, y: b.y as number, width: b.width as number, height: b.height as number, maximized: b.maximized === true }
}

/** `base` with every valid field of `raw` applied; anything unknown or malformed is ignored. */
export function mergeSettings(base: Settings, raw: unknown): Settings {
  const out: Settings = { ...base }
  if (typeof raw !== 'object' || raw === null) return out
  const r = raw as Record<string, unknown>
  if (typeof r.sidebarExpanded === 'boolean') out.sidebarExpanded = r.sidebarExpanded
  if (typeof r.lastView === 'string' && r.lastView.startsWith('/')) out.lastView = r.lastView.length <= 200 ? r.lastView : '/'
  if (typeof r.accentTint === 'boolean') out.accentTint = r.accentTint
  if (typeof r.autoUpdate === 'boolean') out.autoUpdate = r.autoUpdate
  if (typeof r.resumeAfterUpdate === 'boolean') out.resumeAfterUpdate = r.resumeAfterUpdate
  if ('miniBounds' in r) {
    const m = r.miniBounds as Record<string, unknown> | null
    out.miniBounds = m && num(m.x) && num(m.y) ? { x: m.x as number, y: m.y as number } : null
  }
  if ('windowBounds' in r) { const b = bounds(r.windowBounds); if (b !== undefined) out.windowBounds = b }
  return out
}
