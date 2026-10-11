import type { AlbumRow } from '@protocol/AlbumRow'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Play } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { albumFormat } from '../lib/format'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { useUi } from '../stores/ui'
import { Artwork } from './Artwork'
import { albumMenu, playAlbum } from './trackMenu'
import s from './AlbumGrid.module.css'

const CARD = 180
const GAP = 20
const ROW_H = CARD + 76

export function AlbumCard({ a }: { a: AlbumRow }) {
  const go = useGo()
  return (
    <div className={s.card} role="listitem" onClick={() => go(`/albums/${a.id}`)}
      onContextMenu={(e) => { e.preventDefault(); useUi.getState().openMenu(e.clientX, e.clientY, albumMenu(a, go)) }}>
      <div className={s.cover}>
        <Artwork id={a.artworkId} size={300} px={CARD} alt="" />
        <button className={s.play} aria-label={`${S.play} ${a.title || S.unknownAlbum}`} onClick={(e) => { e.stopPropagation(); void playAlbum(a.id) }}><Play size={16} fill="currentColor" /></button>
      </div>
      <a href={`#/albums/${a.id}`} className={s.title} onClick={(e) => { e.preventDefault(); go(`/albums/${a.id}`) }} style={{ color: 'inherit', textDecoration: 'none' }}>{a.title || S.unknownAlbum}</a>
      <div className={s.sub}>{a.artist || S.unknownArtist}{a.year ? ` · ${a.year}` : ''}</div>
      <div className={s.fmt}>{albumFormat(a)}</div>
    </div>
  )
}

export interface VirtualGridProps<T> {
  label: string
  count: number
  row: (i: number) => T | undefined
  onRange?: (start: number, end: number) => void | Promise<void>
  render: (item: T) => React.ReactNode
  keyOf: (item: T) => React.Key
  /** Card height in px, excluding the gap. */
  height?: number
}

/** Responsive rows of 180 px cards, virtualized inside the nearest scrolling `<main>`. */
export function VirtualGrid<T>({ label, count, row, onRange, render, keyOf, height = ROW_H }: VirtualGridProps<T>) {
  const ref = useRef<HTMLDivElement>(null)
  const [scrollEl, setScrollEl] = useState<HTMLElement | null>(null)
  const [width, setWidth] = useState(1000)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    setScrollEl((el.closest('main') as HTMLElement | null) ?? el)
    setWidth(el.getBoundingClientRect().width)
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const cols = Math.max(1, Math.floor((width + GAP) / (CARD + GAP)))
  const rowCount = Math.ceil(count / cols)
  const v = useVirtualizer({ count: rowCount, getScrollElement: () => scrollEl, estimateSize: () => height + GAP, overscan: 3, scrollMargin: ref.current?.offsetTop ?? 0 })
  const vis = v.getVirtualItems()
  const first = (vis[0]?.index ?? 0) * cols
  const last = Math.min(count - 1, ((vis.at(-1)?.index ?? 0) + 1) * cols - 1)
  useEffect(() => { if (count) onRange?.(first, last) }, [first, last, count, onRange])
  return (
    <div ref={ref} className={s.grid} role="list" aria-label={label} style={{ height: v.getTotalSize() }}>
      {vis.map((r) => (
        <div key={r.key} className={s.row} style={{ transform: `translateY(${r.start - v.options.scrollMargin}px)`, gridTemplateColumns: `repeat(${cols}, ${CARD}px)`, justifyContent: 'start' }}>
          {Array.from({ length: cols }, (_, c) => {
            const i = r.index * cols + c
            if (i >= count) return null
            const item = row(i)
            return item ? <div key={keyOf(item)}>{render(item)}</div> : <div key={`p${i}`} className={s.card} aria-busy="true" />
          })}
        </div>
      ))}
    </div>
  )
}

export function AlbumGrid(props: { label: string; count: number; row: (i: number) => AlbumRow | undefined; onRange?: (start: number, end: number) => void }) {
  return <VirtualGrid {...props} render={(a) => <AlbumCard a={a} />} keyOf={(a) => a.id} />
}
