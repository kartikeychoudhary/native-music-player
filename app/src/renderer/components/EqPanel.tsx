import { Save, Trash2 } from 'lucide-react'
import { S } from '../lib/strings'
import { useDsp } from '../stores/dsp'
import s from './EqPanel.module.css'

const BANDS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
const fmt = (db: number) => `${db > 0 ? '+' : ''}${db.toFixed(1)}`

export function EqPanel({ disabled }: { disabled: boolean }) {
  const eq = useDsp((d) => d.state?.eq)
  const presets = useDsp((d) => d.presets)
  if (!eq) return null
  const dsp = useDsp.getState()
  const current = presets.find((p) => p.name === eq.preset)
  return (
    <div role="group" aria-label={S.equalizer} className={s.panel} data-disabled={disabled || undefined}>
      <div className={s.head}>
        <button role="switch" aria-checked={eq.enabled} aria-label={S.equalizer} className={s.switch} disabled={disabled} onClick={() => void dsp.setEnabled(!eq.enabled)}>
          <span className={s.knob} />
        </button>
        <select aria-label={S.eqPreset} value={eq.preset ?? ''} disabled={disabled} onChange={(e) => { if (e.target.value) void dsp.applyPreset(e.target.value) }}>
          {!eq.preset && <option value="">{S.custom}</option>}
          {presets.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
        </select>
        <button className={s.icon} aria-label={S.savePreset} title={S.savePreset} disabled={disabled} onClick={() => void dsp.savePreset()}><Save size={15} /></button>
        <button className={s.icon} aria-label={S.deletePreset} title={S.deletePreset} disabled={disabled || !current || current.builtin} onClick={() => void dsp.deletePreset()}><Trash2 size={15} /></button>
      </div>
      <div className={s.sliders}>
        <label className={s.band}>
          <span className="mono">{fmt(eq.preamp)}</span>
          <input type="range" min={-12} max={12} step={0.5} value={eq.preamp} aria-label={S.preamp} disabled={disabled} onChange={(e) => void dsp.setPreamp(Number(e.target.value))} />
          <span className={s.label}>{S.preamp}</span>
        </label>
        <span className={s.divider} />
        {BANDS.map((hz, i) => (
          <label key={hz} className={s.band}>
            <span className="mono">{fmt(eq.bands[i] ?? 0)}</span>
            <input type="range" min={-12} max={12} step={0.5} value={eq.bands[i] ?? 0} aria-label={S.bandLabel(hz)} disabled={disabled} onChange={(e) => void dsp.setBand(i, Number(e.target.value))} />
            <span className={`${s.label} mono`}>{S.bandLabel(hz).replace(' Hz', '').replace(' kHz', 'k')}</span>
          </label>
        ))}
      </div>
    </div>
  )
}
