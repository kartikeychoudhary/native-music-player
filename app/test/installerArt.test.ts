import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { encodeBmp24 } from '../build-tools/bmp.ts'
import { headerBmp, sidebarBmp } from '../build-tools/make-installer-art.ts'

/** RGB of pixel (x, y) (top-left origin) in a 24-bit bottom-up BMP. */
function rgb(bmp: Buffer, x: number, y: number): number[] {
  const w = bmp.readInt32LE(18), h = bmp.readInt32LE(22), stride = Math.ceil((w * 3) / 4) * 4
  const at = bmp.readUInt32LE(10) + (h - 1 - y) * stride + x * 3
  return [bmp[at + 2], bmp[at + 1], bmp[at]]
}

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
  it('draws the sidebar on charcoal and the header on white, each with the amber mark', () => {
    const s = sidebarBmp(), h = headerBmp()
    expect([s.readInt32LE(18), s.readInt32LE(22)]).toEqual([164, 314])
    expect([h.readInt32LE(18), h.readInt32LE(22)]).toEqual([150, 57])
    expect(rgb(s, 2, 300)).toEqual([0x17, 0x15, 0x1a])
    expect(rgb(s, 82, 48 + 20)).toEqual([0xff, 0xb5, 0x47]) // the mark's upper body: amber
    expect(rgb(h, 5, 5)).toEqual([0xff, 0xff, 0xff])
    expect(rgb(h, 101 + 8, 8 + 20)).toEqual([0xff, 0xb5, 0x47])
  })
  it('is committed up to date (run node app/build-tools/make-installer-art.ts)', () => {
    expect(readFileSync(join(__dirname, '../build/installerSidebar.bmp')).equals(sidebarBmp())).toBe(true)
    expect(readFileSync(join(__dirname, '../build/installerHeader.bmp')).equals(headerBmp())).toBe(true)
  })
  it('ships the license page text identical to the repository LICENSE', () => {
    expect(readFileSync(join(__dirname, '../build/license.txt'), 'utf8').replace(/\r\n/g, '\n')).toBe(readFileSync(join(__dirname, '../../LICENSE'), 'utf8').replace(/\r\n/g, '\n'))
  })
})
