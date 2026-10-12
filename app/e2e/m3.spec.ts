import { expect, test } from '@playwright/test'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { launchAurora, musicFolder, tempDir } from './fixture'

test('lyrics, visualizer, EQ in the signal path, and the mini-player', async () => {
  const musicDir = musicFolder(['flac16_44k.flac', 'short_a.flac'])
  writeFileSync(join(musicDir, 'flac16_44k.lrc'), '[00:00.00]Aurora sings\n[00:01.00]Second line\n')
  const { app, win } = await launchAurora({ dataDir: tempDir('aurora-data-'), musicDir, speed: 0.2 })
  try {
    await win.getByRole('button', { name: 'Add a music folder' }).click()
    await expect(win.getByRole('heading', { name: 'Recently added' })).toBeVisible({ timeout: 30_000 })
    await win.getByRole('link', { name: 'Albums' }).click()
    await win.getByRole('list', { name: 'Albums' }).getByText('Unknown album').click()
    await win.getByRole('button', { name: 'Play album' }).click()
    await expect(win.getByTestId('now-title')).toHaveText('flac16_44k')

    // Lyrics (Ctrl/Cmd+L) from the sidecar .lrc, with the visualizer receiving frames.
    await win.keyboard.press(process.platform === 'darwin' ? 'Meta+L' : 'Control+L')
    await expect(win.getByRole('tab', { name: 'Lyrics', selected: true })).toBeVisible()
    await expect(win.getByText('Aurora sings')).toBeVisible()
    await expect.poll(async () => Number(await win.getByRole('img', { name: 'Spectrum' }).getAttribute('data-frames')), { timeout: 10_000 }).toBeGreaterThan(3)

    // EQ: enable and pick a preset in Settings; the signal path names it.
    await win.getByRole('link', { name: 'Settings' }).click()
    const eq = win.getByRole('group', { name: 'Equalizer' })
    await eq.getByRole('switch', { name: 'Equalizer' }).click()
    await eq.getByRole('combobox', { name: 'EQ preset' }).selectOption('Bass')
    await win.getByRole('link', { name: 'Signal path' }).click()
    await expect(win.getByText('Bass')).toBeVisible()

    // Mini-player (Ctrl/Cmd+M): a second, always-on-top window showing the track.
    const [mini] = await Promise.all([app.waitForEvent('window'), win.keyboard.press(process.platform === 'darwin' ? 'Meta+M' : 'Control+M')])
    await expect(mini.getByText('flac16_44k')).toBeVisible()
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().find((w) => w.getTitle().includes('mini'))?.isAlwaysOnTop())).toBe(true)
    await mini.getByRole('button', { name: 'Pause' }).click()
    await expect(win.getByRole('button', { name: 'Play', exact: true })).toBeVisible()
    await mini.getByRole('button', { name: 'Close mini-player' }).click()
    await expect.poll(() => app.windows().length).toBe(1)
  } finally {
    await app.close()
  }
})
