import { closestCenter, DndContext, KeyboardSensor, PointerSensor, useSensor, useSensors, type DragEndEvent } from '@dnd-kit/core'
import { SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { GripVertical } from 'lucide-react'
import { S } from '../lib/strings'
import s from './SortableList.module.css'

function Row({ id, children }: { id: number; children: (handle: React.HTMLAttributes<HTMLElement>) => React.ReactNode }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id })
  return (
    <li ref={setNodeRef} className={s.item} data-dragging={isDragging} style={{ transform: CSS.Transform.toString(transform), transition }}>
      {children({ ...attributes, ...listeners, 'aria-label': S.dragHandle, className: s.handle } as React.HTMLAttributes<HTMLElement>)}
    </li>
  )
}

/** Drag-to-reorder list (pointer, or keyboard: Space, arrows, Space). `onMove` gets array-move indices. */
export function SortableList<T>({ label, items, getId, onMove, renderRow }: {
  label: string
  items: T[]
  getId: (item: T) => number
  onMove: (from: number, to: number) => void
  renderRow: (item: T, index: number, handle: React.HTMLAttributes<HTMLElement>) => React.ReactNode
}) {
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }), useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }))
  const ids = items.map(getId)
  const onDragEnd = (e: DragEndEvent) => {
    if (!e.over || e.active.id === e.over.id) return
    const from = ids.indexOf(Number(e.active.id))
    const to = ids.indexOf(Number(e.over.id))
    if (from >= 0 && to >= 0) onMove(from, to)
  }
  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      <SortableContext items={ids} strategy={verticalListSortingStrategy}>
        <ul className={s.list} aria-label={label}>
          {items.map((item, i) => <Row key={ids[i]} id={ids[i]}>{(handle) => renderRow(item, i, handle)}</Row>)}
        </ul>
      </SortableContext>
    </DndContext>
  )
}

export function DragHandle(props: React.HTMLAttributes<HTMLElement>) {
  return <span role="button" tabIndex={0} {...props}><GripVertical size={14} aria-hidden /></span>
}

export { s as sortableStyles }
