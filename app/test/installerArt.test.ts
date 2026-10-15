import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { encodeBmp24 } from '../build-tools/bmp.ts'

/** RGB of pixel (x, y) (top-left origin) in a 24-bit bottom-up BMP. */
function rgb(bmp: Buffer, x: number, y: number): number[] {
  const w = bmp.readInt32LE(18), h = bmp.readInt32LE(22), stride = Math.ceil((w * 3) / 4) * 4
  const at = bmp.readUInt32LE(10) + (h - 1 - y) * stride + x * 3
  return [bmp[at + 2], bmp[at + 1], bmp[at]]
}

/** Every pixel of a 24-bit BMP, as [r, g, b]. */
function pixels(bmp: Buffer): number[][] {
  const w = bmp.readInt32LE(18), h = bmp.readInt32LE(22), out: number[][] = []
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) out.push(rgb(bmp, x, y))
  return out
}

const amber = ([r, g, b]: number[]) => r > 0xe0 && g > 0x90 && g < 0xd0 && b < 0x80
const dark = ([r, g, b]: number[]) => r < 0x40 && g < 0x40 && b < 0x40
const light = ([r, g, b]: number[]) => r > 0xf0 && g > 0xf0 && b > 0xf0

const art = (name: string) => readFileSync(join(__dirname, '../build', name))

describe('installer art', () => {
  it('encodes a padded 24-bit bottom-up BMP, compositing alpha over the background', () => {
    const px = new Uint8Array([0x47, 0xb5, 0xff, 0xff, 0, 0, 0, 0, 0x47, 0xb5, 0xff, 0x80]) // amber, transparent, half amber (BGRA)
    const bmp = encodeBmp24(3, 1, px, [0x17, 0x15, 0x1a])
    expect(bmp.toString('ascii', 0, 2)).toBe('BM')
    expect([bmp.readInt32LE(18), bmp.readInt32LE(22), bmp.readUInt16LE(28)]).toEqual([3, 1, 24])
    expect(bmp.length).toBe(54 + 12) // 9 bytes of pixels padded to 12
    expect(rgb(bmp, 0, 0)).toEqual([0xff, 0xb5, 0x47])
    expect(rgb(bmp, 1, 0)).toEqual([0x17, 0x15, 0x1a])
    expect(rgb(bmp, 2, 0)[0]).toBeGreaterThan(0x17)
    expect(rgb(bmp, 2, 0)[0]).toBeLessThan(0xff)
  })

  // The committed BMPs are rendered from build-tools/installer-art/art.html (node app/build-tools/make-installer-art.ts).
  // Text rendering differs between machines, so these check the composition rather than exact bytes.
  it('the sidebar is a 164 x 314 charcoal panel with the amber mark, a light wordmark and the spectrum bars', () => {
    const s = art('installerSidebar.bmp')
    expect([s.readInt32LE(18), s.readInt32LE(22), s.readUInt16LE(28)]).toEqual([164, 314, 24])
    expect(dark(rgb(s, 2, 2))).toBe(true)
    expect(dark(rgb(s, 2, 311))).toBe(true)
    const all = pixels(s)
    expect(all.filter(amber).length).toBeGreaterThan(500) // the mark plus the bars
    expect(all.filter(light).length).toBeGreaterThan(40) // the "Aurora" wordmark
    const bottom = all.slice(164 * 240) // the lowest rows hold the spectrum bars
    expect(bottom.filter(amber).length).toBeGreaterThan(30)
  })

  it('the header is a 150 x 57 white strip with the amber mark and a dark wordmark', () => {
    const h = art('installerHeader.bmp')
    expect([h.readInt32LE(18), h.readInt32LE(22), h.readUInt16LE(28)]).toEqual([150, 57, 24])
    expect(light(rgb(h, 1, 1))).toBe(true)
    const all = pixels(h)
    expect(all.filter(amber).length).toBeGreaterThan(150)
    // The wordmark sits left of x = 100, where the old mark-only header had nothing dark.
    const leftDark = all.filter((p, i) => i % 150 < 100 && dark(p))
    expect(leftDark.length).toBeGreaterThan(40)
  })

  it('ships the license page text identical to the repository LICENSE', () => {
    expect(readFileSync(join(__dirname, '../build/license.txt'), 'utf8').replace(/\r\n/g, '\n')).toBe(readFileSync(join(__dirname, '../../LICENSE'), 'utf8').replace(/\r\n/g, '\n'))
  })
})
