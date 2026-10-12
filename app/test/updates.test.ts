import { EventEmitter } from 'node:events'
import { describe, expect, it, vi } from 'vitest'
import { DEV_BUILD } from '../src/main/buildInfo'
import { CHECK_EVERY_MS, CHECK_TIMEOUT_MS, FIRST_CHECK_MS, QUIT_FALLBACK_MS, createUpdateController, isNewer, latestPublishedVersion, releaseUrl, updateMode, type UpdateState } from '../src/main/updates'

const signed = { ...DEV_BUILD, macSigned: true }
const nofdk = { ...DEV_BUILD, variant: 'nofdk' as const }

describe('update mode', () => {
  it('auto-installs only where the installer can replace itself', () => {
    expect(updateMode('nsis', DEV_BUILD)).toBe('auto')
    expect(updateMode('appimage', DEV_BUILD)).toBe('auto')
    expect(updateMode('mac', signed)).toBe('auto')
  })
  it('only links to the release page for portable, package-managed, unsigned macOS and no-fdk builds', () => {
    expect(updateMode('win-zip', DEV_BUILD)).toBe('notify')
    expect(updateMode('linux-package', DEV_BUILD)).toBe('notify')
    expect(updateMode('mac', DEV_BUILD)).toBe('notify')
    for (const k of ['nsis', 'appimage', 'mac', 'win-zip'] as const) expect(updateMode(k, { ...nofdk, macSigned: true })).toBe('notify')
  })
  it('is off in development', () => { expect(updateMode('dev', DEV_BUILD)).toBe('off') })
})

describe('versions', () => {
  it('compares semver with prereleases', () => {
    expect(isNewer('0.2.0', '0.1.0')).toBe(true)
    expect(isNewer('0.1.10', '0.1.9')).toBe(true)
    expect(isNewer('1.0.0', '1.0.0-rc.2')).toBe(true)
    expect(isNewer('1.0.0-rc.2', '1.0.0')).toBe(false)
    expect(isNewer('0.1.0', '0.1.0')).toBe(false)
    expect(isNewer('garbage', '0.1.0')).toBe(false)
  })
  it('builds the release page URL', () => {
    expect(releaseUrl('0.2.0')).toBe('https://github.com/kartikeychoudhary/native-music-player/releases/tag/v0.2.0')
  })
  it('reads the latest published release, treating 404 as none and other failures as errors', async () => {
    const ok = vi.fn(async (_url: string, _init?: RequestInit) => new Response(JSON.stringify({ tag_name: 'v0.3.1' }), { status: 200 }))
    expect(await latestPublishedVersion(ok as unknown as typeof fetch)).toBe('0.3.1')
    expect(ok.mock.calls[0][0]).toBe('https://api.github.com/repos/kartikeychoudhary/native-music-player/releases/latest')
    expect(ok.mock.calls[0][1]?.signal).toBeInstanceOf(AbortSignal)
    expect(CHECK_TIMEOUT_MS).toBe(30_000)
    expect(await latestPublishedVersion((async () => new Response('', { status: 404 })) as unknown as typeof fetch)).toBeNull()
    await expect(latestPublishedVersion((async () => new Response('', { status: 403 })) as unknown as typeof fetch)).rejects.toThrow('GitHub answered 403')
  })
})

class FakeUpdater extends EventEmitter {
  autoDownload = false; autoInstallOnAppQuit = false
  checkForUpdates = vi.fn(async () => null)
  quitAndInstall = vi.fn()
}

function setup(o: { mode: 'auto' | 'notify' | 'off'; latest?: () => Promise<string | null>; enabled?: boolean }) {
  const states: UpdateState[] = [], timers: { fn: () => void; ms: number; repeat: boolean }[] = []
  const updater = new FakeUpdater()
  const beforeInstall = vi.fn(async () => {})
  const quit = vi.fn()
  const c = createUpdateController({
    mode: o.mode, currentVersion: '0.1.0', enabled: () => o.enabled ?? true,
    autoUpdater: () => updater, latestVersion: o.latest ?? (async () => null), beforeInstall, quit,
    onState: (s) => states.push(s),
    setTimer: (fn, ms) => timers.push({ fn, ms, repeat: false }), setRepeat: (fn, ms) => timers.push({ fn, ms, repeat: true }),
  })
  return { c, states, timers, updater, beforeInstall, quit }
}

