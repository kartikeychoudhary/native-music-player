import { expect, test } from '@playwright/test'
import { resolve } from 'node:path'
import { launchAurora, root, tempDir } from './fixture'

test('open → play → signal path → bit-perfect', async () => {
  const { app, win } = await launchAurora({ dataDir: tempDir('aurora-data-'), openFiles: resolve(root, 'fixtures/out/flac24_96k.flac') })
  try {
    await win.getByRole('button', { name: /open files/i }).click()
    await expect(win.getByTestId('now-title')).toHaveText('flac24_96k')
    await expect(win.getByTestId('format-badge')).toHaveText('FLAC 24/96')
    await expect(win.getByRole('button', { name: 'Pause', exact: true })).toBeVisible()

    await win.getByRole('link', { name: 'Signal path' }).click()
    await expect(win.getByTestId('signal-output')).toContainText('Null Output')

    await win.getByRole('button', { name: 'Pause', exact: true }).click()
    await expect(win.getByRole('button', { name: 'Play', exact: true })).toBeVisible()

    await win.getByRole('switch', { name: 'Bit-perfect' }).click()
    await expect(win.getByRole('slider', { name: 'Volume' })).toBeDisabled()
    await win.getByRole('button', { name: 'Play', exact: true }).click()
    await expect(win.getByTestId('bitperfect-badge')).toHaveText('Bit-perfect')
  } finally {
    await app.close()
  }
})
