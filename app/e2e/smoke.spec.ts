import { _electron as electron, expect, test } from '@playwright/test'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { resolve, join } from 'node:path'

const root = resolve(__dirname, '../..')
const exe = process.platform === 'win32' ? 'aurora-engine.exe' : 'aurora-engine'

test('open → play → signal path → bit-perfect', async () => {
  const app = await electron.launch({
    args: [resolve(__dirname, '../out/main/index.js')],
    env: {
      ...process.env,
      AURORA_ENGINE_PATH: resolve(root, 'engine/target/debug', exe),
      AURORA_DATA_DIR: mkdtempSync(join(tmpdir(), 'aurora-e2e-')),
      AURORA_OUTPUT: 'null',
      AURORA_NULL_SPEED: '0.5',
      AURORA_TEST_OPEN_FILES: resolve(root, 'fixtures/out/flac24_96k.flac'),
    },
  })
  const win = await app.firstWindow()
  await win.getByRole('button', { name: /open files/i }).click()
  await expect(win.getByTestId('now-title')).toHaveText('flac24_96k')
  await expect(win.getByTestId('format-badge')).toHaveText('FLAC 24/96')
  await expect(win.getByRole('button', { name: 'Pause' })).toBeVisible()

  await win.getByRole('link', { name: 'Signal path' }).click()
  await expect(win.getByTestId('signal-output')).toContainText('Null Output')

  await win.getByRole('button', { name: 'Pause' }).click()
  await expect(win.getByRole('button', { name: 'Play' })).toBeVisible()

  await win.getByRole('switch', { name: 'Bit-perfect' }).click()
  await expect(win.getByRole('slider', { name: 'Volume' })).toBeDisabled()
  await win.getByRole('button', { name: 'Play' }).click()
  await expect(win.getByTestId('bitperfect-badge')).toHaveText('Bit-perfect')
  await app.close()
})
