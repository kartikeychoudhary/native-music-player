import type { LyricLine } from '@protocol/LyricLine'
import type { LyricWord } from '@protocol/LyricWord'

function lastAtOrBefore<T>(items: T[], ms: number, t: (x: T) => number | undefined): number {
  let lo = 0, hi = items.length - 1, found = -1
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    const v = t(items[mid])
    if (v !== undefined && v <= ms) { found = mid; lo = mid + 1 } else hi = mid - 1
  }
  return found
}

/** Index of the line being sung at `ms` (lines are sorted by time), or -1 before the first. */
export const activeLine = (lines: LyricLine[], ms: number) => lastAtOrBefore(lines, ms, (l) => l.tMs ?? undefined)
export const activeWord = (words: LyricWord[], ms: number) => lastAtOrBefore(words, ms, (w) => w.tMs)
