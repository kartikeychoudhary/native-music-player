const FALL = 0.06 // max drop per frame, as a fraction of full scale

/** Next bar heights (0–1): rise instantly, fall gradually; with reduced motion, no smoothing. */
export function smooth(prev: Float32Array, next: Uint8Array, reduced: boolean): Float32Array {
  const out = new Float32Array(next.length)
  for (let i = 0; i < next.length; i++) {
    const v = next[i] / 255
    out[i] = reduced ? v : Math.max(v, (prev[i] ?? 0) - FALL)
  }
  return out
}
