import { describe, expect, it } from 'vitest'
import { LineDecoder } from '../src/main/engine/lineCodec'

describe('LineDecoder', () => {
  it('splits complete lines and buffers partial ones', () => {
    const d = new LineDecoder()
    expect(d.push('{"a":1}\n{"b"')).toEqual(['{"a":1}'])
    expect(d.push(':2}\r\n\n')).toEqual(['{"b":2}'])
    expect(d.push('')).toEqual([])
  })
})
