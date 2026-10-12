import { describe, expect, it } from 'vitest'
import { miniPosition } from '../src/main/windows'

const screen = { x: 0, y: 0, width: 1920, height: 1040 }
const second = { x: 1920, y: 0, width: 1280, height: 1000 }

describe('miniPosition', () => {
  it('mini_bounds_are_clamped_to_a_visible_display', () => {
    expect(miniPosition({ x: 100, y: 200 }, [screen])).toEqual({ x: 100, y: 200 })
    expect(miniPosition({ x: 2000, y: 50 }, [screen, second]), 'fits on the second display').toEqual({ x: 2000, y: 50 })
    expect(miniPosition({ x: 2000, y: 50 }, [screen]), 'display unplugged').toEqual({ x: 1920 - 340 - 24, y: 1040 - 110 - 24 })
    expect(miniPosition({ x: 1700, y: 1000 }, [screen]), 'partly off-screen').toEqual({ x: 1556, y: 906 })
    expect(miniPosition(null, [screen])).toEqual({ x: 1556, y: 906 })
  })
})
