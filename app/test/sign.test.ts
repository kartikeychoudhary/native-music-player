import { mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'

const req = createRequire(import.meta.url)
const cache = req('../build-tools/signCache.cjs') as { cacheName: (f: string) => string; collect: (f: string, d: string) => string; signedCopy: (f: string, d: string) => string | null; kindOf: (f: string) => string }
const sign = req('../build-tools/sign.cjs') as (c: { path: string }) => Promise<void>

const tmp = () => mkdtempSync(join(tmpdir(), 'aurora-sign-'))
const file = (dir: string, name: string, body: string) => { const p = join(dir, name); writeFileSync(p, body); return p }

describe('sign cache', () => {
  it('names a file by its content hash and base name', () => {
    const d = tmp()
    const a = file(d, 'Aurora.exe', 'one'), b = file(join(d), 'other.exe', 'one')
    expect(cache.cacheName(a)).toMatch(/^[0-9a-f]{16}-Aurora\.exe$/)
    expect(cache.cacheName(a).slice(0, 16)).toBe(cache.cacheName(b).slice(0, 16))
    writeFileSync(a, 'two')
    expect(cache.cacheName(a).slice(0, 16)).not.toBe(cache.cacheName(b).slice(0, 16))
  })
  it('finds the signed copy of exactly the collected bytes', () => {
    const d = tmp(), f = file(d, 'ffmpeg.dll', 'dll bytes')
    const name = cache.collect(f, join(d, 'to-sign'))
    expect(readdirSync(join(d, 'to-sign'))).toEqual([name])
    mkdirSync(join(d, 'signed'))
    writeFileSync(join(d, 'signed', name), 'signed dll bytes')
    expect(readFileSync(cache.signedCopy(f, join(d, 'signed'))!, 'utf8')).toBe('signed dll bytes')
    writeFileSync(f, 'changed')
    expect(cache.signedCopy(f, join(d, 'signed'))).toBeNull()
  })
  it('tells the installer and uninstaller from app binaries', () => {
    expect(cache.kindOf('C:\\d\\Aurora-0.1.0-win-setup.exe')).toBe('installer')
    expect(cache.kindOf('/d/Aurora-0.1.0-win-setup.__uninstaller.exe')).toBe('uninstaller')
    expect(cache.kindOf('/d/win-unpacked/Aurora.exe')).toBe('binary')
    expect(cache.kindOf('/d/win-unpacked/vk_swiftshader.dll')).toBe('binary')
  })
})

describe('sign hook', () => {
  afterEach(() => { delete process.env.AURORA_SIGN_MODE; delete process.env.AURORA_SIGN_DIR; vi.restoreAllMocks() })

  it('does nothing when signing is off', async () => {
    const d = tmp(), f = file(d, 'Aurora.exe', 'x')
    await sign({ path: f })
    process.env.AURORA_SIGN_MODE = 'off'
    await sign({ path: f })
    expect(readFileSync(f, 'utf8')).toBe('x')
  })
  it('collects binaries and the uninstaller but never the installer', async () => {
    const d = tmp()
    process.env.AURORA_SIGN_MODE = 'collect'; process.env.AURORA_SIGN_DIR = d
    await sign({ path: file(d, 'Aurora.exe', 'exe') })
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.__uninstaller.exe', 'un') })
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.exe', 'setup') })
    expect(readdirSync(join(d, 'to-sign')).map((n) => n.slice(17)).sort()).toEqual(['Aurora-0.1.0-win-setup.__uninstaller.exe', 'Aurora.exe'])
  })
  it('swaps in the signed copy, and fails loudly for a binary that was never signed', async () => {
    const d = tmp(), f = file(d, 'Aurora.exe', 'exe')
    mkdirSync(join(d, 'signed'))
    writeFileSync(join(d, 'signed', cache.cacheName(f)), 'signed exe')
    process.env.AURORA_SIGN_MODE = 'apply'; process.env.AURORA_SIGN_DIR = d
    await sign({ path: f })
    expect(readFileSync(f, 'utf8')).toBe('signed exe')
    await expect(sign({ path: file(d, 'libEGL.dll', 'never collected') })).rejects.toThrow(/no signed copy of .*libEGL\.dll/)
  })
  it('leaves an unmatched uninstaller unsigned with a warning, and skips the installer', async () => {
    const d = tmp(); mkdirSync(join(d, 'signed'))
    process.env.AURORA_SIGN_MODE = 'apply'; process.env.AURORA_SIGN_DIR = d
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.__uninstaller.exe', 'regenerated') })
    await sign({ path: file(d, 'Aurora-0.1.0-win-setup.exe', 'setup') })
    expect(warn).toHaveBeenCalledTimes(1)
  })
  it('refuses to run without a sign dir or with an unknown mode', async () => {
    const d = tmp(), f = file(d, 'Aurora.exe', 'x')
    process.env.AURORA_SIGN_MODE = 'collect'
    await expect(sign({ path: f })).rejects.toThrow(/AURORA_SIGN_DIR/)
    process.env.AURORA_SIGN_DIR = d; process.env.AURORA_SIGN_MODE = 'later'
    await expect(sign({ path: f })).rejects.toThrow(/unknown AURORA_SIGN_MODE/)
  })
})
