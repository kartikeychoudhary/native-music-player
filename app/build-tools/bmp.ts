/**
 * A 24-bit bottom-up BMP (the format NSIS wizard images require) from BGRA pixels (Electron's bitmap order).
 * BMP has no alpha channel, so each pixel is composited over `background` (RGB).
 */
export function encodeBmp24(width: number, height: number, bgra: Uint8Array, background: [number, number, number]): Buffer {
  const stride = Math.ceil((width * 3) / 4) * 4 // rows are padded to 4 bytes
  const header = Buffer.alloc(54)
  header.write('BM', 0, 'ascii')
  header.writeUInt32LE(54 + stride * height, 2) // file size
  header.writeUInt32LE(54, 10) // pixel data offset
  header.writeUInt32LE(40, 14) // BITMAPINFOHEADER size
  header.writeInt32LE(width, 18); header.writeInt32LE(height, 22) // positive height: bottom-up
  header.writeUInt16LE(1, 26); header.writeUInt16LE(24, 28) // planes, bits per pixel (compression and the rest stay 0)
  header.writeUInt32LE(stride * height, 34) // image size
  const pixels = Buffer.alloc(stride * height) // zero-filled, so the row padding is 0
  const [bgR, bgG, bgB] = background
  const over = (c: number, bg: number, a: number) => Math.round((c * a + bg * (255 - a)) / 255)
  for (let y = 0; y < height; y++) {
    const row = (height - 1 - y) * stride
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4, o = row + x * 3, a = bgra[i + 3]
      pixels[o] = over(bgra[i], bgB, a); pixels[o + 1] = over(bgra[i + 1], bgG, a); pixels[o + 2] = over(bgra[i + 2], bgR, a)
    }
  }
  return Buffer.concat([header, pixels])
}
