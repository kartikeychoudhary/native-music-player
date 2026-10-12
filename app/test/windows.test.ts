import { describe, expect, it } from 'vitest'
import { miniPosition, primaryFirst } from '../src/main/windows'

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

describe('primaryFirst', () => {
  it('mini_default_lands_on_the_primary_display', () => {
    // getAllDisplays() need not list the primary display first.
    const displays = [{ id: 2, workArea: second }, { id: 1, workArea: screen }]
    expect(primaryFirst(displays, 1)).toEqual([screen, second])
    expect(miniPosition(null, primaryFirst(displays, 1))).toEqual({ x: 1556, y: 906 })
    expect(miniPosition({ x: 2000, y: 50 }, primaryFirst(displays, 1)), 'a saved spot on another display still wins').toEqual({ x: 2000, y: 50 })
    expect(primaryFirst(displays, 9), 'unknown primary keeps the order').toEqual([second, screen])
  })
})
