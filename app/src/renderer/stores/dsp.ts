import { create } from 'zustand'
import type { DspState } from '@protocol/DspState'
import type { EqPreset } from '@protocol/EqPreset'
import type { ReplayGainMode } from '@protocol/ReplayGainMode'
import type { AuroraApi } from '@shared/api'
import { S } from '../lib/strings'
import { useUi } from './ui'

interface DspStore {
  api: AuroraApi | null
  state: DspState | null
  presets: EqPreset[]
  connect(api: AuroraApi): () => void
  setBand(i: number, db: number): Promise<void>
  setPreamp(db: number): Promise<void>
  setEnabled(on: boolean): Promise<void>
  applyPreset(name: string): Promise<void>
  savePreset(): Promise<void>
  deletePreset(): Promise<void>
  setCrossfade(secs: number): Promise<void>
  setReplayGain(mode: ReplayGainMode): Promise<void>
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

export const useDsp = create<DspStore>()((set, get) => {
  const load = async () => {
    const api = get().api
    if (!api) return
    try {
      const [state, presets] = await Promise.all([api.call('dsp.get', {}), api.call('dsp.presets', {})])
      set({ state, presets })
    } catch { /* engine not ready; engine.status → ready reloads */ }
  }
  /** Runs an engine call; on failure shows the error and reloads the engine's truth. */
  const run = async <T,>(p: (api: AuroraApi) => Promise<T>): Promise<T | null> => {
    const api = get().api
    if (!api) return null
    try { return await p(api) } catch (e) { useUi.getState().toast('error', message(e)); void load(); return null }
  }
  const sendEq = (bands: number[], preamp: number, preset?: string) => {
    const s = get().state
    if (s) set({ state: { ...s, eq: { ...s.eq, bands, preamp, preset: preset ?? null } } })
    return run((a) => a.call('dsp.setEq', preset ? { bands, preamp, preset } : { bands, preamp })).then(() => {})
  }
  return {
    api: null,
    state: null,
    presets: [],
    connect(api) {
      set({ api })
      const offs = [
        api.on('dsp.changed', (state) => set({ state })),
        api.on('engine.status', (s) => { if (s.state === 'ready') void load() }),
      ]
      void load()
      return () => offs.forEach((off) => off())
    },
    async setBand(i, db) {
      const s = get().state
      if (!s) return
      const bands = s.eq.bands.map((b, j) => (j === i ? db : b))
      await sendEq(bands, s.eq.preamp)
    },
    async setPreamp(db) { const s = get().state; if (s) await sendEq(s.eq.bands, db) },
    async setEnabled(on) {
      const s = get().state
      if (s) set({ state: { ...s, eq: { ...s.eq, enabled: on } } })
      await run((a) => a.call('dsp.setEqEnabled', { enabled: on }))
    },
    async applyPreset(name) {
      const p = get().presets.find((x) => x.name === name)
      if (p) await sendEq(p.bands, p.preamp, p.name)
    },
    async savePreset() {
      const name = await useUi.getState().askName({ title: S.savePresetTitle, confirm: S.save })
      if (!name) return
      if (await run((a) => a.call('dsp.savePreset', { name }))) await load()
    },
    async deletePreset() {
      const name = get().state?.eq.preset
      const p = get().presets.find((x) => x.name === name)
      if (!name || !p || p.builtin) return
      if (await run((a) => a.call('dsp.deletePreset', { name }))) await load()
    },
    async setCrossfade(secs) {
      const s = get().state
      if (s) set({ state: { ...s, crossfadeSecs: secs } })
      await run((a) => a.call('dsp.setCrossfade', { seconds: secs }))
    },
    async setReplayGain(mode) {
      const s = get().state
      if (s) set({ state: { ...s, replayGain: mode } })
      await run((a) => a.call('dsp.setReplayGain', { mode }))
    },
  }
})
