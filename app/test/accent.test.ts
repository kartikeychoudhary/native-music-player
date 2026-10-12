import { describe, expect, it } from 'vitest'
import { contrastRatio, tintFor } from '../src/renderer/lib/accent'

describe('accent tint', () => {
  it('uses the vibrant colour only when enabled and readable on the background', () => {
    expect(contrastRatio('#ffffff', '#000000')).toBeCloseTo(21, 1)
    expect(contrastRatio('#FFB547', '#17151A')).toBeGreaterThan(4.5)
    expect(tintFor('#4FC3F7', true)).toBe('#4FC3F7')
    expect(tintFor('#4FC3F7', false)).toBeNull()
    expect(tintFor('#2A2530', true), 'too dark on --bg').toBeNull()
    expect(tintFor(null, true)).toBeNull()
  })
})