describe('update controller', () => {
  it('schedules a first check after 10 s and one every 6 h, and none when off', () => {
    const { c, timers } = setup({ mode: 'notify' })
    c.start()
    expect(timers.map((t) => [t.ms, t.repeat])).toEqual([[FIRST_CHECK_MS, false], [CHECK_EVERY_MS, true]])
    const off = setup({ mode: 'off' }); off.c.start(); expect(off.timers).toEqual([])
  })
  it('notify mode: a newer published version becomes "available"', async () => {
    const { c, states } = setup({ mode: 'notify', latest: async () => '0.2.0' })
    await c.check()
    expect(states).toEqual([{ status: 'checking' }, { status: 'available', version: '0.2.0' }])
  })
  it('automatic checks are skipped when the user turned them off; manual ones are not', async () => {
    const latest = vi.fn(async () => '0.2.0')
    const { c, timers } = setup({ mode: 'notify', latest, enabled: false })
    c.start(); timers[0].fn(); await Promise.resolve()
    expect(latest).not.toHaveBeenCalled()
    await c.check(); expect(latest).toHaveBeenCalledTimes(1)
  })
  it('a failed automatic check is silent and keeps what was known; a failed manual check says why', async () => {
    let fail = false
    const { c, states, timers } = setup({ mode: 'notify', latest: async () => { if (fail) throw new Error('offline'); return '0.2.0' } })
    await c.check()
    fail = true
    c.start(); timers[0].fn(); await new Promise((r) => setTimeout(r, 0))
    expect(c.state).toEqual({ status: 'available', version: '0.2.0' })
    await c.check()
    expect(states.at(-1)).toEqual({ status: 'error', message: 'offline' })
  })
  it('auto mode downloads, reports progress, and waits for the user to install', async () => {
    const { c, states, updater, beforeInstall } = setup({ mode: 'auto' })
    await c.check()
    expect(updater.autoDownload && updater.autoInstallOnAppQuit).toBe(true)
    updater.emit('update-available', { version: '0.2.0' })
    updater.emit('download-progress', { percent: 41.6 })
    updater.emit('update-downloaded', { version: '0.2.0' })
    expect(states.slice(-3)).toEqual([{ status: 'downloading', version: '0.2.0', percent: 0 }, { status: 'downloading', version: '0.2.0', percent: 42 }, { status: 'ready', version: '0.2.0' }])
    expect(updater.quitAndInstall).not.toHaveBeenCalled()
    await c.install()
    expect(beforeInstall).toHaveBeenCalledBefore(updater.quitAndInstall)
    expect(updater.quitAndInstall).toHaveBeenCalledWith(true, true)
  })
  it('quits the app shortly after quitAndInstall in case electron-updater declined to', async () => {
    const { c, updater, timers, quit } = setup({ mode: 'auto' })
    await c.check()
    updater.emit('update-downloaded', { version: '0.2.0' })
    await c.install()
    expect(updater.quitAndInstall).toHaveBeenCalledWith(true, true)
    expect(timers.map((t) => [t.ms, t.repeat])).toEqual([[QUIT_FALLBACK_MS, false]])
    expect(quit).not.toHaveBeenCalled()
    timers[0].fn()
    expect(quit).toHaveBeenCalledTimes(1)
  })
  it('a failed automatic download goes back to what was known, without an error or banner state', async () => {
    const { c, states, updater, timers } = setup({ mode: 'auto' })
    updater.checkForUpdates.mockImplementation(async () => { updater.emit('update-available', { version: '0.2.0' }); updater.emit('error', new Error('offline')); return null })
    c.start(); timers[0].fn(); await new Promise((r) => setTimeout(r, 0))
    expect(states.map((s) => s.status)).toEqual(['checking', 'downloading', 'idle'])
    expect(c.state).toEqual({ status: 'idle' })
  })
  it('a rejected download promise does not escape as an unhandled rejection', async () => {
    const { c, updater } = setup({ mode: 'auto' })
    const seen = vi.fn()
    process.on('unhandledRejection', seen)
    try {
      updater.checkForUpdates.mockImplementation((async () => ({ downloadPromise: Promise.reject(new Error('offline')) })) as never)
      await c.check()
      await new Promise((r) => setTimeout(r, 10))
      expect(seen).not.toHaveBeenCalled()
    } finally { process.off('unhandledRejection', seen) }
  })
  it('install does nothing unless an update is ready', async () => {
    const { c, updater, beforeInstall } = setup({ mode: 'auto' })
    await c.install()
    expect(beforeInstall).not.toHaveBeenCalled(); expect(updater.quitAndInstall).not.toHaveBeenCalled()
  })
  it('does not start a second check while one is running or an update is ready', async () => {
    const { c, updater } = setup({ mode: 'auto' })
    const first = c.check(); const second = c.check(); await Promise.all([first, second])
    expect(updater.checkForUpdates).toHaveBeenCalledTimes(1)
    updater.emit('update-downloaded', { version: '0.2.0' }); await c.check()
    expect(updater.checkForUpdates).toHaveBeenCalledTimes(1)
  })
})
