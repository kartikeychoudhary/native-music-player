import type { TrackRow } from '@protocol/TrackRow'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { formatTime, trackFormat } from '../lib/format'
import { S } from '../lib/strings'
import { useGo } from '../lib/useGo'
import { usePlayer } from '../stores/player'
import { useUi, type MenuItem } from '../stores/ui'
import { trackMenu } from './trackMenu'
import s from './TrackTable.module.css'

export type Column = 'index' | 'title' | 'artist' | 'album' | 'format' | 'duration' | 'plays' | 'added'
const WIDTHS: Record<Column, string> = { index: '40px', title: 'minmax(160px, 2fr)', artist: 'minmax(120px, 1.2fr)', album: 'minmax(120px, 1.2fr)', format: '96px', duration: '56px', plays: '52px', added: '92px' }
const ROW = 40

export interface TrackTableProps {
  label: string
  count: number
  row: (i: number) => TrackRow | undefined
  onRange?: (start: number, end: number) => void
  columns?: Column[]
  onPlay: (index: number) => void
  extraMenu?: (rows: TrackRow[]) => MenuItem[]
}

function cell(c: Column, t: TrackRow, i: number): React.ReactNode {
  switch (c) {
    case 'index': return <span className={s.num}>{t.trackNo ?? i + 1}</span>
    case 'title': return (
      <span className={`${s.cell} ${s.title}`}>
        {(t.missing || t.unplayable) && <span className={s.warn} title={t.missing ? S.missingFile : S.unplayable(t.unplayable!)} aria-label={t.missing ? S.missingFile : S.unplayable(t.unplayable!)}>⚠</span>}
        {t.title}
      </span>
    )
    case 'artist': return <span className={`${s.cell} muted`}>{t.artist || S.unknownArtist}</span>
    case 'album': return <span className={`${s.cell} muted`}>{t.album || S.unknownAlbum}</span>
    case 'format': return <span className={`${s.cell} ${s.fmt}`}>{trackFormat(t)}</span>
    case 'duration': return <span className={s.num}>{t.durationMs != null ? formatTime(t.durationMs) : ''}</span>
    case 'plays': return <span className={s.num}>{t.playCount || ''}</span>
    case 'added': return <span className={`${s.cell} ${s.fmt}`}>{new Date(t.addedAt * 1000).toLocaleDateString()}</span>
  }
}

export function TrackTable({ label, count, row, onRange, columns = ['index', 'title', 'artist', 'album', 'format', 'duration'], onPlay, extraMenu }: TrackTableProps) {
  const ref = useRef<HTMLDivElement>(null)
  const [scrollEl, setScrollEl] = useState<HTMLElement | null>(null)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [focus, setFocus] = useState(0)
  const anchor = useRef(0)
  const go = useGo()
  const playingId = usePlayer((p) => p.state.track?.trackId ?? null)
  useLayoutEffect(() => { setScrollEl((ref.current?.closest('main') as HTMLElement | null) ?? ref.current) }, [])
  const v = useVirtualizer({ count, getScrollElement: () => scrollEl, estimateSize: () => ROW, overscan: 12, scrollMargin: ref.current?.offsetTop ?? 0 })
  const items = v.getVirtualItems()
  const first = items[0]?.index ?? 0
  const last = items.at(-1)?.index ?? 0
  useEffect(() => { if (count) onRange?.(first, last) }, [first, last, count, onRange])
  useEffect(() => { setSelected(new Set()); setFocus(0) }, [count])

  const select = (i: number, e: { shiftKey?: boolean; ctrlKey?: boolean; metaKey?: boolean }) => {
    setFocus(i)
    if (e.shiftKey) {
      const [a, b] = [Math.min(anchor.current, i), Math.max(anchor.current, i)]
      setSelected(new Set(Array.from({ length: b - a + 1 }, (_, k) => a + k)))
      return
    }
    anchor.current = i
    if (e.ctrlKey || e.metaKey) setSelected((prev) => { const n = new Set(prev); if (n.has(i)) n.delete(i); else n.add(i); return n })
    else setSelected(new Set([i]))
  }

  const openMenu = (i: number, x: number, y: number) => {
    const indices = selected.has(i) ? [...selected].sort((a, b) => a - b) : [i]
    if (!selected.has(i)) select(i, {})
    const rows = indices.map(row).filter((r): r is TrackRow => !!r)
    if (rows.length) useUi.getState().openMenu(x, y, trackMenu(rows, go, extraMenu?.(rows)))
  }

  const onKey = (e: React.KeyboardEvent) => {
    if (!count) return
    const move = (to: number) => { e.preventDefault(); const n = Math.max(0, Math.min(count - 1, to)); select(n, { shiftKey: e.shiftKey }); v.scrollToIndex(n) }
    if (e.key === 'ArrowDown') move(focus + 1)
    else if (e.key === 'ArrowUp') move(focus - 1)
    else if (e.key === 'Home') move(0)
    else if (e.key === 'End') move(count - 1)
    else if (e.key === 'Enter') { e.preventDefault(); onPlay(focus) }
    else if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
      e.preventDefault()
      const r = (ref.current?.querySelector(`[data-index="${focus}"]`) as HTMLElement | null)?.getBoundingClientRect()
      openMenu(focus, r ? r.left + 40 : 100, r ? r.bottom : 100)
    }
  }

  const cols = columns.map((c) => WIDTHS[c]).join(' ')
  return (
    <div ref={ref} className={s.grid} role="grid" aria-label={label} aria-rowcount={count + 1} aria-multiselectable tabIndex={0} onKeyDown={onKey} style={{ ['--cols' as string]: cols }}>
      <div className={s.head} role="row" aria-rowindex={1}>
        {columns.map((c) => <span key={c} role="columnheader" className={c === 'duration' || c === 'plays' || c === 'index' ? s.num : undefined}>{S.col[c]}</span>)}
      </div>
      <div className={s.body} style={{ height: v.getTotalSize() }}>
        {items.map((it) => {
          const t = row(it.index)
          const top = it.start - v.options.scrollMargin
          if (!t) return <div key={it.key} className={s.row} style={{ transform: `translateY(${top}px)` }} role="row" aria-rowindex={it.index + 2} aria-busy="true" />
          return (
            <div
              key={it.key} data-index={it.index} role="row" aria-rowindex={it.index + 2} aria-selected={selected.has(it.index)}
              data-playing={t.id === playingId} data-dim={t.missing || !!t.unplayable}
              className={`${s.row} ${focus === it.index ? s.focus : ''}`} style={{ transform: `translateY(${top}px)` }}
              onClick={(e) => select(it.index, e)} onDoubleClick={() => onPlay(it.index)}
              onContextMenu={(e) => { e.preventDefault(); openMenu(it.index, e.clientX, e.clientY) }}
            >
              {columns.map((c) => <span key={c} role="gridcell" className={s.cell}>{cell(c, t, it.index)}</span>)}
            </div>
          )
        })}
      </div>
    </div>
  )
}
