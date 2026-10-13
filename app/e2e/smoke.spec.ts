import { expect, test } from '@playwright/test'
import { resolve } from 'node:path'
import { launchAurora, root, tempDir } from './fixture'

test('open → play → signal path → bit-perfect', async () => {
  const { app, win } = await launchAurora({ dataDir: tempDir('aurora-data-'), openFiles: resolve(root, 'fixtures/out/flac24_96k.flac') })
  try {
    // Wide enough that the bar shows its format badges (narrow bars hide them; CI windows are small).
    await win.setViewportSize({ width: 1600, height: 900 })
    await win.getByRole('button', { name: /open files/i }).click()
    await expect(win.getByTestId('now-title')).toHaveText('flac24_96k')
    await expect(win.getByTestId('format-badge')).toHaveText('FLAC 24/96')
    // The bar's right-hand controls line up: icons share one vertical centre, boxed controls one height.
    const bar = await win.locator('footer > div:last-child').evaluate((right) => {
      const box = (el: Element) => el.getBoundingClientRect()
      const mid = (el: Element) => box(el).top + box(el).height / 2
      const row = mid(right)
      return {
        offCentre: [...right.querySelectorAll('svg, [data-testid=format-badge]')].map((el) => Math.abs(mid(el) - row)),
        heights: [right.querySelector('[data-testid=format-badge]')!, right.querySelector('[role=switch]')!, right.querySelector('select')!].map((el) => Math.round(box(el).height)),
      }
    })
    expect(Math.max(...bar.offCentre)).toBeLessThanOrEqual(1)
    expect(new Set(bar.heights).size).toBe(1)
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
