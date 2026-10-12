import { create } from 'zustand'
import type { LucideIcon } from 'lucide-react'
import type { Settings } from '@shared/settings'
import { usePlayer } from './player'

export type NowPlayingTab = 'lyrics' | 'queue' | 'signal' | 'credits'

export interface MenuItem { label: string; icon?: LucideIcon; onSelect?: () => void; disabled?: boolean; items?: MenuItem[]; separator?: boolean }
export interface Toast { id: number; kind: 'info' | 'error'; text: string }
export interface PromptState { title: string; initial: string; confirm: string; resolve: (value: string | null) => void }

export interface UiStore {
  sidebarExpanded: boolean
  toasts: Toast[]
  menu: { x: number; y: number; items: MenuItem[] } | null
  trackInfo: number | null
  prompt: PromptState | null
  searchText: string
  nowPlayingTab: NowPlayingTab
  setNowPlayingTab(t: NowPlayingTab): void
  accentTint: boolean
  setAccentTint(on: boolean): void
  autoUpdate: boolean
  setAutoUpdate(on: boolean): void
  setSidebar(expanded: boolean, persist?: boolean): void
  applySettings(s: Settings): void
  toast(kind: Toast['kind'], text: string): void
  dismissToast(id: number): void
  openMenu(x: number, y: number, items: MenuItem[]): void
  closeMenu(): void
  showTrackInfo(id: number | null): void
  askName(o: { title: string; initial?: string; confirm: string }): Promise<string | null>
  setSearchText(text: string): void
}

let nextToast = 1
export const TOAST_MS = 5000

export const useUi = create<UiStore>()((set, get) => ({
  sidebarExpanded: false,
  toasts: [],
  menu: null,
  trackInfo: null,
  prompt: null,
  searchText: '',
  nowPlayingTab: 'lyrics',
  setNowPlayingTab: (t) => set({ nowPlayingTab: t }),
  accentTint: false,
  setAccentTint(on) {
    set({ accentTint: on })
    void usePlayer.getState().api?.setSettings({ accentTint: on }).catch(() => {})
  },
  autoUpdate: true,
  setAutoUpdate(on) {
    set({ autoUpdate: on })
    void usePlayer.getState().api?.setSettings({ autoUpdate: on }).catch(() => {})
  },
  setSidebar(expanded, persist = true) {
    set({ sidebarExpanded: expanded })
    if (persist) void usePlayer.getState().api?.setSettings({ sidebarExpanded: expanded }).catch(() => {})
  },
  applySettings: (s) => set({ sidebarExpanded: s.sidebarExpanded, accentTint: s.accentTint, autoUpdate: s.autoUpdate }),
  toast(kind, text) {
    const id = nextToast++
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, text }] }))
    setTimeout(() => get().dismissToast(id), TOAST_MS)
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  openMenu: (x, y, items) => set({ menu: { x, y, items } }),
  closeMenu: () => set({ menu: null }),
  showTrackInfo: (trackInfo) => set({ trackInfo }),
  askName: ({ title, initial = '', confirm }) =>
    new Promise((resolve) => {
      get().prompt?.resolve(null)
      set({ prompt: { title, initial, confirm, resolve: (v) => { set({ prompt: null }); resolve(v) } } })
    }),
  setSearchText: (searchText) => set({ searchText }),
}))
