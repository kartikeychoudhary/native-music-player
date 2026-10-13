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
 * Plays files handed to Aurora by the OS. Files arriving close together (a multi-select Open with
 * launches the app once per file on some systems) join one queue, and files arriving before the
 * engine is ready are held until it is.
 */
export function createFileOpener(o: {
  call: (method: OpenMethod, params: object) => Promise<unknown>
  batchMs?: number
  setTimer?: (fn: () => void, ms: number) => unknown
  onOpen?: () => void
}): { open(paths: string[]): void; engineReady(): void; engineLost(): void } {
  const setTimer = o.setTimer ?? ((fn, ms) => setTimeout(fn, ms))
  let pending: string[] = []
  let ready = false
  let timer = false

  async function flush(): Promise<void> {
    const paths = pending
    pending = []
    try {
      await o.call('queue.setPaths', { paths, startIndex: 0 })
      await o.call('player.play', {})
      o.onOpen?.()
    } catch {
      // The engine went away mid-open; the user can open the file again.
    }
  }
  function schedule(): void {
    if (timer) return
    timer = true
    setTimer(() => {
      timer = false
      if (ready && pending.length > 0) void flush()
    }, o.batchMs ?? 250)
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
    engineLost() { ready = false },
  }
}
