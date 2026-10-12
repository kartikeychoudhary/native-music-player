import { mkdirSync, mkdtempSync, statSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
const hook = createRequire(import.meta.url)('../build-tools/afterPack.cjs') as { enginePathInApp: (out: string, platform: string, arch: string, product: string) => string; default: (ctx: unknown) => Promise<void> }

describe('afterPack', () => {
  it('locates the engine inside each platform layout', () => {
    expect(hook.enginePathInApp('/d/win-unpacked', 'win32', 'x64', 'Aurora').replace(/\\/g, '/')).toBe('/d/win-unpacked/resources/bin/win32-x64/aurora-engine.exe')
    expect(hook.enginePathInApp('/d/mac-arm64', 'darwin', 'arm64', 'Aurora').replace(/\\/g, '/')).toBe('/d/mac-arm64/Aurora.app/Contents/Resources/bin/darwin-arm64/aurora-engine')
    expect(hook.enginePathInApp('/d/linux-unpacked', 'linux', 'x64', 'Aurora').replace(/\\/g, '/')).toBe('/d/linux-unpacked/resources/bin/linux-x64/aurora-engine')
  })
  it('fails the build when the engine for that arch is missing, and makes it executable when present', async () => {
    const out = mkdtempSync(join(tmpdir(), 'aurora-pack-'))
    const ctx = { appOutDir: out, electronPlatformName: 'linux', arch: 3 /* arm64 */, packager: { appInfo: { productFilename: 'Aurora' } } }
    await expect(hook.default(ctx)).rejects.toThrow(/no engine for linux-arm64/)
    mkdirSync(join(out, 'resources/bin/linux-arm64'), { recursive: true })
    writeFileSync(join(out, 'resources/bin/linux-arm64/aurora-engine'), 'bin')
    await hook.default(ctx)
    if (process.platform !== 'win32') expect(statSync(join(out, 'resources/bin/linux-arm64/aurora-engine')).mode & 0o755).toBe(0o755)
  })
})
