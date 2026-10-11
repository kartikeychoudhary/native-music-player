import { describe, expect, it } from 'vitest'
import { shouldBlockNavigation } from '../src/main/navigation'

describe('shouldBlockNavigation', () => {
  const packaged = 'file:///C:/Program%20Files/Aurora/resources/app/out/renderer/index.html'

  it('blocks a dropped file', () => {
    expect(shouldBlockNavigation('file:///C:/Music/a.flac', packaged)).toBe(true)
    expect(shouldBlockNavigation('file:///home/me/a.flac', 'file:///opt/aurora/out/renderer/index.html')).toBe(true)
  })

  it('blocks remote and other-scheme URLs', () => {
    expect(shouldBlockNavigation('https://example.com/', packaged)).toBe(true)
    expect(shouldBlockNavigation('javascript:alert(1)', packaged)).toBe(true)
    expect(shouldBlockNavigation('not a url', packaged)).toBe(true)
  })

  it('allows the app page itself (reload, hash changes)', () => {
    expect(shouldBlockNavigation(packaged, packaged)).toBe(false)
    expect(shouldBlockNavigation(`${packaged}#queue`, packaged)).toBe(false)
  })

  it('allows the dev server origin and nothing else', () => {
    expect(shouldBlockNavigation('http://localhost:5173/', 'http://localhost:5173/')).toBe(false)
    expect(shouldBlockNavigation('http://localhost:5173/index.html', 'http://localhost:5173')).toBe(false)
    expect(shouldBlockNavigation('http://localhost:5174/', 'http://localhost:5173/')).toBe(true)
    expect(shouldBlockNavigation('file:///C:/Music/a.flac', 'http://localhost:5173/')).toBe(true)
  })
})
