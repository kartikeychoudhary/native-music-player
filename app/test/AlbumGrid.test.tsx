// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AlbumCard, VirtualGrid } from '../src/renderer/components/AlbumGrid'
import { albumMenu } from '../src/renderer/components/trackMenu'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, renderInRouter, resetStores } from './helpers'

afterEach(cleanup)

const album = { id: 4, title: 'Blue', artist: 'Alpha', artistId: 1, year: 2001, trackCount: 3, durationMs: 1, artworkId: null, addedAt: 0, codec: null, maxSampleRate: null, maxBitDepth: null } as never
const toasts = () => useUi.getState().toasts.map((t) => t.text)

describe('VirtualGrid', () => {
  beforeEach(resetStores)

  it('lays cards out in fixed 180 px columns so a card never outgrows its virtual row', () => {
    render(<VirtualGrid label="Cards" count={3} row={(i) => ({ id: i })} render={(x) => <span>card {x.id}</span>} keyOf={(x) => x.id} />)
    const row = screen.getByRole('list', { name: 'Cards' }).firstElementChild as HTMLElement
    expect(row.getAttribute('style')).toContain('repeat(5, 180px)')
    expect(row.style.justifyContent).toBe('start')
  })
})

describe('album actions', () => {
  beforeEach(resetStores)

  it('toast an engine failure instead of leaving an unhandled rejection (card play button)', async () => {
    usePlayer.setState({ api: fakeApi({ 'library.album': () => { throw new Error('engine down') } }).api })
    await renderInRouter(<AlbumCard a={album} />)
    fireEvent.click(screen.getByRole('button', { name: 'Play Blue' }))
    await vi.waitFor(() => expect(toasts()).toContain('engine down'))
  })

  it('toast an engine failure from the album menu and queue nothing', async () => {
    const { api, calls } = fakeApi({ 'library.album': () => { throw new Error('engine down') } })
    usePlayer.setState({ api })
    albumMenu(album, vi.fn()).find((i) => i.label === 'Add to queue')!.onSelect!()
    await vi.waitFor(() => expect(toasts()).toContain('engine down'))
    expect(calls.map((c) => c[0])).not.toContain('queue.add')
  })
})
