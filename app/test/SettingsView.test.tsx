// @vitest-environment jsdom
import { act, cleanup, fireEvent, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useUpdates } from '../src/renderer/stores/updates'
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

describe('About', () => {
  const info = (extra: Record<string, unknown> = {}) => ({ version: '0.1.0', variant: 'standard', platform: 'win32', arch: 'x64', licenses: true, updateMode: 'notify', ...extra })

  it('shows the app and engine versions and whether HE-AAC is in this build', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info() as never)
    api.status.mockResolvedValue({ state: 'ready', engine: { version: '0.1.0', protocolVersion: 3, features: ['opus', 'fdk-aac'] } } as never)
    await renderAt('/settings', api)
    expect(await screen.findByText('Aurora 0.1.0')).toBeTruthy()
    expect(screen.getByText('Engine 0.1.0')).toBeTruthy()
    expect(screen.getByText('Included (Fraunhofer FDK AAC)')).toBeTruthy()
    expect(screen.getByText('win32-x64')).toBeTruthy()
  })
  it('says when HE-AAC is not in this build and labels the no-fdk variant', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info({ variant: 'nofdk', platform: 'linux', arch: 'arm64', licenses: false }) as never)
    api.status.mockResolvedValue({ state: 'ready', engine: { version: '0.1.0', protocolVersion: 3, features: ['opus'] } } as never)
    await renderAt('/settings', api)
    expect(await screen.findByText('Not in this build')).toBeTruthy()
    expect(await screen.findByText('linux-arm64 · without fdk-aac')).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Third-party licenses' }) as HTMLButtonElement).disabled).toBe(true)
  })
  it('opens the license file', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info({ platform: 'darwin', arch: 'arm64' }) as never)
    await renderAt('/settings', api)
    await screen.findByText('Aurora 0.1.0')
    fireEvent.click(screen.getByRole('button', { name: 'Third-party licenses' }))
    expect(api.openLicenses).toHaveBeenCalled()
  })
})

describe('About: updates', () => {
  const info = (extra: Record<string, unknown> = {}) => ({ version: '0.1.0', variant: 'standard', platform: 'win32', arch: 'x64', licenses: true, updateMode: 'notify', ...extra })

  it('says updates are off in development and offers no check', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info({ updateMode: 'off' }) as never)
    await renderAt('/settings', api)
    expect(await screen.findByText('Updates are off in development builds.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Check now' })).toBeNull()
    expect(screen.queryByRole('checkbox', { name: 'Check for updates automatically' })).toBeNull()
  })
  it('checks on demand and explains where notify-mode downloads come from', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info() as never)
    await renderAt('/settings', api)
    expect(await screen.findByText('New versions are downloaded from the GitHub release page.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Check now' }))
    expect(api.checkForUpdates).toHaveBeenCalled()
  })
  it('does not offer the release-page note in auto mode', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info({ updateMode: 'auto' }) as never)
    await renderAt('/settings', api)
    await screen.findByRole('button', { name: 'Check now' })
    expect(screen.queryByText('New versions are downloaded from the GitHub release page.')).toBeNull()
  })
  it('shows the status of the last check, and what went wrong', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info() as never)
    await renderAt('/settings', api)
    await screen.findByRole('button', { name: 'Check now' })
    act(() => useUpdates.setState({ state: { status: 'current' } }))
    expect(screen.getByText('Aurora is up to date.')).toBeTruthy()
    act(() => useUpdates.setState({ state: { status: 'error', message: 'GitHub answered 403' } }))
    expect(screen.getByText("Couldn't check for updates: GitHub answered 403")).toBeTruthy()
    act(() => useUpdates.setState({ state: { status: 'downloading', version: '0.2.0', percent: 42 } }))
    expect(screen.getByText('Downloading Aurora 0.2.0… 42%')).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Check now' }) as HTMLButtonElement).disabled).toBe(true)
  })
  it('turns automatic checks off and on', async () => {
    const { api } = fakeApi()
    api.appInfo.mockResolvedValue(info() as never)
    await renderAt('/settings', api)
    const box = await screen.findByRole('checkbox', { name: 'Check for updates automatically' })
    expect((box as HTMLInputElement).checked).toBe(true)
    fireEvent.click(box)
    expect(api.setSettings).toHaveBeenCalledWith({ autoUpdate: false })
    expect((screen.getByRole('checkbox', { name: 'Check for updates automatically' }) as HTMLInputElement).checked).toBe(false)
  })
})
