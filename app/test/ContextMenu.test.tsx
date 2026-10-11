// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ContextMenu } from '../src/renderer/components/ContextMenu'
import { useUi } from '../src/renderer/stores/ui'

afterEach(cleanup)

describe('ContextMenu', () => {
  beforeEach(() => useUi.setState(useUi.getInitialState()))

  it('opens at the pointer, selects with Enter and closes', () => {
    const play = vi.fn()
    render(<ContextMenu />)
    act(() => useUi.getState().openMenu(10, 20, [{ label: 'Play next', onSelect: play }, { label: 'Disabled', disabled: true }]))
    const menu = screen.getByRole('menu')
    expect(screen.getAllByRole('menuitem')).toHaveLength(2)
    fireEvent.keyDown(menu, { key: 'Enter' })
    expect(play).toHaveBeenCalled()
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('moves with the arrow keys, skipping disabled items, and closes on Escape', () => {
    const second = vi.fn()
    render(<ContextMenu />)
    act(() => useUi.getState().openMenu(0, 0, [{ label: 'A' }, { label: 'B', disabled: true }, { label: 'C', onSelect: second }]))
    const menu = screen.getByRole('menu')
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    fireEvent.keyDown(menu, { key: 'Enter' })
    expect(second).toHaveBeenCalled()
    act(() => useUi.getState().openMenu(0, 0, [{ label: 'A' }]))
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('opens a submenu with ArrowRight', () => {
    const pick = vi.fn()
    render(<ContextMenu />)
    act(() => useUi.getState().openMenu(0, 0, [{ label: 'Add to playlist', items: [{ label: 'Mix', onSelect: pick }] }]))
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowRight' })
    const menus = screen.getAllByRole('menu')
    expect(menus).toHaveLength(2)
    fireEvent.keyDown(menus[1], { key: 'Enter' })
    expect(pick).toHaveBeenCalled()
  })
})
