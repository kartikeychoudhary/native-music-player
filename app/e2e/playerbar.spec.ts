import { expect, test } from '@playwright/test'
import { resolve } from 'node:path'
import { launchAurora, root, tempDir } from './fixture'

const files = [resolve(root, 'fixtures/out/gap_a.flac'), resolve(root, 'fixtures/out/gap_b.flac')].join(process.platform === 'win32' ? ';' : ':')

test('player bar controls never overlap, from the minimum window width up', async () => {
  const { app, win } = await launchAurora({ dataDir: tempDir('aurora-data-'), openFiles: files, speed: 0.05 })
  try {
    await win.getByRole('button', { name: /open files/i }).click()
    await expect(win.getByRole('button', { name: 'Pause' })).toBeVisible()
    for (const width of [940, 1100, 1280, 1600]) {
      // Viewport emulation gives an exact CSS width; resizing the window depends on frame and display scaling.
      await win.setViewportSize({ width, height: 800 })
      await expect.poll(() => win.evaluate(() => innerWidth)).toBe(width)
      const bar = await win.evaluate(() => {
        const box = (el: Element) => el.getBoundingClientRect()
        const [left, center, right] = [...document.querySelector('footer')!.children].map(box)
        const controls = [...document.querySelector('footer > div:last-child')!.children].filter((c) => box(c).width > 0).map(box)
        const seek = box(document.querySelector('input[aria-label="Seek"]')!)
        return { leftRight: left.right, centerLeft: center.left, centerRight: center.right, rightLeft: right.left, firstControl: Math.min(...controls.map((c) => c.left)), lastControl: Math.max(...controls.map((c) => c.right)), seekWidth: seek.width, vw: innerWidth }
      })
      expect(bar.firstControl, `${width}px: right-hand controls start inside their column`).toBeGreaterThanOrEqual(bar.rightLeft - 0.5)
      expect(bar.rightLeft, `${width}px: right column starts after the transport`).toBeGreaterThanOrEqual(bar.centerRight)
      expect(bar.centerLeft, `${width}px: transport starts after the track info`).toBeGreaterThanOrEqual(bar.leftRight)
      expect(bar.lastControl, `${width}px: nothing runs off the window`).toBeLessThanOrEqual(bar.vw)
      expect(bar.seekWidth, `${width}px: the seek bar stays usable`).toBeGreaterThan(120)
    }
  } finally {
    await app.close()
  }
})

test('a seek drag released past the end of the slider still seeks and the bar keeps moving', async () => {
  const { app, win } = await launchAurora({ dataDir: tempDir('aurora-data-'), openFiles: files, speed: 0.05 })
  try {
    await win.setViewportSize({ width: 1600, height: 800 })
    await win.getByRole('button', { name: /open files/i }).click()
    await expect(win.getByRole('button', { name: 'Pause' })).toBeVisible()
    const slider = win.getByRole('slider', { name: 'Seek' })
    const b = (await slider.boundingBox())!
    await win.mouse.move(b.x + b.width * 0.3, b.y + b.height / 2)
    await win.mouse.down()
    await win.mouse.move(b.x + b.width + 80, b.y + b.height / 2, { steps: 5 })
    await win.mouse.up()
    // Released past the end = a seek to the very end, so playback moves on to the next track.
    const engine = () => win.evaluate(() => (window as unknown as { aurora: { call: (m: string, p: unknown) => Promise<{ positionMs: number; queueIndex: number | null }> } }).aurora.call('player.state', {}))
    await expect.poll(async () => (await engine()).queueIndex, { message: 'the engine received the seek' }).toBe(1)
    const shown = () => slider.evaluate((el: HTMLInputElement) => Number(el.value))
    await expect.poll(async () => Math.abs((await shown()) - (await engine()).positionMs), { message: 'the bar follows playback instead of freezing on the dragged value' }).toBeLessThan(400)
    expect(await shown()).toBeLessThan(2000)
  } finally {
    await app.close()
  }
})
