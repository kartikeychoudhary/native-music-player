import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { inflateSync } from 'node:zlib'
import { describe, expect, it } from 'vitest'
import { encodePng } from '../build-tools/png.ts'
import { ICON_INSET, ICON_SIZE, iconPng } from '../build-tools/make-icon.ts'

/** RGBA of pixel (x, y) from a PNG written by encodePng (8-bit RGBA, filter 0 on every row). */
function pixel(png: Buffer, x: number, y: number): number[] {
  const width = png.readUInt32BE(16)
  let idat = Buffer.alloc(0)
  for (let o = 8; o < png.length;) {
    const len = png.readUInt32BE(o), type = png.toString('ascii', o + 4, o + 8)
    if (type === 'IDAT') idat = Buffer.concat([idat, png.subarray(o + 8, o + 8 + len)])
    o += 12 + len
  }
  const raw = inflateSync(idat), stride = width * 4 + 1, at = y * stride + 1 + x * 4
  return [...raw.subarray(at, at + 4)]
}

describe('app icon', () => {
  it('encodes a valid RGBA PNG', () => {
    const png = encodePng(2, 1, new Uint8Array([0x47, 0xb5, 0xff, 0xff, 0, 0, 0, 0]))
    expect([...png.subarray(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
    expect(png.toString('ascii', 12, 16)).toBe('IHDR')
    expect([png.readUInt32BE(16), png.readUInt32BE(20), png[24], png[25]]).toEqual([2, 1, 8, 6])
    expect(pixel(png, 0, 0)).toEqual([0xff, 0xb5, 0x47, 0xff]) // BGRA in, RGBA out: amber #FFB547
  })
  it('is an amber square with a dark play triangle inside a transparent margin', () => {
    const png = iconPng()
    expect(png.readUInt32BE(16)).toBe(ICON_SIZE)
    expect(pixel(png, 10, 10)[3]).toBe(0) // margin
    expect(pixel(png, ICON_INSET + 60, ICON_SIZE / 2)).toEqual([0xff, 0xb5, 0x47, 0xff]) // left edge of the square: amber
    expect(pixel(png, ICON_SIZE / 2, ICON_SIZE / 2)).toEqual([0x17, 0x15, 0x1a, 0xff]) // centre: dark triangle
  })
  it('is committed up to date (run node app/build-tools/make-icon.ts)', () => {
    expect(readFileSync(join(__dirname, '../build/icon.png')).equals(iconPng())).toBe(true)
  })
})
