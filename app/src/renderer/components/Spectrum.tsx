import { useEffect, useRef } from 'react'
import { smooth } from '../lib/spectrum'
import { usePlayer } from '../stores/player'

const reducedMotion = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches

/** 64-band spectrum (spec §3.6) drawn on a canvas. Subscribes only while mounted and visible. */
export function Spectrum({ className, height = 120 }: { className?: string; height?: number }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const api = usePlayer((p) => p.api)
  useEffect(() => {
    if (!api) return
    let bars: Float32Array = new Float32Array(64)
    let frames = 0
    let raf = 0
    const reduced = reducedMotion()
    const draw = () => {
      raf = 0
      const c = canvas.current
      const ctx = c?.getContext('2d')
      if (!c || !ctx) return
      const dpr = window.devicePixelRatio || 1
      // Whole pixels: at a fractional DPR (125%/150%) the raw product never equals the integer c.width, so it would reallocate every frame.
      const w = Math.round(c.clientWidth * dpr), h = Math.round(c.clientHeight * dpr)
      if (c.width !== w || c.height !== h) { c.width = w; c.height = h }
      ctx.clearRect(0, 0, w, h)
      ctx.fillStyle = getComputedStyle(c).getPropertyValue('--accent').trim() || '#FFB547'
      const gap = 2 * dpr
      const bw = (w - gap * (bars.length - 1)) / bars.length
      bars.forEach((v, i) => { const bh = Math.max(dpr, v * h); ctx.fillRect(i * (bw + gap), h - bh, bw, bh) })
    }
    const off = api.onSpectrum((bins) => {
      bars = smooth(bars, bins, reduced)
      frames += 1
      if (canvas.current) canvas.current.dataset.frames = String(frames)
      if (!raf && typeof requestAnimationFrame === 'function') raf = requestAnimationFrame(draw)
    })
    const sync = () => api.setViz(document.visibilityState !== 'hidden')
    sync()
    document.addEventListener('visibilitychange', sync)
    return () => {
      document.removeEventListener('visibilitychange', sync)
      off()
      api.setViz(false)
      if (raf && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(raf)
    }
  }, [api])
  return <canvas ref={canvas} role="img" aria-label="Spectrum" className={className} style={{ width: '100%', height, display: 'block' }} data-frames="0" />
}
