import { useCallback, useEffect, useRef, useState } from 'react'
import type { LibraryScope } from '@protocol/LibraryScope'
import type { AuroraApi } from '@shared/api'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'

/** Fetches engine data for a view; refetches when `key`, the engine, or a watched library scope changes. */
export function useEngineQuery<T>(key: unknown[], fetcher: (api: AuroraApi) => Promise<T>, scopes: LibraryScope[] = ['tracks']) {
  const api = usePlayer((p) => p.api)
  const ready = usePlayer((p) => p.engine.state === 'ready')
  const version = useLibrary((l) => scopes.reduce((n, s) => n + l.versions[s], 0))
  const [data, setData] = useState<T | undefined>(undefined)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [nonce, setNonce] = useState(0)
  const seq = useRef(0)
  const fetchRef = useRef(fetcher)
  fetchRef.current = fetcher
  const keyStr = JSON.stringify(key)
  useEffect(() => {
    if (!api || !ready) return
    const mine = ++seq.current
    setLoading(true)
    fetchRef.current(api).then(
      (d) => { if (mine === seq.current) { setData(d); setError(null); setLoading(false) } },
      (e: unknown) => { if (mine === seq.current) { setError(e instanceof Error ? e.message : String(e)); setLoading(false) } },
    )
  }, [api, ready, version, keyStr, nonce])
  const reload = useCallback(() => setNonce((n) => n + 1), [])
  return { data, error, loading, reload }
}
