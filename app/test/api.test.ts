import { describe, expect, it } from 'vitest'
import { ALLOWED_METHODS, AUDIO_EXTENSIONS, isAllowedMethod } from '../src/shared/api'

describe('api allowlist', () => {
  it('allows exactly the M1 engine methods', () => {
    expect([...ALLOWED_METHODS].sort()).toEqual([
      'devices.list', 'player.next', 'player.pause', 'player.play', 'player.prev', 'player.seek', 'player.setDevice',
      'player.setMode', 'player.setVolume', 'player.state', 'player.stop', 'player.toggle', 'queue.get', 'queue.setPaths',
    ])
    expect(isAllowedMethod('engine.shutdown')).toBe(false)
    expect(isAllowedMethod({})).toBe(false)
  })
  it('lists every supported audio extension', () => {
    for (const e of ['mp3', 'm4a', 'aac', 'flac', 'wav', 'aiff', 'ogg', 'opus', 'dsf', 'dff']) expect(AUDIO_EXTENSIONS).toContain(e)
  })
})
