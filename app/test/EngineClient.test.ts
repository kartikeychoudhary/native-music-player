import { PassThrough } from 'node:stream'
import { EventEmitter } from 'node:events'
import { describe, expect, it } from 'vitest'
import { EngineClient, RpcError, type ChildLike } from '../src/main/engine/EngineClient'

function fakeChild() {
  const ev = new EventEmitter()
  const child = Object.assign(ev, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: () => { ev.emit('exit', null); return true } })
  const sent: any[] = []
  child.stdin.on('data', (b: Buffer) => b.toString().split('\n').filter(Boolean).forEach((l) => sent.push(JSON.parse(l))))
  const emit = (o: unknown) => child.stdout.write(JSON.stringify(o) + '\n')
  return { child: child as unknown as ChildLike & EventEmitter, sent, emit }
}

const ready = (v = 1) => ({ jsonrpc: '2.0', method: 'engine.ready', params: { version: '0.1.0', protocolVersion: v, features: [] } })

describe('EngineClient', () => {
  it('resolves ready and correlates responses by id', async () => {
    const { child, sent, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    emit(ready())
    await expect(c.ready()).resolves.toMatchObject({ protocolVersion: 1 })
    const p = c.call('player.state')
    await new Promise((r) => setImmediate(r))
    expect(sent[0]).toMatchObject({ jsonrpc: '2.0', method: 'player.state', id: 1 })
    emit({ jsonrpc: '2.0', id: 1, result: { status: 'stopped' } })
    await expect(p).resolves.toEqual({ status: 'stopped' })
  })

  it('maps error responses to RpcError', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    emit(ready())
    const p = c.call('nope')
    emit({ jsonrpc: '2.0', id: 1, error: { code: -32601, message: 'method not found' } })
    await expect(p).rejects.toBeInstanceOf(RpcError)
    await expect(p).rejects.toMatchObject({ code: -32601 })
  })

  it('emits notifications', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const got = new Promise((r) => c.on('notification', (m, p) => r([m, p])))
    emit({ jsonrpc: '2.0', method: 'player.state', params: { status: 'playing' } })
    await expect(got).resolves.toEqual(['player.state', { status: 'playing' }])
  })

  it('rejects ready on protocol mismatch', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    emit(ready(2))
    await expect(c.ready()).rejects.toMatchObject({ code: 'PROTOCOL_MISMATCH' })
  })

  it('rejects pending calls when the engine exits, and times out', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1, timeoutMs: 20 })
    emit(ready())
    await expect(c.call('slow')).rejects.toThrow(/timed out/)
    const p = c.call('player.state')
    child.emit('exit', 101)
    await expect(p).rejects.toThrow(/exited/)
  })

  it('ignores non-JSON stdout lines and reports them as logs', async () => {
    const { child } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const log = new Promise((r) => c.on('log', r))
    ;(child.stdout as unknown as PassThrough).write('garbage\n')
    await expect(log).resolves.toMatch(/garbage/)
  })

  it('routes valid-JSON non-object lines to log without throwing', async () => {
    const { child } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const logs: string[] = []
    c.on('log', (l) => logs.push(l))
    const out = child.stdout as unknown as PassThrough
    out.write('null\n')
    out.write('123\n')
    out.write('[1]\n')
    await new Promise((r) => setImmediate(r))
    expect(logs).toHaveLength(3)
  })

  it('treats engine.ready without params as a protocol mismatch', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const logs: string[] = []
    c.on('log', (l) => logs.push(l))
    emit({ jsonrpc: '2.0', method: 'engine.ready' })
    await expect(c.ready()).rejects.toMatchObject({ code: 'PROTOCOL_MISMATCH' })
    expect(logs.join('\n')).toMatch(/malformed/)
  })

  it('swallows stdin errors (EPIPE) and logs them', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const log = new Promise((r) => c.on('log', r))
    emit(ready())
    expect(() => (child.stdin as unknown as PassThrough).emit('error', new Error('write EPIPE'))).not.toThrow()
    await expect(log).resolves.toMatch(/EPIPE/)
  })

  it('rejects ready and pending calls on a child spawn error, emitting spawnError', async () => {
    const { child } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const p = c.call('x')
    const spawnErr = new Promise((r) => c.on('spawnError', r))
    const err = Object.assign(new Error('spawn engine ENOENT'), { code: 'ENOENT' })
    child.emit('error', err)
    await expect(c.ready()).rejects.toThrow(/ENOENT/)
    await expect(p).rejects.toThrow(/ENOENT/)
    await expect(spawnErr).resolves.toBe(err)
  })

  it('emits binary frames without disturbing JSON handling', async () => {
    const { child, emit } = fakeChild()
    const c = new EngineClient(child, { protocolVersion: 1 })
    const frames: [number, Uint8Array][] = []
    c.on('frame', (t: number, d: Uint8Array) => frames.push([t, d]))
    emit(ready())
    await expect(c.ready()).resolves.toBeTruthy()
    const f = new Uint8Array(70); f[0] = 0x1e; new DataView(f.buffer).setUint32(1, 65, true); f[5] = 1; f[6] = 200
    ;(child.stdout as unknown as PassThrough).write(Buffer.from(f))
    await new Promise((r) => setImmediate(r))
    expect(frames).toHaveLength(1)
    expect(frames[0][0]).toBe(1)
    expect(frames[0][1][0]).toBe(200)
  })
})
