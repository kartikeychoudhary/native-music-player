import { describe, expect, it } from 'vitest'
import { StreamDecoder } from '../src/main/engine/streamCodec'

const enc = new TextEncoder()
const frame = (type: number, data: number[]) => { const b = new Uint8Array(5 + 1 + data.length); b[0] = 0x1e; new DataView(b.buffer).setUint32(1, 1 + data.length, true); b[5] = type; b.set(data, 6); return b }
const cat = (...parts: Uint8Array[]) => { const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let o = 0; for (const p of parts) { out.set(p, o); o += p.length } return out }

describe('StreamDecoder', () => {
  it('splits JSON lines and binary frames that arrive interleaved', () => {
    const d = new StreamDecoder()
    const bytes = cat(enc.encode('{"a":1}\r\n\n'), frame(1, [9, 8, 7]), enc.encode('{"b":2}\n'))
    expect(d.push(bytes)).toEqual([
      { kind: 'line', text: '{"a":1}' },
      { kind: 'frame', type: 1, data: new Uint8Array([9, 8, 7]) },
      { kind: 'line', text: '{"b":2}' },
    ])
  })

  it('reassembles across arbitrary chunk boundaries, including inside headers and characters', () => {
    const all = cat(enc.encode('{"t":"é—✓"}\n'), frame(1, Array.from({ length: 64 }, (_, i) => i)), enc.encode('{"z":0}\n'))
    for (let cut = 1; cut < all.length; cut++) {
      const d = new StreamDecoder()
      const out = [...d.push(all.slice(0, cut)), ...d.push(all.slice(cut))]
      expect(out.map((c) => c.kind)).toEqual(['line', 'frame', 'line'])
      expect(out[0]).toEqual({ kind: 'line', text: '{"t":"é—✓"}' })
      expect((out[1] as { data: Uint8Array }).data.length).toBe(64)
    }
  })
})
