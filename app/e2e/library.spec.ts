import { expect, test } from '@playwright/test'
import { launchAurora, musicFolder, tempDir } from './fixture'

test('add folder → scan → albums → play → search → play next → queue → restart restores', async () => {
  const dataDir = tempDir('aurora-data-')
  const musicDir = musicFolder(['flac16_44k.flac', 'short_a.flac', 'short_b.flac'])
  // Slow null output: a 2 s track lasts 20 s, so the queue is still on track 1 when we inspect it.
  let { app, win } = await launchAurora({ dataDir, musicDir, speed: 0.1 })
  try {
    await win.getByRole('button', { name: 'Add a music folder' }).click()
    await expect(win.getByRole('heading', { name: 'Recently added' })).toBeVisible({ timeout: 30_000 })

    await win.getByRole('link', { name: 'Albums', exact: true }).click()
    await win.getByRole('list', { name: 'Albums' }).getByText('Unknown album').click()
    await expect(win.getByRole('heading', { name: 'Unknown album' })).toBeVisible()
    const tracks = win.getByRole('grid', { name: 'Unknown album' })
    await expect(tracks.getByRole('row')).toHaveCount(4) // header + 3 tracks

    await win.getByRole('button', { name: 'Play album' }).click()
    await expect(win.getByTestId('now-title')).toHaveText('flac16_44k')
    await expect(win.getByRole('button', { name: 'Pause', exact: true })).toBeVisible()

    await win.keyboard.press(process.platform === 'darwin' ? 'Meta+F' : 'Control+F')
    const search = win.getByRole('searchbox', { name: 'Search' })
    await expect(search).toBeFocused()
    await search.fill('short')
    const results = win.getByRole('grid', { name: 'Tracks' })
    await expect(results.getByText('short_b')).toBeVisible()
    await results.getByText('short_b').click({ button: 'right' })
    await win.getByRole('menuitem', { name: 'Play next' }).click()
    await expect(win.getByRole('status').filter({ hasText: 'Playing next' })).toBeVisible()

    await win.getByRole('button', { name: 'Queue', exact: true }).click()
    const queue = win.getByRole('list', { name: 'Queue' })
    await expect(queue.getByRole('listitem')).toHaveCount(4)
    await expect(queue.getByRole('listitem').nth(1)).toContainText('short_b')

    await win.getByRole('button', { name: 'Pause', exact: true }).click()
    await expect(win.getByRole('button', { name: 'Play', exact: true })).toBeVisible()
  } finally {
    await app.close()
  }

  // Relaunch on the same data: the engine restores the session paused (spec §5.2).
  ;({ app, win } = await launchAurora({ dataDir, speed: 0.1 }))
  try {
    await expect(win.getByTestId('now-title')).toHaveText('flac16_44k', { timeout: 20_000 })
    await expect(win.getByRole('button', { name: 'Play', exact: true })).toBeVisible()
    // The app reopens on the last view (/queue); the sidebar link always lands there, unlike the bar's toggle.
    await win.getByRole('link', { name: 'Queue', exact: true }).click()
    await expect(win.getByRole('list', { name: 'Queue' }).getByRole('listitem')).toHaveCount(4)
    await win.getByRole('link', { name: 'Home', exact: true }).click()
    await expect(win.getByRole('heading', { name: 'Recently added' })).toBeVisible()
  } finally {
    await app.close()
  }
})
