/** Picks playback back up after an update restart: the flag is set only when music was playing as the update began. */
export interface ResumerDeps {
  get: () => boolean
  set: (v: boolean) => void
  play: () => Promise<unknown>
}

export function createResumer(o: ResumerDeps) {
  let status = 'stopped'
  let handled = false
  return {
    /** The last `player.state` status the engine reported. */
    noteStatus(s: string): void { status = s },
    /** Called as the update restart begins, before the engine stops. */
    beforeUpdateRestart(): void { if (status === 'playing') o.set(true) },
    /**
     * Called on the engine's ready status. Acts once per launch, so a later engine restart never resumes.
     * The flag is cleared first so a failed or interrupted resume cannot repeat on the following launch.
     */
    engineReady(e: { openingFiles: boolean }): void {
      if (handled) return
      handled = true
      if (!o.get()) return
      o.set(false)
      if (!e.openingFiles) o.play().catch(() => {})
    },
  }
}
