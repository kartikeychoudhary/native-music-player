// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { UpdateBanner } from '../src/renderer/components/UpdateBanner'
import { useUpdates } from '../src/renderer/stores/updates'
import { fakeApi } from './helpers'

afterEach(cleanup)

describe('update banner', () => {
  beforeEach(() => useUpdates.setState({ state: { status: 'idle' }, dismissed: null }))
  it('shows nothing until an update is available or ready', () => {
    for (const s of [{ status: 'idle' }, { status: 'checking' }, { status: 'current' }, { status: 'error', message: 'x' }, { status: 'downloading', version: '0.2.0', percent: 3 }] as const) {
      act(() => useUpdates.setState({ state: s }))
      const { container, unmount } = render(<UpdateBanner />)
      expect(container.textContent).toBe(''); unmount()
    }
  })
  it('offers a restart when an update is downloaded, and a download link when one is available', () => {
    const installUpdate = vi.fn(async () => {}), openRelease = vi.fn(async () => {})
    useUpdates.setState({ api: { installUpdate, openRelease } as never, state: { status: 'ready', version: '0.2.0' } })
    const { rerender } = render(<UpdateBanner />)
    expect(screen.getByText('Aurora 0.2.0 is ready to install.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Restart now' })); expect(installUpdate).toHaveBeenCalled()
    act(() => useUpdates.setState({ state: { status: 'available', version: '0.3.0' } })); rerender(<UpdateBanner />)
    fireEvent.click(screen.getByRole('button', { name: 'Download' })); expect(openRelease).toHaveBeenCalled()
  })
  it('stays dismissed for that version only', () => {
    useUpdates.setState({ state: { status: 'available', version: '0.2.0' } })
    const { rerender, container } = render(<UpdateBanner />)
    fireEvent.click(screen.getByRole('button', { name: 'Later' }))
    expect(container.textContent).toBe('')
    act(() => useUpdates.setState({ state: { status: 'available', version: '0.2.1' } })); rerender(<UpdateBanner />)
    expect(screen.getByText('Aurora 0.2.1 is available.')).toBeTruthy()
  })
})

describe('update store', () => {
  beforeEach(() => useUpdates.setState(useUpdates.getInitialState()))
  it('loads the current state and follows update.state events', async () => {
    const { api, emit } = fakeApi()
    api.updateState.mockResolvedValue({ status: 'current' })
    const off = useUpdates.getState().init(api)
    await vi.waitFor(() => expect(useUpdates.getState().state).toEqual({ status: 'current' }))
    emit('update.state', { status: 'available', version: '0.2.0' })
    expect(useUpdates.getState().state).toEqual({ status: 'available', version: '0.2.0' })
    off()
    emit('update.state', { status: 'idle' })
    expect(useUpdates.getState().state).toEqual({ status: 'available', version: '0.2.0' })
  })
})
