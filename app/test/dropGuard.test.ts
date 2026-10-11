// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { installDropGuard } from '../src/renderer/lib/dropGuard'

function fire(target: EventTarget, type: string) {
  const e = new Event(type, { bubbles: true, cancelable: true })
  target.dispatchEvent(e)
  return e
}

describe('installDropGuard', () => {
  let uninstall: (() => void) | undefined
  afterEach(() => { uninstall?.(); document.body.innerHTML = '' })

  it('cancels dragover and drop anywhere in the document, so a dropped file never navigates', () => {
    uninstall = installDropGuard(document)
    const el = document.body.appendChild(document.createElement('div'))
    expect(fire(el, 'dragover').defaultPrevented).toBe(true)
    expect(fire(el, 'drop').defaultPrevented).toBe(true)
  })

  it('still lets a drop zone handle the drop first', () => {
    uninstall = installDropGuard(document)
    const zone = document.body.appendChild(document.createElement('div'))
    let handled = 0
    zone.addEventListener('drop', (e) => { handled++; e.preventDefault() })
    fire(zone, 'drop')
    expect(handled).toBe(1)
  })

  it('can be uninstalled', () => {
    installDropGuard(document)()
    expect(fire(document.body, 'drop').defaultPrevented).toBe(false)
  })
})
