import { Link, useRouterState } from '@tanstack/react-router'
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { isActive, NAV } from '../lib/nav'
import { S } from '../lib/strings'
import { useLibrary } from '../stores/library'
import { useUi } from '../stores/ui'
import { ScanStatus } from './ScanStatus'
import st from './ScanStatus.module.css'
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
      <ScanStatus compact={!expanded} className={st.sidebar} />
    </nav>
  )
}

export function SidebarPlaylists() {
  const playlists = useLibrary((l) => l.playlists)
  const pathname = useRouterState({ select: (r) => r.location.pathname })
  if (!playlists.length) return null
  return (
    <>
      <div className={s.sectionTitle}>{S.playlistsTitle.toUpperCase()}</div>
      <ul className={s.nav}>
        {playlists.map((p) => (
          <li key={p.id}>
            <Link to="/playlists/$playlistId" params={{ playlistId: String(p.id) }} className={s.navLink} aria-current={pathname === `/playlists/${p.id}` ? 'page' : undefined}>
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</span>
            </Link>
          </li>
        ))}
      </ul>
    </>
  )
}
