import { describe, expect, it, vi } from 'vitest'
import { createEventRelay } from '../src/main/eventRelay'

describe('event relay', () => {
  it('holds library notices until the renderer is ready, then replays them in order', () => {
    const deliver = vi.fn((_event: string, _payload: unknown) => true)
    const relay = createEventRelay(deliver)
    relay.send('engine.error', { message: 'rebuilt' })
    relay.send('library.scanProgress', { phase: 'walking' })
    relay.send('library.changed', { scopes: ['folders'] })
    expect(deliver).not.toHaveBeenCalled()
    relay.ready()
    expect(deliver.mock.calls).toEqual([
      ['engine.error', { message: 'rebuilt' }],
      ['library.scanProgress', { phase: 'walking' }],
      ['library.changed', { scopes: ['folders'] }],
    ])
    relay.send('engine.error', { message: 'later' })
    expect(deliver).toHaveBeenLastCalledWith('engine.error', { message: 'later' })
    relay.ready()
    expect(deliver).toHaveBeenCalledTimes(4)
  })

  it('passes other events straight through, ready or not', () => {
    const deliver = vi.fn((_event: string, _payload: unknown) => true)
    const relay = createEventRelay(deliver)
    relay.send('player.state', { status: 'stopped' })
    expect(deliver).toHaveBeenCalledWith('player.state', { status: 'stopped' })
    relay.ready()
    expect(deliver).toHaveBeenCalledTimes(1)
  })

  it('keeps only the most recent held events', () => {
    const deliver = vi.fn((_event: string, _payload: unknown) => true)
    const relay = createEventRelay(deliver, 3)
    for (let i = 0; i < 5; i++) relay.send('engine.error', { i })
    relay.ready()
    expect(deliver.mock.calls.map(([, p]) => p)).toEqual([{ i: 2 }, { i: 3 }, { i: 4 }])
  })

  it('holds only the latest scan progress, so a busy scan cannot push out a notice', () => {
    const deliver = vi.fn((_event: string, _payload: unknown) => true)
    const relay = createEventRelay(deliver, 3)
    relay.send('engine.error', { message: 'rebuilt' })
    for (let i = 0; i < 10; i++) relay.send('library.scanProgress', { processed: i })
    relay.ready()
    expect(deliver.mock.calls).toEqual([['engine.error', { message: 'rebuilt' }], ['library.scanProgress', { processed: 9 }]])
  })

  it('holds again after a reset, and holds what a ready renderer failed to receive', () => {
    let up = true
    const deliver = vi.fn((_event: string, _payload: unknown) => up)
    const relay = createEventRelay(deliver)
    relay.ready()
    up = false
    relay.send('engine.error', { n: 1 })
    relay.reset()
    relay.send('engine.error', { n: 2 })
    up = true
    relay.ready()
    expect(deliver.mock.calls.slice(-2)).toEqual([['engine.error', { n: 1 }], ['engine.error', { n: 2 }]])
  })
})
