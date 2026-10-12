import { S } from '../lib/strings'
import { useUpdates } from '../stores/updates'

export function UpdateBanner() {
  const state = useUpdates((u) => u.state)
  const dismissed = useUpdates((u) => u.dismissed)
  const api = useUpdates((u) => u.api)
  if ((state.status !== 'available' && state.status !== 'ready') || dismissed === state.version) return null
  const bar: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 12, padding: '8px 14px', margin: '0 0 16px', borderRadius: 6, border: '1px solid var(--border)', background: 'var(--raised)' }
  return (
    <div role="status" style={bar}>
      <span style={{ flex: 1 }}>{state.status === 'ready' ? S.updateReady(state.version) : S.updateAvailable(state.version)}</span>
      {state.status === 'ready'
        ? <button onClick={() => void api?.installUpdate()} style={{ color: 'var(--accent)' }}>{S.restartNow}</button>
        : <button onClick={() => void api?.openRelease()} style={{ color: 'var(--accent)' }}>{S.download}</button>}
      <button onClick={() => useUpdates.getState().dismiss(state.version)} style={{ color: 'var(--text-muted)' }}>{S.later}</button>
    </div>
  )
}
