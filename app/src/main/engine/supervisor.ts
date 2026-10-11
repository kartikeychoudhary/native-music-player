import { EventEmitter } from 'node:events'
import { EngineClient, type ChildLike } from './EngineClient'

export type EngineStatus = { state: 'starting' | 'ready' | 'restarting' | 'failed' | 'mismatch'; message?: string }
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

  async stop() {
    this.generation++
    this.stopping = true
    const c = this.client
    if (!c) return
    await Promise.race([c.call('engine.shutdown').catch(() => {}), new Promise((r) => setTimeout(r, 1000))])
    c.dispose()
  }

  call<T = unknown>(method: string, params?: unknown): Promise<T> {
    if (!this.client || !this.isReady) return Promise.reject(new Error('engine not ready'))
    return this.client.call<T>(method, params)
  }
}
