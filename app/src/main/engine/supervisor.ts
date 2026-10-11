import { EventEmitter } from 'node:events'
import type { EngineStatus } from '@shared/api'
import { EngineClient, type ChildLike } from './EngineClient'

/** How long stop() waits for the engine to exit after `engine.shutdown` before killing it. */
const STOP_TIMEOUT_MS = 3000

export type RestartDecision = { action: 'restart'; delayMs: number } | { action: 'giveUp' }

export function restartDecision(crashTimes: number[], now: number): RestartDecision {
  const recent = crashTimes.filter((t) => now - t <= 60_000)
  if (recent.length >= 3) return { action: 'giveUp' }
  return { action: 'restart', delayMs: Math.min(8_000, 500 * 2 ** Math.max(0, recent.length - 1)) }
}

export class EngineSupervisor extends EventEmitter {
  private client: EngineClient | null = null
  private isReady = false
  private crashes: number[] = []
  private stopping = false
  /** Bumped by every start()/stop()/restart(); a scheduled restart only fires if its generation is still current. */
  private generation = 0
  private current: EngineStatus = { state: 'starting' }
  private now: () => number
  private schedule: (fn: () => void, ms: number) => void

  constructor(private spawn: () => ChildLike, private opts: { protocolVersion: number; now?: () => number; schedule?: (fn: () => void, ms: number) => void }) {
    super()
    this.now = opts.now ?? Date.now
    this.schedule = opts.schedule ?? ((fn, ms) => { setTimeout(fn, ms) })
  }

  get status(): EngineStatus { return this.current }

  private setStatus(s: EngineStatus) { this.current = s; this.emit('status', s) }

  start() {
    this.generation++
    this.stopping = false
    this.isReady = false
    const old = this.client
    this.client = null // detach first so the old child's exit is not counted as a crash
    old?.dispose()
    this.setStatus({ state: 'starting' })
    let child: ChildLike
    try { child = this.spawn() } catch (e) { this.setStatus({ state: 'failed', message: `Couldn't start the audio engine: ${(e as Error).message}` }); return }
    const client = new EngineClient(child, { protocolVersion: this.opts.protocolVersion })
    this.client = client
    client.on('notification', (m, p) => this.emit('notification', m, p))
    client.on('log', (l) => this.emit('log', l))
    client.on('exit', () => this.onExit(client))
    client.on('spawnError', (e: Error) => {
      if (this.client !== client || this.isReady) return
      this.stopping = true // spawn failures (e.g. ENOENT) are not retried
      this.setStatus({ state: 'failed', message: `Couldn't start the audio engine: ${e.message}` })
      client.dispose()
    })
    client.ready().then(
      () => { if (this.client === client) { this.isReady = true; this.setStatus({ state: 'ready' }) } },
      (e) => {
        if (e?.code === 'PROTOCOL_MISMATCH' && this.client === client) {
          this.stopping = true
          this.setStatus({ state: 'mismatch', message: 'Engine version mismatch: reinstall Aurora.' })
          client.dispose()
        }
      },
    )
  }

  private onExit(client: EngineClient) {
    if (this.client !== client) return
    this.isReady = false
    if (this.stopping) return
    this.crashes.push(this.now())
    const d = restartDecision(this.crashes, this.now())
    if (d.action === 'giveUp') { this.setStatus({ state: 'failed', message: 'The audio engine crashed 3 times in a minute.' }); return }
    this.setStatus({ state: 'restarting' })
    const gen = this.generation
    this.schedule(() => { if (!this.stopping && this.generation === gen) this.start() }, d.delayMs)
  }

  restart() {
    this.crashes = []
    this.start()
  }

  /**
   * Asks the engine to shut down and waits (bounded) for it to exit on its own: the engine replies to
   * `engine.shutdown` first and writes its final session.json afterwards, so killing at the reply loses it.
   */
  async stop() {
    this.generation++
    this.stopping = true
    const c = this.client
    if (!c) return
    if (c.hasExited) return
    let timer: ReturnType<typeof setTimeout> | undefined
    const exited = new Promise<void>((r) => { c.once('exit', () => r()) })
    const bound = new Promise<void>((r) => { timer = setTimeout(r, STOP_TIMEOUT_MS) })
    c.call('engine.shutdown').catch(() => {})
    await Promise.race([exited, bound])
    clearTimeout(timer)
    if (!c.hasExited) c.dispose()
  }

  call<T = unknown>(method: string, params?: unknown): Promise<T> {
    if (!this.client || !this.isReady) return Promise.reject(new Error('engine not ready'))
    return this.client.call<T>(method, params)
  }
}
