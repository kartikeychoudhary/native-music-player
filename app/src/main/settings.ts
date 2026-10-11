import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import { DEFAULT_SETTINGS, mergeSettings, type Bounds, type Settings } from '@shared/settings'

export function loadSettings(file: string): Settings {
  try { return mergeSettings(DEFAULT_SETTINGS, JSON.parse(readFileSync(file, 'utf8'))) } catch { return { ...DEFAULT_SETTINGS } }
}

export class SettingsStore {
  private current: Settings
  private timer: ReturnType<typeof setTimeout> | null = null
  constructor(private readonly file: string) { this.current = loadSettings(file) }
  get(): Settings { return this.current }
  update(patch: unknown): Settings {
    this.current = mergeSettings(this.current, patch)
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => this.flush(), 300)
    return this.current
  }
  flush(): void {
    if (this.timer) { clearTimeout(this.timer); this.timer = null }
    try {
      mkdirSync(dirname(this.file), { recursive: true })
      const tmp = `${this.file}.tmp`
      writeFileSync(tmp, JSON.stringify(this.current, null, 2))
      renameSync(tmp, this.file)
    } catch { /* preferences are best-effort */ }
  }
}

/** True when at least an 80×80 px piece of `b` lies on one of the screens' work areas. */
export function boundsVisible(b: Bounds, workAreas: { x: number; y: number; width: number; height: number }[]): boolean {
  return workAreas.some((a) => {
    const w = Math.min(b.x + b.width, a.x + a.width) - Math.max(b.x, a.x)
    const h = Math.min(b.y + b.height, a.y + a.height) - Math.max(b.y, a.y)
    return w >= 80 && h >= 80
  })
}
