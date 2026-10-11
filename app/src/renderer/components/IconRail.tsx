import { Activity, House, Library, Search, Settings } from 'lucide-react'
import { usePlayer } from '../stores/player'

export function IconRail() {
  const panel = usePlayer((p) => p.panel)
  const btn = (active: boolean): React.CSSProperties => ({ width: 40, height: 40, display: 'grid', placeItems: 'center', borderRadius: 6, color: active ? 'var(--accent)' : 'var(--text-muted)', background: active ? 'var(--raised)' : 'transparent' })
  return (
    <nav aria-label="Main" style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10, paddingTop: 14, background: 'var(--surface)', borderRight: '1px solid var(--border)' }}>
      <button aria-label="Home" style={btn(panel === 'queue')} onClick={() => usePlayer.getState().setPanel('queue')}><House size={18} /></button>
      <button aria-label="Library" title="Library arrives in M2" disabled style={btn(false)}><Library size={18} /></button>
      <button aria-label="Search" title="Search arrives in M2" disabled style={btn(false)}><Search size={18} /></button>
      <button aria-label="Signal path" style={btn(panel === 'signal')} onClick={() => usePlayer.getState().setPanel('signal')}><Activity size={18} /></button>
      <span style={{ flex: 1 }} />
      <button aria-label="Settings" title="Settings arrive in M2" disabled style={{ ...btn(false), marginBottom: 14 }}><Settings size={18} /></button>
    </nav>
  )
}
