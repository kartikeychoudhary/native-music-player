import { describe, expect, it, vi } from 'vitest'
import { safeSend } from '../src/main/safeSend'

function target(over: { win?: boolean; wc?: boolean; send?: (...a: unknown[]) => void } = {}) {
  const send = vi.fn(over.send ?? (() => {}))
  return { send, t: { isDestroyed: () => over.win ?? false, webContents: { isDestroyed: () => over.wc ?? false, send } } }
}

describe('safeSend', () => {
  it('returns false for a null target', () => {
    expect(safeSend(null, 'aurora:event', 'x', 1)).toBe(false)
  })
  it('returns false and does not send when the window is destroyed', () => {
    const { t, send } = target({ win: true })
    expect(safeSend(t, 'aurora:event')).toBe(false)
    expect(send).not.toHaveBeenCalled()
  })
  it('returns false and does not send when webContents is destroyed', () => {
    const { t, send } = target({ wc: true })
    expect(safeSend(t, 'aurora:event')).toBe(false)
    expect(send).not.toHaveBeenCalled()
  })
  it('swallows a send that throws and returns false', () => {
    const { t } = target({ send: () => { throw new Error('Render frame was disposed') } })
    expect(() => safeSend(t, 'aurora:event', 'x')).not.toThrow()
    expect(safeSend(t, 'aurora:event', 'x')).toBe(false)
  })
  it('sends the channel and args and returns true', () => {
    const { t, send } = target()
    expect(safeSend(t, 'aurora:event', 'engine.status', { state: 'ready' })).toBe(true)
    expect(send).toHaveBeenCalledWith('aurora:event', 'engine.status', { state: 'ready' })
  })
})
