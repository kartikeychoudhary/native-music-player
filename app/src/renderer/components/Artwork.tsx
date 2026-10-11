import { Disc3 } from 'lucide-react'
import { useState } from 'react'
import { artUrl, type ArtSize } from '../lib/art'
import s from './ui.module.css'

export function Artwork({ id, size, alt, className, px }: { id: number | null | undefined; size: ArtSize; alt: string; className?: string; px?: number }) {
  const [failed, setFailed] = useState<number | null>(null)
  const dim = px ?? size
  if (id == null || failed === id) {
    return <div className={`${s.artFallback} ${className ?? ''}`} style={{ width: dim, height: dim }} role="img" aria-label={alt}><Disc3 size={Math.max(16, dim / 3)} /></div>
  }
  return <img className={`${s.art} ${className ?? ''}`} src={artUrl(id, size)} alt={alt} width={dim} height={dim} loading="lazy" draggable={false} onError={() => setFailed(id)} />
}
