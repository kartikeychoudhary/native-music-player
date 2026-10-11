import { usePlayer } from '../stores/player'

export function EngineBanner() {
  const engine = usePlayer((p) => p.engine)
  const error = usePlayer((p) => p.error)
  const api = usePlayer((p) => p.api)
  const bar: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 12, padding: '8px 14px', margin: '0 0 16px', borderRadius: 6, border: '1px solid var(--danger)', color: 'var(--text)', background: 'rgb(255 107 107 / 0.08)' }
  if (engine.state === 'failed' || engine.state === 'mismatch') {
    return (
      <div role="alert" style={bar}>
        <span style={{ flex: 1 }}>{engine.message}</span>
        {engine.state === 'failed' && <button onClick={() => void api?.restartEngine()} style={{ color: 'var(--accent)' }}>Restart engine</button>}
        <button onClick={() => void api?.openLogs()} style={{ color: 'var(--text-muted)' }}>Open logs</button>
      </div>
    )
  }
  if (error) {
    return (
      <div role="status" style={{ ...bar, borderColor: 'var(--border)', background: 'var(--raised)' }}>
        <span className="mono" style={{ color: 'var(--accent)', fontSize: 10 }}>{error.name}</span>
        <span style={{ flex: 1 }}>{error.message}{error.path ? ` · ${error.path.split(/[\\/]/).pop()}` : ''}</span>
        <button onClick={() => usePlayer.getState().dismissError()} aria-label="Dismiss" style={{ color: 'var(--text-muted)' }}>✕</button>
      </div>
    )
  }
  return null
}
