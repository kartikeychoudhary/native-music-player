// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { usePaged } from '../src/renderer/lib/usePaged'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, resetStores } from './helpers'

afterEach(cleanup)

let hook: ReturnType<typeof usePaged<number>>
function Probe({ fetchPage }: { fetchPage: (api: unknown, o: number, l: number) => Promise<{ items: number[]; total: number }> }) {
  hook = usePaged<number>(['n'], fetchPage as never)
  return <div data-testid="total">{hook.total}</div>
}

describe('usePaged', () => {
  beforeEach(() => { resetStores(); usePlayer.setState({ api: fakeApi().api, engine: { state: 'ready' } }) })

  it('loads the first page, then only missing pages', async () => {
    const fetchPage = vi.fn(async (_a: unknown, offset: number, limit: number) => ({ items: Array.from({ length: Math.min(limit, 450 - offset) }, (_, i) => offset + i), total: 450 }))
    render(<Probe fetchPage={fetchPage} />)
    expect(await screen.findByText('450')).toBeTruthy()
    expect(hook.row(5)).toBe(5)
    expect(hook.row(300)).toBeUndefined()
    await act(async () => { await hook.onRange(250, 420) })
    expect(fetchPage).toHaveBeenCalledWith(expect.anything(), 200, 200)
    expect(fetchPage).toHaveBeenCalledWith(expect.anything(), 400, 200)
    expect(hook.row(449)).toBe(449)
    await act(async () => { await hook.onRange(0, 420) })
    expect(fetchPage).toHaveBeenCalledTimes(3)
  })

  it('re-requests the scrolled range after a library change and keeps old rows visible meanwhile', async () => {
    const page = (offset: number, limit: number) => ({ items: Array.from({ length: Math.min(limit, 450 - offset) }, (_, i) => offset + i), total: 450 })
    const fetchPage = vi.fn(async (_a: unknown, offset: number, limit: number) => page(offset, limit))
    render(<Probe fetchPage={fetchPage} />)
    await screen.findByText('450')
    await act(async () => { await hook.onRange(250, 420) })
    expect(hook.row(449)).toBe(449)

    fetchPage.mockClear()
    const releases: (() => void)[] = []
    fetchPage.mockImplementation((_a: unknown, offset: number, limit: number) => new Promise((resolve) => { releases.push(() => resolve(page(offset, limit))) }))
    act(() => useLibrary.setState({ versions: { ...useLibrary.getState().versions, tracks: 1 } }))

    expect(fetchPage.mock.calls.map((c) => c[1]).sort()).toEqual([0, 200, 400])
    expect(hook.row(449)).toBe(449)
    await act(async () => { releases.forEach((r) => r()) })
    expect(hook.row(449)).toBe(449)
    expect(hook.row(300)).toBe(300)
  })
})
