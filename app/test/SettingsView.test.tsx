// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi, renderAt } from './helpers'

afterEach(cleanup)

describe('settings', () => {
  const folders = [{ id: 1, path: '/music', trackCount: 1200, lastScanAt: 1_700_000_000, available: true }, { id: 2, path: '/mnt/usb', trackCount: 40, lastScanAt: null, available: false }]

  it('manages library folders', async () => {
    const { api, calls } = fakeApi({ 'library.folders': () => folders })
    await renderAt('/settings', api)
    const list = await screen.findByRole('list', { name: 'Library folders' })
    expect(within(list).getByText('/music')).toBeTruthy()
    expect(within(list).getByText('Unavailable')).toBeTruthy()
    fireEvent.click(within(list).getAllByRole('button', { name: 'Rescan' })[0])
    await vi.waitFor(() => expect(calls).toContainEqual(['library.rescan', { folderId: 1 }]))
    fireEvent.click(within(list).getAllByRole('button', { name: 'Remove' })[1])
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Remove' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['library.removeFolder', { id: 2 }]))
    fireEvent.click(screen.getByRole('button', { name: 'Rescan all' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['library.rescan', {}]))
    api.openFolder.mockResolvedValueOnce('/more')
    fireEvent.click(screen.getByRole('button', { name: 'Add a music folder' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['library.addFolder', { path: '/more' }]))
  })

  it('switches output mode and lists shortcuts and licences', async () => {
    const { api, calls } = fakeApi()
    await renderAt('/settings', api)
    fireEvent.click(await screen.findByRole('radio', { name: /Bit-perfect/ }))
    await vi.waitFor(() => expect(calls).toContainEqual(['player.setMode', { mode: 'bitPerfect' }]))
    expect(screen.getByText('Ctrl/Cmd + F')).toBeTruthy()
    expect(screen.getByText(/Lucide/)).toBeTruthy()
  })

  it('shows a running scan on its folder and in the sidebar until it ends', async () => {
    const { api, emit } = fakeApi({ 'library.folders': () => folders })
    await renderAt('/settings', api)
    const list = await screen.findByRole('list', { name: 'Library folders' })
    emit('library.scanProgress', { folderId: 1, phase: 'reading', processed: 120, total: 900, added: 120, updated: 0, missing: 0 })
    const row = within(list).getByText('/music').closest('li')!
    expect(within(row).getByRole('progressbar', { name: 'Scanning 120 / 900' })).toBeTruthy()
    expect(within(screen.getByRole('navigation', { name: 'Main' })).getByRole('status').textContent).toContain('Scanning 120 / 900')
    emit('library.scanProgress', { folderId: 1, phase: 'done', processed: 900, total: 900, added: 900, updated: 0, missing: 0 })
    expect(within(row).queryByRole('progressbar')).toBeNull()
    expect(within(screen.getByRole('navigation', { name: 'Main' })).queryByRole('status')).toBeNull()
  })
})
