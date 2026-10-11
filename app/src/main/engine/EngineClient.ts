import { EventEmitter } from 'node:events'
import type { EngineReady } from '@protocol/EngineReady'
import { LineDecoder } from './lineCodec'

export interface ChildLike {
  stdin: NodeJS.WritableStream
  stdout: NodeJS.ReadableStream
  stderr: NodeJS.ReadableStream
  kill(): boolean
  on(e: 'exit', cb: (code: number | null) => void): unknown
  on(e: 'error', cb: (err: Error) => void): unknown
}

export class RpcError extends Error {
  constructor(public code: number, message: string, public data?: unknown) { super(message); this.name = 'RpcError' }
}

type Pending = { resolve: (v: any) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }

export class EngineClient extends EventEmitter {
  private pending = new Map<number, Pending>()
  private nextId = 1
  private exited = false
  private isReady = false
  private readyPromise: Promise<EngineReady>
  private settleReady!: { resolve: (r: EngineReady) => void; reject: (e: Error) => void }

  constructor(private child: ChildLike, private opts: { protocolVersion: number; timeoutMs?: number }) {
    super()
    this.readyPromise = new Promise((resolve, reject) => { this.settleReady = { resolve, reject } })
    this.readyPromise.catch(() => {}) // observed via ready()
    const out = new LineDecoder()
    const err = new LineDecoder()
    child.stdout.setEncoding?.('utf8')
    child.stderr.setEncoding?.('utf8')
    child.stdout.on('data', (c: string | Buffer) => out.push(c.toString()).forEach((l) => this.onLine(l)))
    child.stderr.on('data', (c: string | Buffer) => err.push(c.toString()).forEach((l) => this.emit('log', l)))
    child.on('exit', (code) => {
      this.exited = true
      this.failPending(new Error(`engine exited (code ${code})`))
      this.emit('exit', code)
    })
    // A failed spawn (e.g. ENOENT) emits 'error' instead of 'exit'. Before ready it is fatal for this client;
    // afterwards the exit path handles any real death, so the error is only logged.
    child.on('error', (err) => {
      if (this.isReady || this.exited) { this.emit('log', `engine process error: ${err.message}`); return }
      this.exited = true
      this.failPending(err)
      this.emit('spawnError', err)
    })
    // Writes to a dead child's stdin can raise EPIPE; the exit path deals with the death itself.
    child.stdin.on('error', (err: Error) => this.emit('log', `engine stdin error: ${err.message}`))
  }

  private failPending(e: Error) {
    this.settleReady.reject(e)
    for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(e) }
    this.pending.clear()
  }

  private onLine(line: string) {
    let msg: any
    try { msg = JSON.parse(line) } catch { this.emit('log', `non-JSON stdout line: ${line}`); return }
    if (typeof msg !== 'object' || msg === null || Array.isArray(msg)) { this.emit('log', `malformed stdout message: ${line}`); return }
    if (msg.id !== undefined && msg.id !== null && ('result' in msg || 'error' in msg)) {
      const p = this.pending.get(msg.id)
      if (!p) return
      this.pending.delete(msg.id)
      clearTimeout(p.timer)
      if (msg.error) p.reject(new RpcError(msg.error.code, msg.error.message, msg.error.data))
      else p.resolve(msg.result)
      return
    }
    if (msg.method === 'engine.ready') {
      const r = msg.params as EngineReady | undefined
      if (typeof r !== 'object' || r === null) {
        this.emit('log', `malformed engine.ready: ${line}`)
        this.settleReady.reject(Object.assign(new Error('malformed engine.ready'), { code: 'PROTOCOL_MISMATCH' }))
      } else if (r.protocolVersion !== this.opts.protocolVersion) {
        this.settleReady.reject(Object.assign(new Error(`protocol ${r.protocolVersion} != ${this.opts.protocolVersion}`), { code: 'PROTOCOL_MISMATCH' }))
      } else { this.isReady = true; this.settleReady.resolve(r) }
      return
    }
    if (typeof msg.method === 'string') this.emit('notification', msg.method, msg.params)
    else this.emit('log', `malformed stdout message: ${line}`)
  }

  ready(): Promise<EngineReady> { return this.readyPromise }

  call<T = unknown>(method: string, params: unknown = {}): Promise<T> {
    if (this.exited) return Promise.reject(new Error('engine exited'))
    const id = this.nextId++
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`${method} timed out`)) }, this.opts.timeoutMs ?? 10_000)
      this.pending.set(id, { resolve, reject, timer })
      this.child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n')
    })
  }

  dispose() { this.exited = true; this.child.kill() }
}
