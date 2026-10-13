import { expect, test } from '@playwright/test'
import { resolve } from 'node:path'
import { launchAurora, root, tempDir } from './fixture'

test('a file named on the command line plays at start-up (Open with Aurora)', async () => {
  const { app, win } = await launchAurora({ dataDir: tempDir('aurora-data-'), args: [resolve(root, 'fixtures/out/gap_a.flac')], speed: 0.05 })
  try {
    type State = { status: string; track: { path: string } | null }
    // The engine is still starting for the first moments, when calls reject.
    const state = (): Promise<State> => win.evaluate(async () => {
      try { return await (window as unknown as { aurora: { call: (m: string, p: unknown) => Promise<unknown> } }).aurora.call('player.state', {}) as { status: string; track: { path: string } | null } } catch { return { status: 'starting', track: null } }
    })
    await expect.poll(async () => (await state()).status, { timeout: 30_000 }).toBe('playing')
    expect((await state()).track?.path).toMatch(/gap_a\.flac$/)
    await expect(win.getByRole('button', { name: 'Pause', exact: true })).toBeVisible()
  } finally {
    await app.close()
  }
})
