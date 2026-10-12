import { create } from 'zustand'
import type { AuroraApi, UpdateState } from '@shared/api'

interface Updates { api: AuroraApi | null; state: UpdateState; dismissed: string | null; init(api: AuroraApi): () => void; dismiss(version: string): void }

export const useUpdates = create<Updates>((set) => ({
  api: null,
  state: { status: 'idle' },
  dismissed: null,
  init(api) {
    set({ api })
    void api.updateState().then((state) => set({ state }))
    return api.on('update.state', (state) => set({ state }))
  },
  dismiss: (version) => set({ dismissed: version }),
}))
