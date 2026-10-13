import { isAbsolute, resolve } from 'node:path'
import { AUDIO_EXTENSIONS } from '@shared/api'

const audio = new Set<string>(AUDIO_EXTENSIONS)

/**
 * The audio files named on a command line (a double-click, "Open with Aurora" or a second launch).
 * Skips the executable, plus the main script when running unpackaged, and anything that is a flag.
 */
export function audioFilesFromArgv(argv: readonly string[], o: { isPackaged: boolean; cwd: string; exists: (p: string) => boolean }): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const arg of argv.slice(o.isPackaged ? 1 : 2)) {
    if (!arg || arg.startsWith('-')) continue
    const dot = arg.lastIndexOf('.')
    if (dot < 0 || !audio.has(arg.slice(dot + 1).toLowerCase())) continue
    const path = isAbsolute(arg) ? arg : resolve(o.cwd, arg)
    // Windows paths are case-insensitive: 'A.mp3' and 'a.MP3' are one file.
    const key = process.platform === 'win32' ? path.toLowerCase() : path
    if (seen.has(key) || !o.exists(path)) continue
    seen.add(key)
    out.push(path)
  }
  return out
}

type OpenMethod = 'queue.setPaths' | 'player.play'

/**
 * Plays files handed to Aurora by the OS. Files arriving close together join one queue (Windows starts
 * one process per file of a multi-select Open, so they can trickle in over a second or more): each open
 * restarts the batch timer, but a batch never waits longer than maxBatchMs for its first file. Files
 * arriving before the engine is ready are held until it is, and files caught by an engine restart
 * mid-flush are put back.
 */
export function createFileOpener(o: {
  call: (method: OpenMethod, params: object) => Promise<unknown>
  batchMs?: number
  maxBatchMs?: number
  setTimer?: (fn: () => void, ms: number) => unknown
  now?: () => number
  onOpen?: () => void
  onError?: (e: unknown) => void
}): { open(paths: string[]): void; engineReady(): void; engineLost(): void } {
  const setTimer = o.setTimer ?? ((fn, ms) => setTimeout(fn, ms))
  const now = o.now ?? Date.now
  const batchMs = o.batchMs ?? 250
  const maxBatchMs = o.maxBatchMs ?? 2000
  let pending: string[] = []
  let ready = false
  let lost = 0 // counts engineLost() calls, to notice a restart during a flush
  let batchStart: number | null = null
  let token = 0 // only the newest timer acts; older ones were superseded by a later open()

  async function flush(): Promise<void> {
    const paths = pending
    pending = []
    const lostBefore = lost
    try {
      await o.call('queue.setPaths', { paths, startIndex: 0 })
      await o.call('player.play', {})
      o.onOpen?.()
    } catch (e) {
      if (!ready || lost !== lostBefore) {
        pending = [...paths, ...pending] // the engine restarted under us: play them once it is back
        if (ready) schedule()
      } else {
        o.onError?.(e)
      }
    }
  }
  function schedule(): void {
    if (batchStart === null) batchStart = now()
    const mine = ++token
    const delay = Math.max(0, Math.min(batchMs, batchStart + maxBatchMs - now()))
    setTimer(() => {
      if (mine !== token) return
      batchStart = null
      if (ready && pending.length > 0) void flush()
    }, delay)
  }

  return {
    open(paths) {
      if (paths.length === 0) return
      pending.push(...paths)
      schedule()
    },
    engineReady() {
      ready = true
      if (pending.length > 0) schedule()
    },
    engineLost() { ready = false; lost++ },
  }
}
