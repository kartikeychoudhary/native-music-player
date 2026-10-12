import type { BuildInfo } from './buildInfo'
import type { InstallKind } from './installKind'
import type { UpdateMode, UpdateState } from '@shared/api'

export type { UpdateMode, UpdateState }

export const RELEASES = { owner: 'kartikeychoudhary', repo: 'native-music-player' } as const
export const FIRST_CHECK_MS = 10_000
export const CHECK_EVERY_MS = 6 * 60 * 60 * 1000
/** A request that hangs would leave the state at `checking` for good, skipping every later check. */
export const CHECK_TIMEOUT_MS = 30_000
/** How long electron-updater gets to quit the app after `quitAndInstall` before Aurora quits itself. */
export const QUIT_FALLBACK_MS = 5_000

/** Which builds install updates themselves; everything else only links to the release page. */
export function updateMode(kind: InstallKind, build: BuildInfo): UpdateMode {
  if (kind === 'dev') return 'off'
  if (build.variant === 'nofdk') return 'notify' // the update feed is the standard variant's
  if (kind === 'nsis' || kind === 'appimage') return 'auto'
  if (kind === 'mac') return build.macSigned ? 'auto' : 'notify' // Squirrel.Mac needs a signed app
  return 'notify'
}

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/

export function isNewer(candidate: string, current: string): boolean {
  const a = SEMVER.exec(candidate), b = SEMVER.exec(current)
  if (!a || !b) return false
  for (let i = 1; i <= 3; i++) { const d = Number(a[i]) - Number(b[i]); if (d) return d > 0 }
  if (!a[4] || !b[4]) return !a[4] && !!b[4] // a release beats its prereleases
  return a[4].localeCompare(b[4], 'en', { numeric: true }) > 0
}

export function releaseUrl(version: string): string {
  return `https://github.com/${RELEASES.owner}/${RELEASES.repo}/releases/tag/v${version}`
}

/** The newest published (not draft, not prerelease) version on GitHub, or null if there is none. */
export async function latestPublishedVersion(fetchFn: typeof fetch): Promise<string | null> {
  const r = await fetchFn(`https://api.github.com/repos/${RELEASES.owner}/${RELEASES.repo}/releases/latest`, { headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'Aurora' }, signal: AbortSignal.timeout(CHECK_TIMEOUT_MS) })
  if (r.status === 404) return null
  if (!r.ok) throw new Error(`GitHub answered ${r.status}`)
  const j = (await r.json()) as { tag_name?: unknown }
  return typeof j.tag_name === 'string' ? j.tag_name.replace(/^v/, '') : null
}

/** The slice of electron-updater's AppUpdater this module uses. */
export interface AutoUpdaterLike {
  autoDownload: boolean
  autoInstallOnAppQuit: boolean
  // `any`: electron-updater types each event's listener separately; this slice only needs them to be callable.
  on(event: string, cb: (arg: any) => void): unknown
  checkForUpdates(): Promise<unknown>
  quitAndInstall(isSilent?: boolean, isForceRunAfter?: boolean): void
}

export interface ControllerDeps {
  mode: UpdateMode
  currentVersion: string
  /** Whether automatic checks are on (Settings); manual checks ignore it. */
  enabled: () => boolean
  autoUpdater: () => AutoUpdaterLike
  latestVersion: () => Promise<string | null>
  /** Stops the engine so the installer can replace its binary. */
  beforeInstall: () => Promise<void>
  /** Quits the app (the fallback when quitAndInstall declines to). */
  quit: () => void
  onState: (s: UpdateState) => void
  setTimer: (fn: () => void, ms: number) => unknown
  setRepeat: (fn: () => void, ms: number) => unknown
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

export function createUpdateController(d: ControllerDeps) {
  let state: UpdateState = { status: 'idle' }
  let before: UpdateState = state
  let manual = false
  let au: AutoUpdaterLike | null = null
  const set = (s: UpdateState) => { state = s; d.onState(s) }
  const fail = (e: unknown) => {
    if (state.status !== 'checking' && state.status !== 'downloading') return
    set(manual ? { status: 'error', message: message(e) } : before)
  }
  const updater = () => {
    if (au) return au
    au = d.autoUpdater()
    au.autoDownload = true
    au.autoInstallOnAppQuit = true
    au.on('update-available', (i: { version: string }) => set({ status: 'downloading', version: i.version, percent: 0 }))
    au.on('download-progress', (p: { percent: number }) => { if (state.status === 'downloading') set({ ...state, percent: Math.round(p.percent) }) })
    au.on('update-downloaded', (i: { version: string }) => set({ status: 'ready', version: i.version }))
    au.on('update-not-available', () => { if (state.status === 'checking') set({ status: 'current' }) })
    au.on('error', (e: unknown) => fail(e))
    return au
  }
  async function check(isManual: boolean) {
    if (d.mode === 'off' || ['checking', 'downloading', 'ready'].includes(state.status)) return
    if (!isManual && !d.enabled()) return
    manual = isManual
    before = state
    set({ status: 'checking' })
    try {
      if (d.mode === 'auto') {
        const r = (await updater().checkForUpdates()) as { downloadPromise?: Promise<unknown> } | null
        // A failed download already reached `fail` through the 'error' event, but electron-updater also rejects this promise.
        void r?.downloadPromise?.catch(() => {})
        // electron-updater emits its events before resolving; one that resolved silently (inactive updater) found nothing.
        if (state.status === 'checking') set({ status: 'current' })
      } else {
        const v = await d.latestVersion()
        set(v && isNewer(v, d.currentVersion) ? { status: 'available', version: v } : { status: 'current' })
      }
    } catch (e) { fail(e) }
  }
  return {
    get state() { return state },
    start() {
      if (d.mode === 'off') return
      d.setTimer(() => void check(false), FIRST_CHECK_MS)
      d.setRepeat(() => void check(false), CHECK_EVERY_MS)
    },
    check: () => check(true),
    async install() {
      if (state.status !== 'ready' || !au) return
      await d.beforeInstall()
      au.quitAndInstall(true, true)
      // quitAndInstall returns without quitting when its install step fails; the engine is already stopped, so quit cleanly.
      d.setTimer(() => d.quit(), QUIT_FALLBACK_MS)
    },
  }
}
