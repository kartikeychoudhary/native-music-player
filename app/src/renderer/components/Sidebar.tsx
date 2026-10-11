import { Link, useRouterState } from '@tanstack/react-router'
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { isActive, NAV } from '../lib/nav'
import { S } from '../lib/strings'
import { useUi } from '../stores/ui'
import s from '../Layout.module.css'

export function Sidebar({ children }: { children?: React.ReactNode }) {
  const expanded = useUi((u) => u.sidebarExpanded)
  const pathname = useRouterState({ select: (r) => r.location.pathname })
  return (
    <nav aria-label="Main" className={s.sidebar} data-expanded={expanded}>
      <button className={s.toggle} aria-label={expanded ? S.collapseSidebar : S.expandSidebar} aria-expanded={expanded}
        onClick={() => useUi.getState().setSidebar(!expanded)}>
        {expanded ? <PanelLeftClose size={18} /> : <PanelLeftOpen size={18} />}
      </button>
      <ul className={s.nav}>
        {NAV.map((n) => {
          const active = isActive(pathname, n.to)
          return (
            <li key={n.to}>
              <Link to={n.to as never} className={s.navLink} aria-current={active ? 'page' : undefined} aria-label={n.label} title={expanded ? undefined : n.label}>
                <n.icon size={18} aria-hidden />
                {expanded && <span>{n.label}</span>}
              </Link>
            </li>
          )
        })}
      </ul>
      {expanded && children}
    </nav>
  )
}
