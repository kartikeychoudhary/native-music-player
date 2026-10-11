import { EventEmitter } from 'node:events'
import { PassThrough } from 'node:stream'
import { describe, expect, it } from 'vitest'
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
  const spawn = () => {
    const ev = new EventEmitter()
    const child = Object.assign(ev, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: () => true })
    children.push(child)
    return child as any
  }
  const ready = (i: number, v = 1) => children[i].stdout.write(JSON.stringify({ jsonrpc: '2.0', method: 'engine.ready', params: { version: 'x', protocolVersion: v, features: [] } }) + '\n')
  return { spawn, children, ready }
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
})
