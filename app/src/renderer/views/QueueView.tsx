import { ArrowDown, ArrowUp, ListX, X } from 'lucide-react'
import { DragHandle, SortableList, sortableStyles as ss } from '../components/SortableList'
import { formatTime } from '../lib/format'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export function QueueView() {
  const queue = usePlayer((p) => p.queue)
  const album = usePlayer((p) => (p.state.status !== 'stopped' ? p.state.track?.album : null))
  const api = usePlayer((p) => p.api)
  const send = (p: Promise<unknown> | undefined) => void p?.catch((e: unknown) => useUi.getState().toast('error', e instanceof Error ? e.message : String(e)))
  const move = (from: number, to: number) => { if (to >= 0 && to < queue.items.length && from !== to) send(api?.call('queue.move', { from, to })) }
  return (
    <div className={v.view}>
      <div className={v.toolbar}>
        <div>
          <h1 className={v.title}>{S.queueTitle}</h1>
          {album && <div className={v.meta}>{S.playingFrom(album)}</div>}
        </div>
        <button className={v.ghost} disabled={!queue.items.length} onClick={() => send(api?.call('queue.clear', {}))}><ListX size={16} /> {S.clearQueue}</button>
      </div>
      {queue.items.length === 0 ? <p className="muted">{S.queueEmpty}</p> : (
        <SortableList label={S.queueTitle} items={queue.items} getId={(i) => i.id} onMove={move}
          renderRow={(it, index, handle) => (
            <>
              <DragHandle {...handle} />
              <div className={ss.main} data-current={index === queue.current}
                onDoubleClick={() => void usePlayer.getState().playItem(it.id)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  useUi.getState().openMenu(e.clientX, e.clientY, [
                    { label: S.moveUp, icon: ArrowUp, disabled: index === 0, onSelect: () => move(index, index - 1) },
                    { label: S.moveDown, icon: ArrowDown, disabled: index === queue.items.length - 1, onSelect: () => move(index, index + 1) },
                    { label: S.removeFromQueue, icon: X, onSelect: () => send(api?.call('queue.remove', { itemIds: [it.id] })) },
                  ])
                }}>
                <span>{it.title}</span>
                <span className="muted">{it.artist ?? ''}</span>
                <span className="mono muted">{it.durationMs != null ? formatTime(it.durationMs) : ''}</span>
                <button aria-label={S.removeFromQueue} className="muted" onClick={() => send(api?.call('queue.remove', { itemIds: [it.id] }))}><X size={14} /></button>
              </div>
            </>
          )} />
      )}
    </div>
  )
}
