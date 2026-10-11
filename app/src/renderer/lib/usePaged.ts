import { useCallback, useEffect, useRef, useState } from 'react'
import type { LibraryScope } from '@protocol/LibraryScope'
import type { Page } from '@protocol/Page'
import type { AuroraApi } from '@shared/api'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'

export const PAGE = 200

/** Sparse, page-at-a-time loading for virtualized lists over `{ items, total }` queries. */
export function usePaged<T>(key: unknown[], fetchPage: (api: AuroraApi, offset: number, limit: number) => Promise<Page<T>>, scopes: LibraryScope[] = ['tracks']) {
  const api = usePlayer((p) => p.api)
  const ready = usePlayer((p) => p.engine.state === 'ready')
  const version = useLibrary((l) => scopes.reduce((n, s) => n + l.versions[s], 0))
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [, bump] = useState(0)
  const rows = useRef(new Map<number, T>())
  const requested = useRef(new Set<number>())
  const generation = useRef(0)
  const fetchRef = useRef(fetchPage)
  fetchRef.current = fetchPage
  const keyStr = JSON.stringify(key)

  const load = useCallback((page: number) => {
    if (!api || requested.current.has(page)) return Promise.resolve()
    requested.current.add(page)
    const gen = generation.current
    return fetchRef.current(api, page * PAGE, PAGE).then((p) => {
      if (gen !== generation.current) return
      p.items.forEach((item, i) => rows.current.set(page * PAGE + i, item))
      setTotal(p.total)
      setLoading(false)
      bump((n) => n + 1)
    }, () => { requested.current.delete(page); setLoading(false) })
  }, [api])

  useEffect(() => {
    if (!api || !ready) return
    generation.current++
    rows.current = new Map()
    requested.current = new Set()
    setLoading(true)
    void load(0)
  }, [api, ready, version, keyStr, load])

  const onRange = useCallback((start: number, end: number) => {
    const pages = []
    for (let p = Math.floor(start / PAGE); p <= Math.floor(Math.max(start, end) / PAGE); p++) pages.push(load(p))
    return Promise.all(pages).then(() => {})
  }, [load])

  return { total, row: (i: number) => rows.current.get(i), onRange, loading }
}
