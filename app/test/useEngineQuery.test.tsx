// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useEngineQuery } from '../src/renderer/lib/useEngineQuery'
import { useLibrary } from '../src/renderer/stores/library'
import { usePlayer } from '../src/renderer/stores/player'
import { fakeApi, resetStores } from './helpers'

afterEach(cleanup)

function Probe({ fetcher, scopes }: { fetcher: () => Promise<string>; scopes?: ('tracks' | 'playlists')[] }) {
  const q = useEngineQuery(['probe'], fetcher, scopes)
  return <div data-testid="out">{q.data ?? (q.loading ? 'loading' : 'none')}</div>
}

describe('useEngineQuery', () => {
  beforeEach(() => { resetStores(); usePlayer.setState({ api: fakeApi().api, engine: { state: 'ready' } }) })

  it('fetches, then refetches when its scope changes', async () => {
    let n = 0
    const fetcher = vi.fn(async () => `v${++n}`)
    render(<Probe fetcher={fetcher} />)
    expect(await screen.findByText('v1')).toBeTruthy()
    act(() => useLibrary.setState((s) => ({ versions: { ...s.versions, playlists: 1 } })))
    expect(fetcher).toHaveBeenCalledTimes(1)
    act(() => useLibrary.setState((s) => ({ versions: { ...s.versions, tracks: 1 } })))
    expect(await screen.findByText('v2')).toBeTruthy()
  })

  it('drops responses that arrive after a newer request', async () => {
    let resolveFirst: (v: string) => void = () => {}
    const fetcher = vi.fn().mockImplementationOnce(() => new Promise<string>((r) => { resolveFirst = r })).mockResolvedValueOnce('fresh')
    render(<Probe fetcher={fetcher} />)
    act(() => useLibrary.setState((s) => ({ versions: { ...s.versions, tracks: 1 } })))
    expect(await screen.findByText('fresh')).toBeTruthy()
    await act(async () => resolveFirst('stale'))
    expect(screen.getByTestId('out').textContent).toBe('fresh')
  })
})
