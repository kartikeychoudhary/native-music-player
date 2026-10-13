import { describe, expect, it, vi } from 'vitest'
import { audioFilesFromArgv, createFileOpener } from '../src/main/openFiles'

const exists = (p: string) => !p.includes('missing')
describe('audio files from argv', () => {
  it('takes existing audio files after the executable (and the main script when unpackaged)', () => {
    expect(audioFilesFromArgv(['C:\\A\\Aurora.exe', 'C:\\Music\\a.FLAC', '--flag', 'C:\\Music\\notes.txt', 'C:\\Music\\missing.mp3', 'C:\\Music\\b.mp3'], { isPackaged: true, cwd: 'C:\\', exists }))
      .toEqual(['C:\\Music\\a.FLAC', 'C:\\Music\\b.mp3'])
    expect(audioFilesFromArgv(['/usr/bin/electron', '/repo/app/out/main/index.js', '/m/c.opus'], { isPackaged: false, cwd: '/', exists })).toEqual(['/m/c.opus'])
  })
  it('resolves relative paths against the launching directory and drops duplicates', () => {
    const got = audioFilesFromArgv(['/opt/aurora', 'song.ogg', './song.ogg'], { isPackaged: true, cwd: '/home/me/Music', exists })
    expect(got.map((p) => p.replace(/\\/g, '/'))).toEqual([expect.stringMatching(/\/home\/me\/Music\/song\.ogg$/)])
  })
})

describe('file opener', () => {
  function setup() {
    const calls: [string, object][] = []
    const timers: (() => void)[] = []
    const onOpen = vi.fn()
    const o = createFileOpener({ call: async (m, p) => { calls.push([m, p]) }, setTimer: (fn) => timers.push(fn), onOpen })
    const flushTimers = async () => { while (timers.length) timers.shift()!(); await Promise.resolve(); await Promise.resolve() }
    return { o, calls, flushTimers, onOpen }
  }
  it('holds files opened before the engine is ready and plays them once it is', async () => {
    const { o, calls, flushTimers, onOpen } = setup()
    o.open(['/m/a.flac'])
    await flushTimers()
    expect(calls).toEqual([])
    o.engineReady()
    await flushTimers()
    expect(calls).toEqual([['queue.setPaths', { paths: ['/m/a.flac'], startIndex: 0 }], ['player.play', {}]])
    expect(onOpen).toHaveBeenCalledTimes(1)
  })
  it('joins files opened in quick succession into one queue', async () => {
    const { o, calls, flushTimers } = setup()
    o.engineReady()
    o.open(['/m/a.flac']); o.open(['/m/b.flac'])
    await flushTimers()
    expect(calls[0]).toEqual(['queue.setPaths', { paths: ['/m/a.flac', '/m/b.flac'], startIndex: 0 }])
  })
  it('ignores an empty open and holds again after the engine is lost', async () => {
    const { o, calls, flushTimers } = setup()
    o.engineReady(); o.open([]); await flushTimers()
    expect(calls).toEqual([])
    o.engineLost(); o.open(['/m/c.mp3']); await flushTimers()
    expect(calls).toEqual([])
    o.engineReady(); await flushTimers()
    expect(calls.length).toBe(2)
  })
})

describe('file opener robustness', () => {
  /** A fake clock and scheduler: advance(t) runs every timer due by t, in order. */
  function clocked(callImpl?: (method: string, params: object) => Promise<unknown>) {
    let clock = 0
    const timers: { at: number; fn: () => void }[] = []
    const calls: [string, object][] = []
    const onError = vi.fn()
    const o = createFileOpener({
      call: async (m, p) => { calls.push([m, p]); if (callImpl) await callImpl(m, p) },
      setTimer: (fn, ms) => timers.push({ at: clock + ms, fn }),
      now: () => clock, onError,
    })
    const advance = async (to: number) => {
      for (;;) {
        const due = timers.filter((t) => t.at <= to).sort((a, b) => a.at - b.at)[0]
        if (!due) break
        timers.splice(timers.indexOf(due), 1)
        clock = Math.max(clock, due.at)
        due.fn()
      }
      clock = to
      for (let i = 0; i < 6; i++) await Promise.resolve()
    }
    return { o, calls, advance, onError }
  }

  it('puts the files back when the engine is lost mid-flush and plays them on the next ready', async () => {
    let rejectFirst!: (e: Error) => void
    let first = true
    const { o, calls, advance, onError } = clocked(() => { if (!first) return Promise.resolve(); first = false; return new Promise((_, rej) => { rejectFirst = rej }) })
    o.engineReady(); o.open(['/m/a.flac'])
    await advance(250) // the flush is under way: queue.setPaths is in flight
    expect(calls.length).toBe(1)
    o.engineLost(); rejectFirst(new Error('engine stopped'))
    await advance(300)
    o.engineReady(); await advance(700)
    expect(calls).toEqual([
      ['queue.setPaths', { paths: ['/m/a.flac'], startIndex: 0 }],
      ['queue.setPaths', { paths: ['/m/a.flac'], startIndex: 0 }],
      ['player.play', {}],
    ])
    expect(onError).not.toHaveBeenCalled()
  })
  it('reports a failure while the engine is ready', async () => {
    const { o, advance, onError } = clocked(async () => { throw new Error('bad path') })
    o.engineReady(); o.open(['/m/a.flac']); await advance(250)
    expect(onError).toHaveBeenCalledTimes(1)
  })
  it('joins opens spread over a few hundred milliseconds into one batch', async () => {
    const { o, calls, advance } = clocked()
    o.engineReady()
    o.open(['/m/a.flac']); await advance(200)
    o.open(['/m/b.flac']); await advance(400)
    o.open(['/m/c.flac']); await advance(600)
    expect(calls).toEqual([])
    await advance(700)
    expect(calls[0]).toEqual(['queue.setPaths', { paths: ['/m/a.flac', '/m/b.flac', '/m/c.flac'], startIndex: 0 }])
    expect(calls.filter((c) => c[0] === 'queue.setPaths').length).toBe(1)
  })
  it('flushes a batch at most 2 s after its first file, however long files keep arriving', async () => {
    const { o, calls, advance } = clocked()
    o.engineReady()
    for (let t = 0; t <= 1800; t += 200) { o.open([`/m/${t}.flac`]); await advance(t + 200 > 1800 ? t : t + 200) }
    await advance(2000)
    expect(calls[0][0]).toBe('queue.setPaths')
    expect((calls[0][1] as { paths: string[] }).paths.length).toBe(10)
  })
})
