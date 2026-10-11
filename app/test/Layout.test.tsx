// @vitest-environment jsdom
import { cleanup, fireEvent, screen } from '@testing-library/react'
import { act } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { fakeApi, renderAt } from './helpers'

afterEach(cleanup)

describe('layout and routing', () => {
  it('routes to the signal path view and marks the nav item current', async () => {
    await renderAt('/signal')
    expect(await screen.findByText('Play something to see its signal path.')).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Signal path' }).getAttribute('aria-current')).toBe('page')
  })

  it('shows a not-found view for unknown routes', async () => {
    await renderAt('/nowhere')
    expect(await screen.findByText('This view does not exist.')).toBeTruthy()
  })

  it('persists the current view and the sidebar state', async () => {
    const { api } = fakeApi()
    const { router } = await renderAt('/', api)
    await act(async () => { await router.navigate({ to: '/signal' }) })
    await vi.waitFor(() => expect(api.setSettings).toHaveBeenCalledWith({ lastView: '/signal' }))
    fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }))
    expect(useUi.getState().sidebarExpanded).toBe(true)
    expect(api.setSettings).toHaveBeenCalledWith({ sidebarExpanded: true })
  })

  it('turns engine errors into toasts', async () => {
    await renderAt('/')
    act(() => usePlayer.setState({ error: { code: -32002, name: 'FILE_MISSING', message: 'gone', path: '/m/a.flac' } }))
    expect(await screen.findByRole('status')).toBeTruthy()
    expect(screen.getByText(/gone/)).toBeTruthy()
  })
})
