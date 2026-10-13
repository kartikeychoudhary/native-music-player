import { describe, expect, it, vi } from 'vitest'
import { createResumer } from '../src/main/resumeAfterUpdate'

function setup(stored = false) {
  let flag = stored
  const play = vi.fn(async () => {})
  const r = createResumer({ get: () => flag, set: (v) => { flag = v }, play })
  return { r, play, flag: () => flag }
}

describe('resume after update', () => {
  it('remembers to resume only if music was playing when the update restart began', () => {
    const a = setup(); a.r.noteStatus('playing'); a.r.beforeUpdateRestart(); expect(a.flag()).toBe(true)
    const b = setup(); b.r.noteStatus('paused'); b.r.beforeUpdateRestart(); expect(b.flag()).toBe(false)
  })
  it('resumes once on the next launch and clears the flag first', () => {
    const { r, play, flag } = setup(true)
    r.engineReady({ openingFiles: false })
    expect(flag()).toBe(false)
    expect(play).toHaveBeenCalledTimes(1)
    r.engineReady({ openingFiles: false }) // an engine restart later in the session
    expect(play).toHaveBeenCalledTimes(1)
  })
  it('does not resume when launched to open files, or after an ordinary quit', () => {
    const a = setup(true); a.r.engineReady({ openingFiles: true }); expect(a.play).not.toHaveBeenCalled(); expect(a.flag()).toBe(false)
    const b = setup(false); b.r.engineReady({ openingFiles: false }); expect(b.play).not.toHaveBeenCalled()
  })
})
