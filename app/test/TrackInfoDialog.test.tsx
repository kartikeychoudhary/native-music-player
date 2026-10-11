// @vitest-environment jsdom
import { cleanup, fireEvent, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it } from 'vitest'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, renderAt, track } from './helpers'

afterEach(cleanup)

describe('track info dialog', () => {
  it('shows the full technical metadata and closes with Escape', async () => {
    const detail = {
      track: track(1, 'Dawn', { album: 'Light', path: '/m/Alpha/Dawn.flac', playCount: 3 }),
      artists: [{ id: 1, name: 'Alpha' }, { id: 2, name: 'Beta' }], genres: [{ id: 3, name: 'Ambient' }], composer: 'C. Writer',
      trackTotal: 9, discTotal: 1, replayGain: { trackGain: -6.5, trackPeak: 0.98, albumGain: null, albumPeak: null },
      musicbrainz: { recording: 'rec-1', release: null, artist: null }, hasLyrics: true, sizeBytes: 31_457_280, mtime: 1_700_000_000_000,
      gapless: { delay: 2112, padding: 576 },
    }
    await renderAt('/', fakeApi({ 'library.track': () => detail }).api)
    act(() => useUi.getState().showTrackInfo(1))
    const dialog = await screen.findByRole('dialog', { name: 'Dawn' })
    for (const text of ['Alpha, Beta', 'Ambient', 'C. Writer', '1 / 9', 'FLAC 16/44.1', '44.1 kHz', '16-bit', '2112 / 576', '-6.50 dB', 'rec-1', '30.0 MB', '/m/Alpha/Dawn.flac'])
      expect(dialog.textContent).toContain(text)
    fireEvent.keyDown(dialog, { key: 'Escape' })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(useUi.getState().trackInfo).toBeNull()
  })
})
