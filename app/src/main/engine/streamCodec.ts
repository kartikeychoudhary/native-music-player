/** The engine's stdout (spec §4.1): newline-delimited JSON lines interleaved with binary frames
 * (0x1E, u32 LE length, payload = [type, ...data]). JSON lines never start with 0x1E. */
export type Chunk = { kind: 'line'; text: string } | { kind: 'frame'; type: number; data: Uint8Array }

const MARK = 0x1e
const NL = 0x0a

export class StreamDecoder {
  private buf = new Uint8Array(0)
  private text = new TextDecoder('utf-8')

  push(chunk: Uint8Array): Chunk[] {
    const next = new Uint8Array(this.buf.length + chunk.length)
    next.set(this.buf)
    next.set(chunk, this.buf.length)
    this.buf = next
    const out: Chunk[] = []
    let pos = 0
    for (;;) {
      if (pos >= this.buf.length) break
      if (this.buf[pos] === MARK) {
        if (this.buf.length - pos < 5) break
        const len = new DataView(this.buf.buffer, this.buf.byteOffset + pos + 1, 4).getUint32(0, true)
        if (this.buf.length - pos < 5 + len) break
        const payload = this.buf.slice(pos + 5, pos + 5 + len)
        if (len > 0) out.push({ kind: 'frame', type: payload[0], data: payload.slice(1) })
        pos += 5 + len
        continue
      }
      const nl = this.buf.indexOf(NL, pos)
      if (nl < 0) break
      const line = this.text.decode(this.buf.subarray(pos, nl)).replace(/\r$/, '')
      if (line.length > 0) out.push({ kind: 'line', text: line })
      pos = nl + 1
    }
    this.buf = this.buf.slice(pos)
    return out
  }
}
