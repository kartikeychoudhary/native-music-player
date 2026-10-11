import { describe, expect, it, vi } from 'vitest'
import { createArtHandler, parseArtUrl } from '../src/main/artProtocol'

describe('aurora-art protocol', () => {
  it('parses art URLs strictly', () => {
    expect(parseArtUrl('aurora-art://art/12/300')).toEqual({ id: 12, size: 300 })
    expect(parseArtUrl('aurora-art://art/12/300?x=1')).toEqual({ id: 12, size: 300 })
    for (const bad of ['aurora-art://art/12', 'aurora-art://art/x/300', 'aurora-art://other/1/300', 'aurora-art://art/-1/64', 'aurora-art://art/1/0', 'https://art/1/64', 'nonsense'])
      expect(parseArtUrl(bad)).toBeNull()
  })

  it('serves the cached file the engine names', async () => {
    const call = vi.fn(async () => ({ path: '/data/artwork/ab/abc-300.webp', palette: null }))
    const fetchFile = vi.fn(async () => new Response('img', { headers: { 'content-type': 'image/webp' } }))
    const res = await createArtHandler(call, fetchFile)(new Request('aurora-art://art/7/200'))
    expect(call).toHaveBeenCalledWith('library.artwork', { id: 7, size: 200 })
    expect(fetchFile).toHaveBeenCalledWith('/data/artwork/ab/abc-300.webp')
    expect(res.status).toBe(200)
    expect(res.headers.get('cache-control')).toBe('max-age=3600')
    expect(await res.text()).toBe('img')
  })

  it('answers 404 for bad URLs and engine errors', async () => {
    const call = vi.fn(async () => { throw new Error('artwork 9 not found') })
    const handler = createArtHandler(call, vi.fn())
    expect((await handler(new Request('aurora-art://art/9/64'))).status).toBe(404)
    expect((await handler(new Request('aurora-art://art/nope/64'))).status).toBe(404)
    expect(call).toHaveBeenCalledTimes(1)
  })
})
