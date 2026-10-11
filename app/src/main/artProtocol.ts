import type { ArtworkInfo } from '@protocol/ArtworkInfo'

export const ART_SCHEME = 'aurora-art'

/** `aurora-art://art/<id>/<size>` → ids and sizes are positive integers. */
export function parseArtUrl(url: string): { id: number; size: number } | null {
  let u: URL
  try { u = new URL(url) } catch { return null }
  if (u.protocol !== `${ART_SCHEME}:` || u.hostname !== 'art') return null
  const m = /^\/(\d+)\/(\d+)$/.exec(u.pathname)
  if (!m) return null
  const id = Number(m[1]), size = Number(m[2])
  return id > 0 && size > 0 && size <= 4096 ? { id, size } : null
}

type Call = (method: 'library.artwork', params: { id: number; size: number }) => Promise<ArtworkInfo>

/** Resolves artwork through the engine (which owns the cache) and streams the file. */
export function createArtHandler(call: Call, fetchFile: (path: string) => Promise<Response>) {
  return async (req: Request): Promise<Response> => {
    const parsed = parseArtUrl(req.url)
    if (!parsed) return new Response(null, { status: 404 })
    try {
      const info = await call('library.artwork', parsed)
      const file = await fetchFile(info.path)
      const headers = new Headers(file.headers)
      headers.set('cache-control', 'max-age=3600')
      return new Response(file.body, { status: file.status, headers })
    } catch {
      return new Response(null, { status: 404 })
    }
  }
}
