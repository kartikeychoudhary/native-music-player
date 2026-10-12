// No imports: Node loads this file directly from the build tools (make-icon.ts) as well as through the app bundle.

const AMBER = [0x47, 0xb5, 0xff, 0xff] // #FFB547 as BGRA
const DARK = [0x1a, 0x15, 0x17, 0xff] // #17151A as BGRA

/**
 * BGRA pixels: a rounded amber square with a dark play triangle (drawn in code, so no asset to ship).
 * The square fills the inner `size - 2 * inset` area; the `inset` margin around it stays transparent.
 */
export function iconPixels(size: number, inset = 0): Uint8Array {
  const px = new Uint8Array(size * size * 4)
  const n = size - 2 * inset // side of the drawn square
  const r = n * 0.22 // corner radius
  const inCorner = (x: number, y: number) => {
    const cx = x < r ? r : x > n - 1 - r ? n - 1 - r : x
    const cy = y < r ? r : y > n - 1 - r ? n - 1 - r : y
    return (x - cx) ** 2 + (y - cy) ** 2 > r * r
  }
  // Play triangle: points (0.36, 0.26), (0.36, 0.74), (0.74, 0.5) of the square.
  const inTriangle = (x: number, y: number) => {
    const u = (x + 0.5) / n, v = (y + 0.5) / n
    if (u < 0.36 || u > 0.74) return false
    const half = 0.24 * (1 - (u - 0.36) / 0.38)
    return Math.abs(v - 0.5) <= half
  }
  for (let y = 0; y < n; y++) {
    for (let x = 0; x < n; x++) {
      if (inCorner(x, y)) continue
      px.set(inTriangle(x, y) ? DARK : AMBER, ((y + inset) * size + x + inset) * 4)
    }
  }
  return px
}
