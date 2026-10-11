import { describe, expect, it } from 'vitest'
import { ALLOWED_METHODS, AUDIO_EXTENSIONS, isAllowedMethod } from '../src/shared/api'

describe('api allowlist', () => {
  it('allows every typed engine method except shutdown', () => {
    for (const m of ['player.play', 'queue.set', 'queue.insertNext', 'queue.setRepeat', 'library.albums', 'library.search', 'library.addFolder', 'playlists.reorder', 'playlists.importM3u'])
      expect(isAllowedMethod(m)).toBe(true)
    expect(isAllowedMethod('engine.shutdown')).toBe(false)
    expect(isAllowedMethod('library.nothing')).toBe(false)
    expect(isAllowedMethod({})).toBe(false)
    expect(new Set(ALLOWED_METHODS).size).toBe(ALLOWED_METHODS.length)
  })
  it('lists every supported audio extension', () => {
    for (const e of ['mp3', 'm4a', 'aac', 'flac', 'wav', 'aiff', 'ogg', 'opus', 'dsf', 'dff']) expect(AUDIO_EXTENSIONS).toContain(e)
  })
})
