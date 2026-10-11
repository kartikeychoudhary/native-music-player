import { Activity, House, type LucideIcon } from 'lucide-react'
import { S } from './strings'

export interface NavItem { to: string; label: string; icon: LucideIcon }

/** Sidebar entries in display order. Tasks add their views here as their routes land. */
export const NAV: NavItem[] = [
  { to: '/', label: S.nav.home, icon: House },
  { to: '/signal', label: S.nav.signal, icon: Activity },
]

export function isActive(pathname: string, to: string): boolean {
  return to === '/' ? pathname === '/' : pathname === to || pathname.startsWith(`${to}/`)
}
