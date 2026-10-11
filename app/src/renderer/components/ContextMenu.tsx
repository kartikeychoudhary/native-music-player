import { ChevronRight } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useUi, type MenuItem } from '../stores/ui'
import s from './ui.module.css'

function enabledIndex(items: MenuItem[], from: number, step: 1 | -1): number {
  for (let i = 1; i <= items.length; i++) {
    const j = (from + step * i + items.length * 2) % items.length
    if (!items[j].disabled && !items[j].separator) return j
  }
  return from
}

/** One menu level. `onClose` closes this level (Escape/ArrowLeft); `closeAll` closes the whole menu after a choice. */
function MenuList({ items, x, y, onClose, closeAll, depth }: { items: MenuItem[]; x: number; y: number; onClose: () => void; closeAll: () => void; depth: number }) {
  const ref = useRef<HTMLDivElement>(null)
  const [active, setActive] = useState(() => enabledIndex(items, -1, 1))
  const [sub, setSub] = useState<number | null>(null)
  const [pos, setPos] = useState({ x, y })
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const r = el.getBoundingClientRect()
    setPos({ x: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)), y: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)) })
    el.focus()
  }, [x, y])
  const choose = (i: number) => {
    const it = items[i]
    if (!it || it.disabled || it.separator) return
    if (it.items) { setSub(i); return }
    closeAll()
    it.onSelect?.()
  }
  const onKey = (e: React.KeyboardEvent) => {
    e.stopPropagation()
    if (e.key === 'ArrowDown') { e.preventDefault(); setActive((a) => enabledIndex(items, a, 1)) }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive((a) => enabledIndex(items, a, -1)) }
    else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); choose(active) }
    else if (e.key === 'ArrowRight' && items[active]?.items) { e.preventDefault(); setSub(active) }
    else if (e.key === 'ArrowLeft' && depth > 0) { e.preventDefault(); onClose() }
    else if (e.key === 'Escape') { e.preventDefault(); onClose() }
  }
  const subRect = sub != null ? ref.current?.children[sub]?.getBoundingClientRect() : undefined
  return (
    <>
      <div ref={ref} role="menu" tabIndex={-1} className={s.menu} style={{ left: pos.x, top: pos.y }} onKeyDown={onKey} onContextMenu={(e) => e.preventDefault()}>
        {items.map((it, i) => it.separator ? <div key={i} className={s.separator} role="separator" /> : (
          <button
            key={i} role="menuitem" type="button" tabIndex={-1} className={s.menuItem} data-active={i === active}
            aria-disabled={it.disabled || undefined} aria-haspopup={it.items ? 'menu' : undefined} aria-expanded={it.items ? sub === i : undefined}
            onMouseEnter={() => { setActive(i); setSub(it.items ? i : null) }} onClick={() => choose(i)}
          >
            {it.icon && <it.icon size={14} aria-hidden />}
            <span>{it.label}</span>
            {it.items && <ChevronRight size={14} className={s.chevron} aria-hidden />}
          </button>
        ))}
      </div>
      {sub != null && items[sub]?.items && (
        <MenuList items={items[sub].items!} x={(subRect?.right ?? pos.x + 200) - 2} y={subRect?.top ?? pos.y} depth={depth + 1}
          closeAll={closeAll} onClose={() => { setSub(null); ref.current?.focus() }} />
      )}
    </>
  )
}

/** The single app-wide context menu, driven by `useUi().openMenu`. */
export function ContextMenu() {
  const menu = useUi((u) => u.menu)
  const close = useUi((u) => u.closeMenu)
  useEffect(() => {
    if (!menu) return
    const onDown = (e: MouseEvent) => { if (!(e.target as Element).closest?.('[role=menu]')) close() }
    window.addEventListener('mousedown', onDown)
    window.addEventListener('blur', close)
    return () => { window.removeEventListener('mousedown', onDown); window.removeEventListener('blur', close) }
  }, [menu, close])
  if (!menu) return null
  return <MenuList key={`${menu.x},${menu.y},${menu.items.length}`} items={menu.items} x={menu.x} y={menu.y} depth={0} onClose={close} closeAll={close} />
}
