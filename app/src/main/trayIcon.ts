import { nativeImage, type NativeImage } from 'electron'
import { iconPixels } from '@shared/iconPixels'

/** BGRA pixels: a rounded amber square with a dark play triangle (drawn in code, so no asset to ship). */
export function trayIconPixels(size: number): Buffer { return Buffer.from(iconPixels(size)) }

export function trayIcon(): NativeImage {
  const img = nativeImage.createFromBitmap(trayIconPixels(16), { width: 16, height: 16, scaleFactor: 1 })
  img.addRepresentation({ scaleFactor: 2, width: 32, height: 32, buffer: trayIconPixels(32) })
  return img
}
