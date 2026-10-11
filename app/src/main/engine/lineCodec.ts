/** Splits a UTF-8 text stream into newline-delimited lines (CRLF tolerated, blank lines dropped). */
export class LineDecoder {
  private buf = ''
  push(chunk: string): string[] {
    this.buf += chunk
    const parts = this.buf.split('\n')
    this.buf = parts.pop() ?? ''
    return parts.map((l) => l.replace(/\r$/, '')).filter((l) => l.length > 0)
  }
}
