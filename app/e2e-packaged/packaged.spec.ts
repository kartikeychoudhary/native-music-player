import { _electron as electron, expect, test } from '@playwright/test'
import { existsSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const exe = process.env.AURORA_PACKAGED_EXE
const variant = process.env.AURORA_EXPECT_VARIANT ?? 'standard'
const version = require('../package.json').version as string

/** 1 s of a 440 Hz sine, 44.1 kHz 16-bit stereo: something to play without the fixture generator. */
function sineWav(path: string) {
  const rate = 44_100, frames = rate, data = Buffer.alloc(frames * 4)
  for (let i = 0; i < frames; i++) { const s = Math.round(Math.sin((2 * Math.PI * 440 * i) / rate) * 8000); data.writeInt16LE(s, i * 4); data.writeInt16LE(s, i * 4 + 2) }
  const h = Buffer.alloc(44)
  h.write('RIFF', 0); h.writeUInt32LE(36 + data.length, 4); h.write('WAVEfmt ', 8); h.writeUInt32LE(16, 16); h.writeUInt16LE(1, 20); h.writeUInt16LE(2, 22)
  h.writeUInt32LE(rate, 24); h.writeUInt32LE(rate * 4, 28); h.writeUInt16LE(4, 32); h.writeUInt16LE(16, 34); h.write('data', 36); h.writeUInt32LE(data.length, 40)
  writeFileSync(path, Buffer.concat([h, data]))
}

test.skip(!exe, 'set AURORA_PACKAGED_EXE to the packaged executable')

test('the packaged app starts its bundled engine, plays a file and shows its build in About', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'aurora-packaged-'))
  const wav = join(dir, 'sine 440 ü.wav'); sineWav(wav)
  // Without the dev overrides, so the app can only run the engine it bundles.
  const { AURORA_ENGINE_PATH: _path, AURORA_ENGINE_PROFILE: _profile, ...env } = process.env
  const app = await electron.launch({ executablePath: resolve(exe!), args: [`--user-data-dir=${join(dir, 'userData')}`], env: { ...env, AURORA_OUTPUT: 'null', AURORA_OSMEDIA: '0' } })
  try {
    expect(await app.evaluate(({ app }) => app.getPath('userData')), 'the smoke test must not touch the real profile').toBe(join(dir, 'userData'))
    const win = await app.firstWindow()
    type Status = { state: string; engine?: { version: string; features: string[] } }
    const status = () => win.evaluate(() => (window as unknown as { aurora: { status: () => Promise<Status> } }).aurora.status())
    await expect.poll(async () => (await status()).state, { timeout: 30_000 }).toBe('ready')
    const engine = (await status()).engine!
    expect(engine.version).toBe(version)
    expect(engine.features.includes('fdk-aac')).toBe(variant === 'standard')
    const resources = await app.evaluate(() => process.resourcesPath)
    expect(existsSync(join(resources, 'THIRD_PARTY_LICENSES.txt'))).toBe(true)
    expect(existsSync(join(resources, 'build-info.json'))).toBe(true)
    await win.evaluate((p) => (window as unknown as { aurora: { call: (m: string, a: unknown) => Promise<unknown> } }).aurora.call('queue.setPaths', { paths: [p], startIndex: 0 }), wav)
    await win.evaluate(() => (window as unknown as { aurora: { call: (m: string, a: unknown) => Promise<unknown> } }).aurora.call('player.play', {}))
    await expect.poll(() => win.evaluate(() => (window as unknown as { aurora: { call: (m: string, a: unknown) => Promise<{ status: string }> } }).aurora.call('player.state', {}).then((s) => s.status)), { timeout: 15_000 }).toBe('playing')
    await win.getByRole('link', { name: 'Settings' }).click()
    await expect(win.getByText(`Aurora ${version}`)).toBeVisible()
    // S.buildTarget renders `<platform>-<arch>` and appends ` · without fdk-aac` for the no-fdk variant.
    if (variant === 'nofdk') await expect(win.getByText('without fdk-aac')).toBeVisible()
    else await expect(win.getByText('without fdk-aac')).toHaveCount(0)
    await expect(win.getByText(variant === 'standard' ? 'Included (Fraunhofer FDK AAC)' : 'Not in this build')).toBeVisible()
  } finally {
    await app.close()
  }
})
