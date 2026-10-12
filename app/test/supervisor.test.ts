import { EventEmitter } from 'node:events'
import { PassThrough } from 'node:stream'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { EngineSupervisor, restartDecision } from '../src/main/engine/supervisor'

describe('restartDecision', () => {
  it('backs off exponentially and gives up after 3 crashes in 60 s', () => {
    expect(restartDecision([1000], 1000)).toEqual({ action: 'restart', delayMs: 500 })
    expect(restartDecision([1000, 2000], 2000)).toEqual({ action: 'restart', delayMs: 1000 })
    expect(restartDecision([1000, 2000, 3000], 3000)).toEqual({ action: 'giveUp' })
    expect(restartDecision([0, 70_000, 71_000], 71_000)).toEqual({ action: 'restart', delayMs: 1000 })
  })
})

function spawner() {
  const children: any[] = []
  const kills: number[] = []
  const spawn = () => {
    const ev = new EventEmitter()
    const child = Object.assign(ev, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: () => { kills.push(children.indexOf(child)); return true } })
    children.push(child)
    return child as any
  }
  const ready = (i: number, v = 1) => children[i].stdout.write(JSON.stringify({ jsonrpc: '2.0', method: 'engine.ready', params: { version: 'x', protocolVersion: v, features: [] } }) + '\n')
  return { spawn, children, ready, kills }
}

describe('EngineSupervisor', () => {
  it('restarts after a crash and gives up after three', async () => {
    let now = 0
    const timers: Array<() => void> = []
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1, now: () => now, schedule: (fn) => timers.push(fn) })
    const states: string[] = []
    sup.on('status', (st) => states.push(st.state))
    sup.start()
    s.ready(0)
    await new Promise((r) => setImmediate(r))
    expect(sup.status.state).toBe('ready')
    for (let i = 0; i < 3; i++) {
      now += 1000
      s.children[i].emit('exit', 1)
      if (i < 2) { timers.shift()!(); s.ready(i + 1); await new Promise((r) => setImmediate(r)) }
    }
    expect(states).toEqual(['starting', 'ready', 'restarting', 'starting', 'ready', 'restarting', 'starting', 'ready', 'failed'])
    expect(s.children).toHaveLength(3)
  })

  it('carries the engine.ready info on the ready status', async () => {
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 3 })
    sup.start()
    s.children[0].stdout.write(JSON.stringify({ jsonrpc: '2.0', method: 'engine.ready', params: { version: '0.1.0', protocolVersion: 3, features: ['opus', 'fdk-aac'] } }) + '\n')
    await new Promise((r) => setImmediate(r))
    expect(sup.status).toEqual({ state: 'ready', engine: { version: '0.1.0', protocolVersion: 3, features: ['opus', 'fdk-aac'] } })
  })

  it('does not restart on protocol mismatch', async () => {
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1, schedule: () => { throw new Error('must not schedule') } })
    sup.start()
    s.ready(0, 9)
    await new Promise((r) => setImmediate(r))
    expect(sup.status.state).toBe('mismatch')
  })

  it('rejects calls while not ready', async () => {
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1 })
    sup.start()
    await expect(sup.call('player.state')).rejects.toThrow(/not ready/)
  })

  async function crashedAndRestarting() {
    const timers: Array<() => void> = []
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1, now: () => 0, schedule: (fn) => timers.push(fn) })
    sup.start()
    s.ready(0)
    await new Promise((r) => setImmediate(r))
    s.children[0].emit('exit', 1)
    expect(sup.status.state).toBe('restarting')
    expect(timers).toHaveLength(1)
    return { s, sup, timers }
  }

  it('restart() during restarting cancels the stale timer: one new spawn, old client killed', async () => {
    const { s, sup, timers } = await crashedAndRestarting()
    sup.restart()
    expect(s.children).toHaveLength(2)
    expect(s.kills).toContain(0)
    timers.shift()!()
    expect(s.children).toHaveLength(2)
  })

  it('stop() during restarting cancels the pending restart', async () => {
    const { s, sup, timers } = await crashedAndRestarting()
    await sup.stop()
    timers.shift()!()
    expect(s.children).toHaveLength(1)
  })

  it('stop() then start() inside the delay window does not spawn a second engine', async () => {
    const { s, sup, timers } = await crashedAndRestarting()
    await sup.stop()
    sup.start()
    expect(s.children).toHaveLength(2)
    timers.shift()!()
    expect(s.children).toHaveLength(2)
  })

  it('start() disposes the existing client before replacing it', () => {
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1 })
    sup.start()
    sup.start()
    expect(s.children).toHaveLength(2)
    expect(s.kills).toEqual([0])
    expect(sup.status.state).toBe('starting')
  })

  it('fails without retrying when the child emits a spawn error before ready', async () => {
    const s = spawner()
    const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1, schedule: () => { throw new Error('must not schedule') } })
    const states: string[] = []
    sup.on('status', (st) => states.push(st.state))
    sup.start()
    s.children[0].emit('error', Object.assign(new Error('spawn aurora-engine ENOENT'), { code: 'ENOENT' }))
    s.children[0].emit('exit', null)
    await new Promise((r) => setImmediate(r))
    expect(sup.status).toEqual({ state: 'failed', message: "Couldn't start the audio engine: spawn aurora-engine ENOENT" })
    expect(states).toEqual(['starting', 'failed'])
  })

  describe('stop()', () => {
    afterEach(() => vi.useRealTimers())

    async function readySup() {
      vi.useFakeTimers()
      const s = spawner()
      const sup = new EngineSupervisor(s.spawn, { protocolVersion: 1 })
      sup.start()
      s.ready(0)
      await vi.advanceTimersByTimeAsync(0)
      expect(sup.status.state).toBe('ready')
      return { s, sup }
    }
    const reply = (s: ReturnType<typeof spawner>) =>
      s.children[0].stdout.write(JSON.stringify({ jsonrpc: '2.0', id: 1, result: null }) + '\n')

    it('waits for the engine to exit after the shutdown reply instead of killing it', async () => {
      const { s, sup } = await readySup()
      let done = false
      const stopping = sup.stop().then(() => { done = true })
      reply(s)
      await vi.advanceTimersByTimeAsync(200)
      expect(done).toBe(false)
      expect(s.kills).toEqual([])
      s.children[0].emit('exit', 0)
      await stopping
      expect(s.kills).toEqual([])
      expect(vi.getTimerCount()).toBe(0)
    })

    it('kills an engine that never exits once the bound has passed', async () => {
      const { s, sup } = await readySup()
      const stopping = sup.stop()
      reply(s)
      await vi.advanceTimersByTimeAsync(2999)
      expect(s.kills).toEqual([])
      await vi.advanceTimersByTimeAsync(1)
      await stopping
      expect(s.kills).toEqual([0])
    })

    it('a repeat stop() after the engine exited returns at once and leaves no timers', async () => {
      const { s, sup } = await readySup()
      const stopping = sup.stop()
      s.children[0].emit('exit', 0)
      await stopping
      await sup.stop()
      expect(vi.getTimerCount()).toBe(0)
    })
  })
})
