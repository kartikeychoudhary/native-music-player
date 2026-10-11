import { Activity, Disc3, House, ListMusic, ListOrdered, Search, Tags, Users, type LucideIcon } from 'lucide-react'
import { S } from './strings'

export interface NavItem { to: string; label: string; icon: LucideIcon }

/** Sidebar entries in display order. Tasks add their views here as their routes land. */
export const NAV: NavItem[] = [
  { to: '/', label: S.nav.home, icon: House },
  { to: '/search', label: S.nav.search, icon: Search },
  { to: '/albums', label: S.nav.albums, icon: Disc3 },
  { to: '/artists', label: S.nav.artists, icon: Users },
  { to: '/genres', label: S.nav.genres, icon: Tags },
  { to: '/playlists', label: S.nav.playlists, icon: ListMusic },
  { to: '/queue', label: S.nav.queue, icon: ListOrdered },
  { to: '/signal', label: S.nav.signal, icon: Activity },
]

export function isActive(pathname: string, to: string): boolean {
  return to === '/' ? pathname === '/' : pathname === to || pathname.startsWith(`${to}/`)
}
