// @vitest-environment jsdom
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { usePlayer } from '../src/renderer/stores/player'
import { useUi } from '../src/renderer/stores/ui'
import { DSP_DEFAULT, fakeApi, renderAt } from './helpers'

afterEach(cleanup)

describe('sound settings', () => {
  it('edits the EQ, presets, crossfade and ReplayGain through the engine', async () => {
    const { api, calls } = fakeApi()
    await renderAt('/settings', api)
    const eq = await screen.findByRole('group', { name: 'Equalizer' })
    fireEvent.click(within(eq).getByRole('switch', { name: 'Equalizer' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['dsp.setEqEnabled', { enabled: true }]))
    fireEvent.change(within(eq).getByRole('slider', { name: '1 kHz' }), { target: { value: '3.5' } })
    await vi.waitFor(() => expect(calls).toContainEqual(['dsp.setEq', { bands: [0, 0, 0, 0, 0, 3.5, 0, 0, 0, 0], preamp: 0 }]))
    expect((within(eq).getByRole('combobox', { name: 'EQ preset' }) as HTMLSelectElement).value, 'a manual change shows Custom').toBe('')
    fireEvent.change(within(eq).getByRole('combobox', { name: 'EQ preset' }), { target: { value: 'Bass' } })
    await vi.waitFor(() => expect(calls).toContainEqual(['dsp.setEq', { bands: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0], preamp: 0, preset: 'Bass' }]))
    fireEvent.click(within(eq).getByRole('button', { name: 'Save preset' }))
    useUi.getState().prompt!.resolve('Road')
    await vi.waitFor(() => expect(calls).toContainEqual(['dsp.savePreset', { name: 'Road' }]))
    fireEvent.change(screen.getByRole('slider', { name: 'Crossfade' }), { target: { value: '4' } })
    await vi.waitFor(() => expect(calls).toContainEqual(['dsp.setCrossfade', { seconds: 4 }]))
    fireEvent.click(screen.getByRole('radio', { name: 'Album' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['dsp.setReplayGain', { mode: 'album' }]))
  })

  it('follows dsp.changed from other windows and only deletes user presets', async () => {
    const { api, emit, calls } = fakeApi()
    await renderAt('/settings', api)
    const eq = await screen.findByRole('group', { name: 'Equalizer' })
    expect((within(eq).getByRole('button', { name: 'Delete preset' }) as HTMLButtonElement).disabled).toBe(true)
    emit('dsp.changed', { ...DSP_DEFAULT, eq: { enabled: true, bands: [1, 1, 1, 1, 1, 1, 1, 1, 1, 1], preamp: -1, preset: 'Mine' }, crossfadeSecs: 2.5 })
    expect((within(eq).getByRole('switch', { name: 'Equalizer' })).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByText('2.5 s')).toBeTruthy()
    fireEvent.click(within(eq).getByRole('button', { name: 'Delete preset' }))
    await vi.waitFor(() => expect(calls).toContainEqual(['dsp.deletePreset', { name: 'Mine' }]))
  })

  it('greys everything out in bit-perfect mode and says why', async () => {
    const { api } = fakeApi({ 'player.state': () => ({ ...usePlayer.getInitialState().state, mode: 'bitPerfect' }) })
    await renderAt('/settings', api)
    await screen.findByText(/Bit-perfect mode sends audio untouched/)
    expect((screen.getByRole('slider', { name: '1 kHz' }) as HTMLInputElement).disabled).toBe(true)
    expect((screen.getByRole('slider', { name: 'Crossfade' }) as HTMLInputElement).disabled).toBe(true)
    expect((screen.getByRole('radio', { name: 'Album' }) as HTMLInputElement).disabled).toBe(true)
  })
})
