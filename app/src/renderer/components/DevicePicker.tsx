import { Bluetooth, Headphones, Monitor, Speaker, Usb } from 'lucide-react'
import { usePlayer } from '../stores/player'

const ICON = { bluetooth: Bluetooth, usb: Usb, hdmi: Monitor, builtin: Speaker, other: Headphones } as const

export function DevicePicker() {
  const devices = usePlayer((p) => p.devices)
  const deviceId = usePlayer((p) => p.state.deviceId)
  const current = devices.find((d) => (deviceId ? d.id === deviceId : d.isDefault))
  const Icon = ICON[current?.transport ?? 'other']
  return (
    <label style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-muted)' }} title={current?.name ?? 'Output device'}>
      <Icon size={16} aria-hidden />
      <select
        aria-label="Output device" value={deviceId ?? ''} onChange={(e) => void usePlayer.getState().setDevice(e.target.value || null)}
        style={{ maxWidth: 'var(--device-max, 150px)', background: 'var(--raised)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 4, boxSizing: 'border-box', height: 'var(--control-h, 24px)', padding: '0 4px', font: '11px var(--font-ui)' }}
      >
        <option value="">System default</option>
        {devices.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
      </select>
    </label>
  )
}
