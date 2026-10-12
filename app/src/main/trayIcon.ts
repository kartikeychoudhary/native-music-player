import { nativeImage, type NativeImage } from 'electron'

const AMBER = [0x47, 0xb5, 0xff, 0xff] // #FFB547 as BGRA
const DARK = [0x1a, 0x15, 0x17, 0xff] // #17151A as BGRA

/** BGRA pixels: a rounded amber square with a dark play triangle (drawn in code, so no asset to ship). */
export function trayIconPixels(size: number): Buffer {
  const px = Buffer.alloc(size * size * 4)
  const r = size * 0.22 // corner radius
  const inCorner = (x: number, y: number) => {
    const cx = x < r ? r : x > size - 1 - r ? size - 1 - r : x
    const cy = y < r ? r : y > size - 1 - r ? size - 1 - r : y
    return (x - cx) ** 2 + (y - cy) ** 2 > r * r
  }
  // Play triangle: points (0.36, 0.26), (0.36, 0.74), (0.74, 0.5) of the square.
  const inTriangle = (x: number, y: number) => {
    const u = (x + 0.5) / size, v = (y + 0.5) / size
    if (u < 0.36 || u > 0.74) return false
    const half = 0.24 * (1 - (u - 0.36) / 0.38)
    return Math.abs(v - 0.5) <= half
  }
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      if (inCorner(x, y)) continue
      px.set(inTriangle(x, y) ? DARK : AMBER, (y * size + x) * 4)
    }
  }
  return px
}

export function trayIcon(): NativeImage {
  const img = nativeImage.createFromBitmap(trayIconPixels(16), { width: 16, height: 16, scaleFactor: 1 })
  img.addRepresentation({ scaleFactor: 2, width: 32, height: 32, buffer: trayIconPixels(32) })
  return img
}
