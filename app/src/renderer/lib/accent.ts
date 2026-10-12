const channel = (c: number) => { const s = c / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4 }
const luminance = (hex: string) => {
  const n = parseInt(hex.replace('#', ''), 16)
  return 0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255)
}

/** WCAG 2.x contrast ratio between two #rrggbb colours. */
export function contrastRatio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

/** The accent to use for album art's vibrant colour: only when the setting is on and it reads ≥ 4.5:1 on the background (spec §5.5, §5.7). */
export function tintFor(vibrant: string | null | undefined, enabled: boolean, bg = '#17151A'): string | null {
  if (!enabled || !vibrant || !/^#[0-9a-f]{6}$/i.test(vibrant)) return null
  return contrastRatio(vibrant, bg) >= 4.5 ? vibrant : null
}

export function applyAccent(color: string | null) {
  const root = document.documentElement
  if (color) root.style.setProperty('--accent', color)
  else root.style.removeProperty('--accent')
}
