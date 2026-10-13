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
